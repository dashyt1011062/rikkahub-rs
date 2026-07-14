use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AccountId;
use crate::db;
use crate::engine;
use crate::error::{AppError, AppResult};
use crate::events::AppEvent;
use crate::settings_store;
use crate::AppState;

#[derive(Deserialize)]
pub struct PageQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    query: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    query: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    parts: Vec<Value>,
    image_generation_mode: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMessageRequest {
    parts: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateRequest {
    message_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectNodeRequest {
    select_index: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleRequest {
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveRequest {
    assistant_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkRequest {
    message_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteMessageRequest {
    tag: String,
}

pub async fn list_legacy(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<db::ConversationListDto>>> {
    let assistant_id = settings_store::current_assistant_id(&state.config, &account.0).await;
    let mut page = db::list_conversations(
        state.config.db_path.clone(),
        account.0.clone(),
        assistant_id,
        0,
        200,
        String::new(),
    )
    .await?;
    for item in &mut page.items {
        item.is_generating = state.engine.is_generating(&account.0, &item.id).await;
    }
    Ok(Json(page.items))
}

pub async fn paged(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> AppResult<Json<db::PagedResult<db::ConversationListDto>>> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(20);
    if offset < 0 {
        return Err(AppError::bad_request("offset must be >= 0"));
    }
    if !(1..=100).contains(&limit) {
        return Err(AppError::bad_request("limit must be in 1..100"));
    }
    let assistant_id = settings_store::current_assistant_id(&state.config, &account.0).await;
    let mut page = db::list_conversations(
        state.config.db_path.clone(),
        account.0.clone(),
        assistant_id,
        offset,
        limit,
        query.query.unwrap_or_default(),
    )
    .await?;
    for item in &mut page.items {
        item.is_generating = state.engine.is_generating(&account.0, &item.id).await;
    }
    Ok(Json(page))
}

pub async fn search(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Vec<db::ConversationSearchResultDto>>> {
    let text = query.query.unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(Json(Vec::new()));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let assistant_id = settings_store::current_assistant_id(&state.config, &account.0).await;
    Ok(Json(
        db::search_conversations(
            state.config.db_path.clone(),
            account.0,
            assistant_id,
            text,
            limit,
        )
        .await?,
    ))
}

pub async fn list_favorites(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Vec<db::FavoriteMessageDto>>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let assistant_id = settings_store::current_assistant_id(&state.config, &account.0).await;
    Ok(Json(
        db::list_message_favorites(
            state.config.db_path.clone(),
            account.0,
            assistant_id,
            limit,
        )
        .await?,
    ))
}

pub async fn favorite_message(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path((id, message_id)): Path<(String, String)>,
    Json(request): Json<FavoriteMessageRequest>,
) -> AppResult<Json<Value>> {
    db::upsert_message_favorite(
        state.config.db_path.clone(),
        account.0,
        id,
        message_id,
        request.tag,
    )
    .await?;
    Ok(Json(json!({ "status": "saved" })))
}

pub async fn unfavorite_message(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path((id, message_id)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    let deleted = db::delete_message_favorite(
        state.config.db_path.clone(),
        account.0,
        id,
        message_id,
    )
    .await?;
    Ok(Json(json!({
        "status": if deleted { "deleted" } else { "not_found" }
    })))
}

pub async fn detail(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<db::ConversationDto>> {
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account.0.clone(), id).await?;
    conversation.is_generating = state.engine.is_generating(&account.0, &conversation.id).await;
    Ok(Json(conversation))
}

pub async fn detail_stream(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let db_path = state.config.db_path.clone();
    let account_id = account.0;
    let engine = state.engine.clone();
    let mut events = state.events.subscribe();
    let stream = stream! {
        match db::get_conversation(db_path.clone(), account_id.clone(), id.clone()).await {
            Ok(mut conversation) => {
                conversation.is_generating = engine.is_generating(&account_id, &id).await;
                let payload = json!({
                    "type": "snapshot",
                    "seq": 1,
                    "conversation": conversation,
                });
                yield Ok(Event::default().event("snapshot").data(payload.to_string()));
            }
            Err(error) => {
                let payload = json!({
                    "type": "error",
                    "message": error.message,
                });
                yield Ok(Event::default().event("error").data(payload.to_string()));
            }
        }
        let mut seq = 2i64;
        loop {
            match events.recv().await {
                Ok(AppEvent::ConversationNodeUpdated {
                    account_id: event_account,
                    conversation_id,
                    node_index,
                    node,
                    update_at,
                    is_generating,
                }) if event_account == account_id && conversation_id == id => {
                    let payload = json!({
                        "type": "node_update",
                        "seq": seq,
                        "conversationId": conversation_id,
                        "nodeId": node.id.clone(),
                        "nodeIndex": node_index,
                        "node": node,
                        "updateAt": update_at,
                        "isGenerating": is_generating,
                    });
                    seq += 1;
                    yield Ok(Event::default().event("node_update").data(payload.to_string()));
                }
                Ok(AppEvent::ConversationChanged { account_id: event_account, conversation_id }) if event_account == account_id && conversation_id == id => {
                    if let Ok(mut conversation) = db::get_conversation(db_path.clone(), account_id.clone(), id.clone()).await {
                        conversation.is_generating = engine.is_generating(&account_id, &id).await;
                        let payload = json!({
                            "type": "snapshot",
                            "seq": seq,
                            "conversation": conversation,
                        });
                        seq += 1;
                        yield Ok(Event::default().event("snapshot").data(payload.to_string()));
                    }
                }
                Ok(AppEvent::ConversationError { account_id: event_account, conversation_id, message }) if event_account == account_id && conversation_id == id => {
                    let payload = json!({ "type": "error", "message": message });
                    yield Ok(Event::default().event("error").data(payload.to_string()));
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn list_stream(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let account_id = account.0;
    let mut events = state.events.subscribe();
    let stream = stream! {
        loop {
            match events.recv().await {
                Ok(AppEvent::ConversationListInvalidated { account_id: event_account, assistant_id }) if event_account == account_id => {
                    let payload = json!({
                        "type": "invalidate",
                        "assistantId": assistant_id,
                        "timestamp": db::now_millis(),
                    });
                    yield Ok(Event::default().event("invalidate").data(payload.to_string()));
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn send_message(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    engine::send_message(state, account.0, id, request.parts, request.image_generation_mode).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "accepted" }))))
}

pub async fn queue_message(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let queued = engine::queue_message(state, account.0, id, request.parts, request.image_generation_mode).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": if queued { "queued" } else { "accepted" } }))))
}

pub async fn delete_pending_message(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path((id, pending_id)): Path<(String, i64)>,
) -> AppResult<Json<Value>> {
    let deleted = engine::delete_pending_message(&state, account.0, id, pending_id).await?;
    Ok(Json(json!({ "status": if deleted { "deleted" } else { "not_found" } })))
}

pub async fn edit_message(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path((id, message_id)): Path<(String, String)>,
    Json(request): Json<EditMessageRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    engine::edit_message(state, account.0, id, message_id, request.parts).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "accepted" }))))
}

pub async fn regenerate(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RegenerateRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    engine::regenerate_at_message(state, account.0, id, request.message_id).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "accepted" }))))
}

pub async fn select_node(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path((id, node_id)): Path<(String, String)>,
    Json(request): Json<SelectNodeRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    engine::select_node(&state, account.0, id, node_id, request.select_index).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "accepted" }))))
}

