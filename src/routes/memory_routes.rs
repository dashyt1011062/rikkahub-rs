use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AccountId;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::AppState;

const MEMORY_MAX_CHARS: usize = 20_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryQuery {
    assistant_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRequest {
    assistant_id: String,
    content: String,
}

pub async fn list(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Query(query): Query<MemoryQuery>,
) -> AppResult<Json<Value>> {
    let assistant_id = query.assistant_id.unwrap_or_default().trim().to_string();
    if assistant_id.is_empty() {
        return Err(AppError::bad_request("assistantId is required"));
    }
    let items = db::list_memories(state.config.db_path.clone(), account.0, assistant_id.clone()).await?;
    Ok(Json(json!({ "assistantId": assistant_id, "items": items })))
}

pub async fn create(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<MemoryRequest>,
) -> AppResult<Json<db::MemoryDto>> {
    let (assistant_id, content) = validate(request)?;
    Ok(Json(db::insert_memory(state.config.db_path.clone(), account.0, assistant_id, content).await?))
}

pub async fn update(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(request): Json<MemoryRequest>,
) -> AppResult<Json<db::MemoryDto>> {
    let (_, content) = validate(request)?;
    Ok(Json(db::update_memory(state.config.db_path.clone(), account.0, id, content).await?))
}

pub async fn delete(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    if !db::delete_memory(state.config.db_path.clone(), account.0, id).await? {
        return Err(AppError::not_found("Memory not found"));
    }
    Ok(Json(json!({ "status": "deleted" })))
}

fn validate(request: MemoryRequest) -> AppResult<(String, String)> {
    let assistant_id = request.assistant_id.trim().to_string();
    let content = request.content.trim().to_string();
    if assistant_id.is_empty() {
        return Err(AppError::bad_request("assistantId is required"));
    }
    if content.is_empty() {
        return Err(AppError::bad_request("Memory content cannot be empty"));
    }
    if content.chars().count() > MEMORY_MAX_CHARS {
        return Err(AppError::bad_request("Memory content is too long"));
    }
    Ok((assistant_id, content))
}
