use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::json;

use crate::auth::AccountId;
use crate::db;
use crate::error::{AppError, AppResult};
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

pub async fn list_legacy(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<db::ConversationListDto>>> {
    let assistant_id = settings_store::current_assistant_id(&state.config, &account.0).await;
    let page = db::list_conversations(
        state.config.db_path.clone(),
        account.0,
        assistant_id,
        0,
        200,
        String::new(),
    )
    .await?;
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
    Ok(Json(
        db::list_conversations(
            state.config.db_path.clone(),
            account.0,
            assistant_id,
            offset,
            limit,
            query.query.unwrap_or_default(),
        )
        .await?,
    ))
}

pub async fn search(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Vec<db::MessageSearchResultDto>>> {
    let text = query.query.unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(Json(Vec::new()));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    Ok(Json(
        db::search_messages(state.config.db_path.clone(), account.0, text, limit).await?,
    ))
}

pub async fn detail(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<db::ConversationDto>> {
    Ok(Json(
        db::get_conversation(state.config.db_path.clone(), account.0, id).await?,
    ))
}

pub async fn detail_stream(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let db_path = state.config.db_path.clone();
    let account_id = account.0;
    let stream = stream! {
        match db::get_conversation(db_path, account_id, id).await {
            Ok(conversation) => {
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
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn list_stream(
    Extension(_account): Extension<AccountId>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream! {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            yield Ok(Event::default().event("keepalive").data("{}"));
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn not_implemented() -> AppResult<StatusCode> {
    Err(AppError::not_implemented(
        "This write operation is not implemented in the Rust preview yet",
    ))
}
