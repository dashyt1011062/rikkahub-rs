use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
pub struct SystemHealthDto {
    status: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfoDto {
    name: &'static str,
    version: String,
    host: String,
    port: u16,
    platform: String,
    data_dir: String,
    jwt_enabled: bool,
}

pub async fn health() -> Json<SystemHealthDto> {
    Json(SystemHealthDto { status: "ok" })
}

pub async fn info(State(state): State<AppState>) -> Json<SystemInfoDto> {
    Json(SystemInfoDto {
        name: "RikkaHub Rust Backend",
        version: state.config.version.clone(),
        host: state.config.host.clone(),
        port: state.config.port,
        platform: std::env::consts::OS.to_string(),
        data_dir: state.config.data_dir.to_string_lossy().to_string(),
        jwt_enabled: state.config.jwt_enabled,
    })
}
