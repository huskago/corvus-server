mod auth;
mod config;
mod error;
mod models;
mod routes;
mod state;
mod storage;

use std::{net::SocketAddr, path::PathBuf};

use config::Config;
use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path: PathBuf = std::env::var("CORVUS_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config.toml"));

    let mut cfg = Config::load_from(&config_path);

    if cfg.auth.password_hash.is_empty() {
        let password = std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| {
            let p = random_password();
            eprintln!("No ADMIN_PASSWORD set, generated: {p}");
            eprintln!("Change it via the admin panel after first run.");
            p
        });
        cfg.auth.password_hash = auth::hash_password(&password);
        tracing::info!("Password hashed and saved.");
    }

    if cfg.auth.jwt_secret.is_empty() {
        cfg.auth.jwt_secret = auth::generate_jwt_secret();
        tracing::info!("JWT secret generated.");
    }

    cfg.save_to(&config_path).expect("write config");

    let data_dir = PathBuf::from(&cfg.server.data_dir);
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    let port = cfg.server.port;
    let public_url = cfg.server.public_url.clone();
    let data_dir_display = data_dir.display().to_string();
    let state = AppState::new(cfg, config_path);
    let app = routes::build_router(state);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("Corvus Server listening on {addr}");
    tracing::info!("Public URL: {public_url}");
    tracing::info!("Data dir:   {data_dir_display}");
    tracing::info!("Admin panel: {public_url}/admin");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind listener");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("serve");
}

fn random_password() -> String {
    let bytes: [u8; 12] = rand::random();
    hex::encode(bytes)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm => {},
    }

    tracing::info!("Shutdown signal received, draining connections...");
}
