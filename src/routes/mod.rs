use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;

use crate::auth;
use crate::AppState;

mod auth_routes;
mod ai_icon_routes;
mod conversation_routes;
mod file_routes;
mod migration_routes;
mod settings_routes;
mod system_routes;

pub fn api_router(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/system/health", get(system_routes::health))
        .route("/system/info", get(system_routes::info))
        .route("/ai-icon", get(ai_icon_routes::icon))
        .route("/auth/token", post(auth_routes::token));

    let protected = Router::new()
        .route("/settings/stream", get(settings_routes::stream))
        .route("/settings/replace", post(settings_routes::replace))
        .route("/settings/assistant", post(settings_routes::assistant))
        .route("/settings/assistant/model", post(settings_routes::assistant_model))
        .route("/settings/assistant/thinking-budget", post(settings_routes::assistant_thinking_budget))
        .route("/settings/assistant/mcp", post(settings_routes::assistant_mcp))
        .route("/settings/assistant/injections", post(settings_routes::assistant_injections))
        .route("/settings/search/enabled", post(settings_routes::search_enabled))
        .route("/settings/search/service", post(settings_routes::search_service))
        .route("/settings/model/built-in-tool", post(settings_routes::model_builtin_tool))
        .route("/settings/favorite-models", post(settings_routes::favorite_models))
        .route("/settings/provider/models/fetch", post(settings_routes::provider_models_fetch))
        .route("/settings/provider/model/test", post(settings_routes::provider_model_test))
        .route("/settings/mcp/sync", post(settings_routes::mcp_sync))
        .route("/conversations", get(conversation_routes::list_legacy))
        .route("/conversations/paged", get(conversation_routes::paged))
        .route("/conversations/search", get(conversation_routes::search))
        .route("/conversations/stream", get(conversation_routes::list_stream))
        .route("/conversations/:id", get(conversation_routes::detail).delete(conversation_routes::delete_conversation))
        .route("/conversations/:id/stream", get(conversation_routes::detail_stream))
        .route("/conversations/:id/messages", post(conversation_routes::send_message))
        .route("/conversations/:id/messages/queue", post(conversation_routes::queue_message))
        .route("/conversations/:id/regenerate", post(conversation_routes::regenerate))
        .route("/conversations/:id/regenerate-title", post(conversation_routes::regenerate_title))
        .route("/conversations/:id/title", post(conversation_routes::update_title))
        .route("/conversations/:id/pin", post(conversation_routes::pin))
        .route("/conversations/:id/move", post(conversation_routes::move_conversation))
        .route("/conversations/:id/fork", post(conversation_routes::fork))
        .route("/conversations/:id/stop", post(conversation_routes::stop))
        .route("/conversations/:id/tool-approval", post(conversation_routes::tool_approval))
        .route("/conversations/:id/nodes/:node_id/select", post(conversation_routes::select_node))
        .route("/conversations/:id/messages/:message_id/edit", post(conversation_routes::edit_message))
        .route("/conversations/:id/messages/:message_id", delete(conversation_routes::delete_message))
        .route("/files/:id", delete(file_routes::delete_by_id))
        .route("/files/id/:id", get(file_routes::by_id))
        .route("/files/path/*path", get(file_routes::by_path))
        .route("/files/upload", post(file_routes::upload))
        .route("/migration/export", get(migration_routes::export_backup))
        .route("/migration/import", post(migration_routes::import_backup))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_auth));

    public.merge(protected)
}