pub async fn delete_message(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path((id, message_id)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    engine::delete_message(&state, account.0, id, message_id).await?;
    Ok(Json(json!({ "status": "deleted" })))
}

pub async fn fork(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ForkRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let conversation_id = engine::fork_conversation(&state, account.0, id, request.message_id).await?;
    Ok((StatusCode::CREATED, Json(json!({ "conversationId": conversation_id }))))
}

pub async fn update_title(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<TitleRequest>,
) -> AppResult<Json<Value>> {
    engine::update_title(&state, account.0, id, request.title).await?;
    Ok(Json(json!({ "status": "updated" })))
}

pub async fn regenerate_title(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    engine::regenerate_title(&state, account.0, id).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "accepted" }))))
}

pub async fn pin(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    engine::toggle_pin(&state, account.0, id).await?;
    Ok(Json(json!({ "status": "updated" })))
}

pub async fn move_conversation(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<MoveRequest>,
) -> AppResult<Json<Value>> {
    engine::move_conversation(&state, account.0, id, request.assistant_id).await?;
    Ok(Json(json!({ "status": "updated" })))
}

pub async fn stop(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    state.engine.stop_generation(&state, &account.0, &id).await;
    Ok(Json(json!({ "status": "stopped" })))
}

pub async fn delete_conversation(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let deleted = engine::delete_conversation(&state, account.0, id).await?;
    if !deleted {
        return Err(AppError::not_found("Conversation not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn tool_approval() -> AppResult<(StatusCode, Json<Value>)> {
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "accepted" }))))
}
