mod auth;
mod config;
mod db;
mod engine;
mod error;
mod events;
mod imgpile;
mod llm;
mod routes;
mod settings_store;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::Router;
use reqwest::Client;
use tokio::sync::Semaphore;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::AppConfig;
use crate::engine::EngineState;
use crate::error::AppResult;
use crate::events::EventHub;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub http: Client,
    pub file_proxy_permits: Arc<Semaphore>,
    pub events: EventHub,
    pub engine: EngineState,
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
        events: EventHub::new(),
        engine: EngineState::default(),
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
    let body_limit = state
        .config
        .upload_max_bytes
        .saturating_add(1024 * 1024)
        .min(usize::MAX as u64) as usize;

    Router::new()
        .nest("/api", routes::api_router(state.clone()))
        .fallback_service(static_service)
        .layer(DefaultBodyLimit::max(body_limit))
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
