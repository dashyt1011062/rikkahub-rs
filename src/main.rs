mod auth;
mod config;
mod db;
mod error;
mod routes;
mod settings_store;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use reqwest::Client;
use tokio::sync::Semaphore;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::AppConfig;
use crate::error::AppResult;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub http: Client,
    pub file_proxy_permits: Arc<Semaphore>,
}

#[tokio::main]
async fn main() -> AppResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = Arc::new(AppConfig::from_env());
    let state = AppState {
        config: Arc::clone(&config),
        http: Client::builder()
            .user_agent("RikkaHub-Web-Rust/0.1")
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map_err(|error| error::AppError::internal(format!("failed to build http client: {error}")))?,
        file_proxy_permits: Arc::new(Semaphore::new(config.max_remote_file_proxies)),
    };

    let app = build_app(state.clone());
    let bind_addr: SocketAddr = config.bind_addr()?;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|error| error::AppError::internal(format!("failed to bind {bind_addr}: {error}")))?;

    tracing::info!(
        "rikkahub-rs listening on http://{} with data_dir={}",
        bind_addr,
        config.data_dir.display()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| error::AppError::internal(format!("server error: {error}")))?;

    Ok(())
}

fn build_app(state: AppState) -> Router {
    let web_root = state.config.web_ui_dir.clone();
    let index = web_root.join("index.html");
    let static_service = ServeDir::new(web_root).fallback(ServeFile::new(index));

    Router::new()
        .nest("/api", routes::api_router(state.clone()))
        .fallback_service(static_service)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        signal.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

