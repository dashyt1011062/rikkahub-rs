use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;

use crate::auth;
use crate::AppState;

mod auth_routes;
mod conversation_routes;
mod file_routes;
mod settings_routes;
mod system_routes;

pub fn api_router(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/system/health", get(system_routes::health))
        .route("/system/info", get(system_routes::info))
        .route("/auth/token", post(auth_routes::token));

    let protected = Router::new()
        .route("/settings/stream", get(settings_routes::stream))
        .route("/settings/replace", post(settings_routes::replace))
        .route("/conversations", get(conversation_routes::list_legacy))
        .route("/conversations/paged", get(conversation_routes::paged))
        .route("/conversations/search", get(conversation_routes::search))
        .route("/conversations/stream", get(conversation_routes::list_stream))
        .route("/conversations/:id", get(conversation_routes::detail))
        .route("/conversations/:id/stream", get(conversation_routes::detail_stream))
        .route("/conversations/:id/messages", post(conversation_routes::not_implemented))
        .route("/conversations/:id/regenerate", post(conversation_routes::not_implemented))
        .route("/conversations/:id/regenerate-title", post(conversation_routes::not_implemented))
        .route("/conversations/:id/messages/:message_id/edit", post(conversation_routes::not_implemented))
        .route("/conversations/:id/messages/:message_id", delete(conversation_routes::not_implemented))
        .route("/files/id/:id", get(file_routes::by_id))
        .route("/files/path/*path", get(file_routes::by_path))
        .route("/files/upload", post(file_routes::upload_not_implemented))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_auth));

    public.merge(protected)
}

