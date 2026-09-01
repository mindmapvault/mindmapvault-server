mod config;
mod db;
mod error;
mod middleware;
mod models;
mod routes;

use std::sync::Arc;
// Optional: use jemalloc as global allocator to get jemalloc profiling support at runtime.
#[cfg(not(windows))]
use jemallocator::Jemalloc;

#[cfg(not(windows))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;


use axum::{
    http::{header, Method, StatusCode},
    middleware::from_fn,
    response::Redirect,
    routing::get,
    Router,
};
use tower_http::{
    cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use chrono::Utc;
use config::AppConfig;
use db::{minio::MinioClient, postgres::PostgresDb, sql_store::DynSqlStore};
use error::AppError;
use middleware::auth::{JwtService, KeyVersionCache};
use middleware::request_cleanup::release_request_caches;
use middleware::request_id::request_id_layer;
use middleware::static_cache::static_cache_headers;
use middleware::throttle::AuthThrottle;
use models::instance_settings::{InstanceSettings, InstanceSettingsHandle};
use models::status::PurgeStatusHandle;
use models::user::MAX_UPLOAD_BODY_BYTES;
use routes::{
    admin::{router as admin_router, AdminState},
    auth_sql::{router as auth_sql_router, AuthSqlState},
    mindmaps_sql::{router as mindmaps_sql_router, MindMapsSqlState},
    public::{router as public_router, PublicState},
    share_public::{router as share_public_router, SharePublicState},
};

/// How many expired shares to clear per sweep. Bounded so one pass cannot
/// stall on a large backlog.
const SHARE_PURGE_BATCH: i64 = 200;

/// How often the cached instance settings are re-read from the database.
///
/// Writes from the admin console update the cache directly, so this only
/// matters when more than one replica shares a database: the other replicas
/// pick the change up within this interval instead of at the next restart.
const SETTINGS_REFRESH_SECS: u64 = 60;

/// How often the throttle drops entries nobody is waiting on.
const THROTTLE_PRUNE_SECS: u64 = 5 * 60;

/// Deletes the stored ciphertext of shares that are revoked or past expiry.
///
/// A share blob is a second, independently-keyed copy of a map. Once the share
/// is over, that copy has no reason to exist — and it counts against the
/// owner's storage until it is gone.
pub(crate) async fn purge_expired_shares(
    store: &DynSqlStore,
    minio: &MinioClient,
    status: &PurgeStatusHandle,
) {
    let shares = match store
        .list_purgeable_mind_map_shares(Utc::now(), SHARE_PURGE_BATCH)
        .await
    {
        Ok(shares) if shares.is_empty() => {
            status.record(0, None);
            return;
        }
        Ok(shares) => shares,
        Err(error) => {
            tracing::warn!(?error, "share purge query failed");
            status.record(0, Some("could not read the list of expired shares".to_string()));
            return;
        }
    };

    let mut cleared = 0_usize;
    for share in shares {
        let attachments = match store.list_mind_map_share_attachments(&share.id).await {
            Ok(attachments) => attachments,
            Err(error) => {
                tracing::warn!(?error, "could not list share attachments");
                continue;
            }
        };

        let mut failed = false;
        for attachment in &attachments {
            if let Err(error) = minio.delete_object(&attachment.s3_key).await {
                if !matches!(error, AppError::NotFound(_)) {
                    tracing::warn!(?error, "share attachment delete failed");
                    failed = true;
                }
            }
        }

        if let Err(error) = minio.delete_object(&share.s3_key).await {
            if !matches!(error, AppError::NotFound(_)) {
                tracing::warn!(?error, "share blob delete failed");
                failed = true;
            }
        }

        // Only mark it done once the bytes are actually gone, so a storage
        // outage retries on the next sweep instead of orphaning the object.
        if failed {
            continue;
        }
        match store.mark_mind_map_share_purged(&share.id).await {
            Ok(()) => cleared += 1,
            Err(error) => tracing::warn!(?error, "could not mark share purged"),
        }
    }

    if cleared > 0 {
        tracing::info!(cleared, "purged expired share blobs");
    }
    status.record(cleared, None);
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

fn app_dist_dir() -> String {
    std::env::var("SERVER_APP_DIST_DIR").unwrap_or_else(|_| "/app/frontend_app_dist".to_string())
}

/// Filesystem the status page reports free space for.
///
/// Defaults to the container root, which under Docker's usual storage driver
/// sits on the same filesystem as the volumes holding Postgres and the object
/// store — so it is the number an operator wants. `STATUS_DISK_PATH` overrides
/// it for a deployment that puts its data somewhere else.
fn status_disk_path() -> String {
    std::env::var("STATUS_DISK_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/".to_string())
}

fn admin_dist_dir() -> String {
    std::env::var("SERVER_ADMIN_DIST_DIR").unwrap_or_else(|_| "/app/frontend_admin_dist".to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Logging ───────────────────────────────────────────────────────────────
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "backend=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Optional: pprof profiler guard is started when `ENABLE_PPROF` env var is set.
    #[cfg(not(windows))]
    use pprof::{protos::Message, ProfilerGuard};
    #[cfg(not(windows))]
    use std::io::Write;

    // Start a profiler guard when requested and write a pprof profile after the
    // configured duration (default 30s). We write to `/tmp/backend-profile.pb`,
    // which can be opened with `go tool pprof`, pprof.me or speedscope.
    #[cfg(not(windows))]
    if std::env::var("ENABLE_PPROF").is_ok() {
        let dur = std::env::var("PPROF_DURATION_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30);

        if let Ok(guard) = ProfilerGuard::new(100) {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(dur));
                if let Ok(report) = guard.report().build() {
                    if let Ok(profile) = report.pprof() {
                        if let Ok(bytes) = profile.write_to_bytes() {
                            if let Ok(mut file) = std::fs::File::create("/tmp/backend-profile.pb") {
                                let _ = file.write_all(&bytes);
                            }
                        }
                    }
                }
            });
        }
    }

    #[cfg(windows)]
    if std::env::var("ENABLE_PPROF").is_ok() {
        tracing::warn!("ENABLE_PPROF is set, but pprof is unavailable on Windows; skipping profiler setup");
    }

    // ── Config ────────────────────────────────────────────────────────────────
    let cfg = AppConfig::from_env()?;
    tracing::info!("Starting MindMapVault backend on {}", cfg.listen_addr());

    // ── Infra connections ─────────────────────────────────────────────────────
    let minio = MinioClient::connect(&cfg).await?;

    let jwt = Arc::new(JwtService::new(
        &cfg.jwt_secret,
        cfg.jwt_access_expiry_secs,
        cfg.jwt_refresh_expiry_secs,
    ));

    let db_engine = cfg.db_engine.to_lowercase();
    // SQL store initialization (PostgreSQL protocol-compatible backend)
    if db_engine != "sql" && db_engine != "postgres" && db_engine != "postgresql" {
        anyhow::bail!("Unsupported DB_ENGINE '{db_engine}'. Supported values: 'sql', 'postgres', 'postgresql'.");
    }

    let sql_store: Option<DynSqlStore> = Some(Arc::new(PostgresDb::connect(&cfg).await?));
    let sql_store_for_purge = sql_store.clone();
    let sql_store_for_settings = sql_store.clone();

    // ── Instance settings ─────────────────────────────────────────────────────
    // Seeded from the environment the first time this database is used and read
    // from the row on every start after that, so the admin console stays the
    // one place these are changed.
    let settings = {
        let store = sql_store.as_ref().expect("sql_store must be initialized");
        let stored = store
            .seed_instance_settings(&InstanceSettings::from_env_seed())
            .await?;
        log_effective_settings(&stored);
        InstanceSettingsHandle::new(stored)
    };
    let throttle = Arc::new(AuthThrottle::new());
    // User id → current key_version, so write requests can refuse sessions
    // that predate a password rotation without a DB read per request.
    let key_versions = KeyVersionCache::new();
    let purge_status = PurgeStatusHandle::new();
    let started_at = Utc::now();

    // ── CORS ──────────────────────────────────────────────────────────────────
    let allowed_origins: Vec<_> = cfg
        .cors_origins()
        .into_iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    if allowed_origins.is_empty() {
        tracing::warn!(
            "CORS_ALLOWED_ORIGINS is empty; only same-origin browser requests will be accepted"
        );
    }

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods(AllowMethods::list([
            Method::GET,
            Method::PATCH,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ]))
        .allow_headers(AllowHeaders::list([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
        ]))
        .allow_credentials(false);

    // ── Router ────────────────────────────────────────────────────────────────
    let app = {
        let sql_store = sql_store.expect("sql_store must be initialized");
        let app_dir = app_dist_dir();
        let admin_dir = admin_dist_dir();
        // Wrapped in a Router each so the cache-policy middleware sees the
        // path the static service was actually asked for — `nest_service`
        // strips the `/admin/` prefix before the inner service runs, which is
        // what makes one `/assets/` check correct for both apps.
        let app_static_service = Router::new()
            .fallback_service(
                ServeDir::new(&app_dir).fallback(ServeFile::new(format!("{app_dir}/index.html"))),
            )
            .layer(from_fn(static_cache_headers));
        let admin_static_service = Router::new()
            .fallback_service(
                ServeDir::new(&admin_dir)
                    .fallback(ServeFile::new(format!("{admin_dir}/index.html"))),
            )
            .layer(from_fn(static_cache_headers));

        let public_state = PublicState {
            settings: settings.clone(),
        };

        let share_public_state = SharePublicState {
            db: sql_store.clone(),
            minio: minio.clone(),
        };

        let admin_state = AdminState {
            db: sql_store.clone(),
            minio: minio.clone(),
            admin_api_token: cfg.admin_api_token.clone(),
            settings: settings.clone(),
            purge_status: purge_status.clone(),
            started_at,
            disk_path: status_disk_path(),
        };

        let auth_state = AuthSqlState {
            db: sql_store.clone(),
            minio: minio.clone(),
            jwt: jwt.clone(),
            settings: settings.clone(),
            throttle: throttle.clone(),
            key_versions: key_versions.clone(),
        };

        let mindmaps_state = MindMapsSqlState {
            db: sql_store.clone(),
            minio: minio.clone(),
            jwt: jwt.clone(),
            diagnostics_enabled: cfg.enable_diagnostics_routes,
            settings: settings.clone(),
            key_versions: key_versions.clone(),
        };

        Router::new()
            .route("/health", get(health))
            .route("/admin", get(|| async { Redirect::permanent("/admin/") }))
            .nest("/api/auth", auth_sql_router(auth_state))
            .nest("/api/admin", admin_router(admin_state))
            .nest("/api/mindmaps", mindmaps_sql_router(mindmaps_state))
            .nest("/api/public", public_router(public_state.clone()))
            // Unauthenticated: a share link is opened by a recipient who has no
            // account here. The ciphertext is useless without the passphrase.
            .nest("/share", share_public_router(share_public_state))
            .nest_service("/admin/", admin_static_service)
            .fallback_service(app_static_service)
            .layer(from_fn(release_request_caches))
            // The outer ceiling has to clear the largest per-route limit or it
            // silently clips it: a 10 MiB ceiling here would override the
            // attachment upload limit and the caller would only see a bare 413.
            .layer(RequestBodyLimitLayer::new(MAX_UPLOAD_BODY_BYTES))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                    .on_response(DefaultOnResponse::new().level(Level::INFO)),
            )
            .layer(from_fn(request_id_layer))
            .layer(cors)
    };

    // ── Retention ─────────────────────────────────────────────────────────────
    // Revoked shares are cleared inline at revoke time; this daily sweep catches
    // the ones that simply ran out of time, plus anything an inline delete
    // failed to remove.
    if let Some(store) = sql_store_for_purge {
        let purge_minio = minio.clone();
        let purge_status = purge_status.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
            loop {
                ticker.tick().await;
                purge_expired_shares(&store, &purge_minio, &purge_status).await;
            }
        });
    }

    // ── Settings refresh ──────────────────────────────────────────────────────
    if let Some(store) = sql_store_for_settings {
        let settings = settings.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_secs(SETTINGS_REFRESH_SECS));
            loop {
                ticker.tick().await;
                match store.load_instance_settings().await {
                    Ok(Some(stored)) => settings.set(stored),
                    // A missing row or a failed read leaves the cached copy in
                    // place: the last known-good settings are a better answer
                    // than reverting an instance to defaults.
                    Ok(None) => tracing::warn!("instance settings row is missing"),
                    Err(error) => tracing::warn!(?error, "could not refresh instance settings"),
                }
            }
        });
    }

    // ── Throttle housekeeping ─────────────────────────────────────────────────
    {
        let throttle = throttle.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_secs(THROTTLE_PRUNE_SECS));
            loop {
                ticker.tick().await;
                throttle.prune();
            }
        });
    }

    // ── Listen ────────────────────────────────────────────────────────────────
    let listener = tokio::net::TcpListener::bind(cfg.listen_addr()).await?;
    tracing::info!("Listening on http://{}", cfg.listen_addr());
    // Connection info is needed so the auth throttles can tell callers apart by
    // address; without it every request would land in one shared bucket.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// Says out loud, once, what the abuse controls are actually set to.
///
/// These are the settings whose effect is invisible until someone hits them, so
/// an operator reading the startup log should not have to open the admin
/// console to find out whether their instance is open to the world.
fn log_effective_settings(settings: &InstanceSettings) {
    let describe = |bytes: Option<i64>| {
        bytes.map_or_else(|| "unlimited".to_string(), |value| format!("{value} bytes"))
    };

    tracing::info!(
        registration_enabled = settings.registration_enabled,
        user_storage_limit = %describe(settings.storage_limit()),
        max_attachment_size = %describe(settings.attachment_limit()),
        auth_rate_limit_per_minute = settings.auth_rate_limit_per_minute,
        failed_login_threshold = settings.failed_login_threshold,
        "instance settings loaded (change these in the admin console)"
    );

    if settings.registration_enabled && settings.user_storage_limit_bytes == 0 {
        tracing::warn!(
            "registration is open and no per-account storage limit is set; \
             anyone who can reach this server can sign up and store without bound"
        );
    }

    if !settings.trust_proxy_headers && settings.auth_rate_limit_per_minute > 0 {
        tracing::info!(
            "auth throttling keys on the connecting address; if this instance sits behind a \
             reverse proxy, turn on trust_proxy_headers or every client will share one allowance"
        );
    }
}
