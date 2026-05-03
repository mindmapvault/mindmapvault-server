use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub sql_dsn: String,
    pub postgres_dsn: String,
    pub minio_endpoint: String,
    pub minio_public_endpoint: String,
    pub minio_access_key: String,
    pub minio_secret_key: String,
    pub minio_bucket: String,
    pub minio_region: String,
    pub minio_presign_expiry_secs: u64,
    pub jwt_secret: String,
    pub jwt_access_expiry_secs: u64,
    pub jwt_refresh_expiry_secs: u64,
    pub cors_allowed_origins: String,

    pub db_engine: String,
    pub enable_diagnostics_routes: bool,
    pub admin_api_token: String,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        // Load .env if present; ignore error if the file doesn't exist.
        let _ = dotenvy::dotenv();

        let cfg = config::Config::builder()
            .add_source(config::Environment::default().ignore_empty(true))
            .set_default("host", "127.0.0.1")?
            .set_default("port", 8080)?
            .set_default("sql_dsn", "")?
            .set_default("postgres_dsn", "")?
            .set_default("minio_public_endpoint", "")?
            .set_default("minio_region", "us-east-1")?
            .set_default("minio_presign_expiry_secs", 3600)?
            .set_default("jwt_access_expiry_secs", 900)?
            .set_default("jwt_refresh_expiry_secs", 2_592_000)?
            .set_default("cors_allowed_origins", "http://localhost:5173,http://tauri.localhost,https://tauri.localhost,http://localhost:8090,https://mindmapvault.com,https://www.mindmapvault.com,https://app.mindmapvault.com,https://mindmap.marazfamily.eu,https://admin.mindmapvault.com")?
            .set_default("db_engine", "sql")?
            .set_default("enable_diagnostics_routes", false)?
            .set_default("admin_api_token", "")?
            .build()?;

        Ok(cfg.try_deserialize()?)
    }

    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Parses CORS_ALLOWED_ORIGINS (comma-separated) into a Vec of header values.
    pub fn cors_origins(&self) -> Vec<String> {
        self.cors_allowed_origins
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}
