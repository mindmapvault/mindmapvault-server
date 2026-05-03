mod config;
mod collaboration;
mod db;
mod error;
mod middleware;
mod models;
mod routes;

use std::sync::Arc;
// Optional: use jemalloc as global allocator to get jemalloc profiling support at runtime
use jemallocator::Jemalloc;

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
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use config::AppConfig;
use db::{minio::MinioClient, postgres::PostgresDb, sql_store::DynSqlStore};
use middleware::auth::JwtService;
use middleware::request_cleanup::release_request_caches;
use routes::{
    admin::{router as admin_router, AdminState},
    auth_sql::{router as auth_sql_router, AuthSqlState},
    collaboration_sql::{router as collaboration_sql_router, CollaborationSqlState},
    mindmaps_sql::{router as mindmaps_sql_router, MindMapsSqlState},
    notifications::{router as notifications_router, NotificationsState},
    plaintext_sql::{router as plaintext_sql_router, PlainTextSqlState},
    public::{router as public_router, PublicState},
    share_public::{router as share_public_router, SharePublicState},
};

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

fn app_dist_dir() -> String {
    std::env::var("SERVER_APP_DIST_DIR").unwrap_or_else(|_| "/app/frontend_app_dist".to_string())
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
    use pprof::ProfilerGuard;
    use std::fs::File;

    // Start a profiler guard when requested and write a flamegraph SVG after the
    // configured duration (default 30s). We write to `/tmp/backend-flamegraph.svg`.
    if std::env::var("ENABLE_PPROF").is_ok() {
        let dur = std::env::var("PPROF_DURATION_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30);

        if let Ok(guard) = ProfilerGuard::new(100) {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(dur));
                if let Ok(report) = guard.report().build() {
                    if let Ok(mut file) = File::create("/tmp/backend-flamegraph.svg") {
                        let _ = report.flamegraph(&mut file);
                    }
                }
            });
        }
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
    let collaboration_hub = collaboration::PlaintextCollaborationHub::default();

    // SQL store initialization (PostgreSQL protocol-compatible backend)
    if db_engine != "sql" && db_engine != "postgres" && db_engine != "postgresql" {
        anyhow::bail!("Unsupported DB_ENGINE '{db_engine}'. Supported values: 'sql', 'postgres', 'postgresql'.");
    }

    let sql_store: Option<DynSqlStore> = Some(Arc::new(PostgresDb::connect(&cfg).await?));

    // ── CORS ──────────────────────────────────────────────────────────────────
    let allowed_origins: Vec<_> = cfg
        .cors_origins()
        .into_iter()
        .filter_map(|o| o.parse().ok())
        .collect();

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
        let app_static_service = ServeDir::new(&app_dir)
            .not_found_service(ServeFile::new(format!("{app_dir}/index.html")));
        let admin_static_service = ServeDir::new(&admin_dir)
            .not_found_service(ServeFile::new(format!("{admin_dir}/index.html")));

        let public_state = PublicState {
            db: sql_store.clone(),
        };

        let admin_state = AdminState {
            db: sql_store.clone(),
            minio: minio.clone(),
            admin_api_token: cfg.admin_api_token.clone(),
        };

        let auth_state = AuthSqlState {
            db: sql_store.clone(),
            minio: minio.clone(),
            jwt: jwt.clone(),
        };

        let mindmaps_state = MindMapsSqlState {
            db: sql_store.clone(),
            minio: minio.clone(),
            jwt: jwt.clone(),
            diagnostics_enabled: cfg.enable_diagnostics_routes,
        };

        let notifications_state = NotificationsState {
            db: sql_store.clone(),
            jwt: jwt.clone(),
        };

        let plaintext_state = PlainTextSqlState {
            db: sql_store.clone(),
            jwt: jwt.clone(),
        };

        let collaboration_state = CollaborationSqlState {
            db: sql_store.clone(),
            jwt: jwt.clone(),
            hub: collaboration_hub.clone(),
        };

        let share_public_state = SharePublicState {
            db: sql_store.clone(),
            minio: minio.clone(),
        };

        Router::new()
            .route("/health", get(health))
            .route("/admin", get(|| async { Redirect::permanent("/admin/") }))
            .nest("/share", share_public_router(share_public_state))
            .nest("/api/auth", auth_sql_router(auth_state))
            .nest("/api/admin", admin_router(admin_state))
            .nest("/api/collaboration/plaintext", collaboration_sql_router(collaboration_state))
            .nest("/api/mindmaps", mindmaps_sql_router(mindmaps_state))
            .nest("/api/notifications", notifications_router(notifications_state))
            .nest("/api/plaintext", plaintext_sql_router(plaintext_state))
            .nest("/api/public", public_router(public_state.clone()))
            .nest_service("/admin", admin_static_service)
            .fallback_service(app_static_service)
            .layer(from_fn(release_request_caches))
            .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024)) // 10 MiB
            .layer(TraceLayer::new_for_http())
            .layer(cors)
    };

    // ── Listen ────────────────────────────────────────────────────────────────
    let listener = tokio::net::TcpListener::bind(cfg.listen_addr()).await?;
    tracing::info!("Listening on http://{}", cfg.listen_addr());
    axum::serve(listener, app).await?;

    Ok(())
}
