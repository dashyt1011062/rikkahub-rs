use axum::extract::State;
use axum::Json;

use crate::auth::{create_web_jwt, secure_password_eq, WebAuthTokenRequest, WebAuthTokenResponse};
use crate::error::{AppError, AppResult};
use crate::AppState;

pub async fn token(
    State(state): State<AppState>,
    Json(request): Json<WebAuthTokenRequest>,
) -> AppResult<Json<WebAuthTokenResponse>> {
    if !state.config.jwt_enabled {
        return Err(AppError::bad_request("JWT auth is disabled"));
    }
    if state.config.effective_accounts().is_empty() {
        return Err(AppError::bad_request("Access password is not configured"));
    }

    let username = request.username.trim();
    let account = state
        .config
        .find_account(username)
        .ok_or_else(|| AppError::unauthorized("Invalid account or password"))?;
    if !secure_password_eq(&request.password, &account.password) {
        return Err(AppError::unauthorized("Invalid account or password"));
    }

    Ok(Json(create_web_jwt(&state.config, &account.username)?))
}

