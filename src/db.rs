use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension, OpenFlags};
use serde::Serialize;
use serde_json::Value;
use tokio::task;

use crate::config::DEFAULT_ASSISTANT_ID;
use crate::error::{AppError, AppResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListDto {
    pub id: String,
    pub assistant_id: String,
    pub title: String,
    pub is_pinned: bool,
    pub create_at: i64,
    pub update_at: i64,
    pub is_generating: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub next_offset: Option<i64>,
    pub has_more: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDto {
    pub id: String,
    pub assistant_id: String,
    pub title: String,
    pub messages: Vec<MessageNodeDto>,
    pub truncate_index: i64,
    pub chat_suggestions: Vec<String>,
    pub is_pinned: bool,
    pub create_at: i64,
    pub update_at: i64,
    pub is_generating: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageNodeDto {
    pub id: String,
    pub messages: Vec<MessageDto>,
    pub select_index: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDto {
    pub id: String,
    pub role: String,
    pub parts: Vec<Value>,
    pub annotations: Vec<Value>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSearchResultDto {
    pub node_id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub title: String,
    pub update_at: i64,
    pub snippet: String,
}

#[derive(Clone, Debug)]
pub struct ManagedFileRecord {
    pub id: i64,
    pub relative_path: String,
    pub storage_provider: String,
    pub remote_url: Option<String>,
    pub display_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub account_id: String,
}

pub async fn list_conversations(
    db_path: PathBuf,
    account_id: String,
    assistant_id: String,
    offset: i64,
    limit: i64,
    query: String,
) -> AppResult<PagedResult<ConversationListDto>> {
    task::spawn_blocking(move || {
        let conn = open_readonly(&db_path)?;
        let fetch_limit = limit + 1;
        let mut items = if query.trim().is_empty() {
            let mut stmt = conn.prepare(
                "SELECT id, assistant_id, title, is_pinned, create_at, update_at
                 FROM conversationentity
                 WHERE account_id = ?1 AND assistant_id = ?2
                 ORDER BY is_pinned DESC, update_at DESC
                 LIMIT ?3 OFFSET ?4",
            )?;
            let rows = stmt.query_map(params![account_id, assistant_id, fetch_limit, offset], row_to_list_item)?;
            read_conversation_list(rows)?
        } else {
            let keyword = format!("%{}%", query.trim());
            let mut stmt = conn.prepare(
                "SELECT id, assistant_id, title, is_pinned, create_at, update_at
                 FROM conversationentity
                 WHERE account_id = ?1 AND assistant_id = ?2 AND title LIKE ?3
                 ORDER BY is_pinned DESC, update_at DESC
                 LIMIT ?4 OFFSET ?5",
            )?;
            let rows = stmt.query_map(params![account_id, assistant_id, keyword, fetch_limit, offset], row_to_list_item)?;
            read_conversation_list(rows)?
        };

        let has_more = items.len() as i64 > limit;
        if has_more {
            items.truncate(limit as usize);
        }
        let next_offset = if has_more { Some(offset + limit) } else { None };
        Ok(PagedResult {
            items,
            next_offset,
            has_more,
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("conversation list task failed: {error}")))?
}

pub async fn get_conversation(db_path: PathBuf, account_id: String, conversation_id: String) -> AppResult<ConversationDto> {
    task::spawn_blocking(move || {
        let conn = open_readonly(&db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, assistant_id, title, truncate_index, suggestions, is_pinned, create_at, update_at
             FROM conversationentity
             WHERE id = ?1 AND account_id = ?2",
        )?;
        let row = stmt
            .query_row(params![conversation_id, account_id], |row| {
                let suggestions_raw: String = row.get(4)?;
                Ok(ConversationRow {
                    id: row.get(0)?,
                    assistant_id: row.get(1)?,
                    title: row.get(2)?,
                    truncate_index: row.get(3)?,
                    suggestions: parse_string_array(&suggestions_raw),
                    is_pinned: row.get::<_, i64>(5)? != 0,
                    create_at: row.get(6)?,
                    update_at: row.get(7)?,
                })
            })
            .optional()?
            .ok_or_else(|| AppError::not_found("Conversation not found"))?;

        let mut node_stmt = conn.prepare(
            "SELECT id, messages, select_index
             FROM message_node
             WHERE conversation_id = ?1
             ORDER BY node_index ASC",
        )?;
        let nodes = node_stmt
            .query_map(params![row.id.clone()], |node_row| {
                let messages_raw: String = node_row.get(1)?;
                Ok(MessageNodeDto {
                    id: node_row.get(0)?,
                    messages: parse_messages(&messages_raw),
                    select_index: node_row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ConversationDto {
            id: row.id,
            assistant_id: row.assistant_id,
            title: row.title,
            messages: nodes,
            truncate_index: row.truncate_index,
            chat_suggestions: row.suggestions,
            is_pinned: row.is_pinned,
            create_at: row.create_at,
            update_at: row.update_at,
            is_generating: false,
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("conversation detail task failed: {error}")))?
}

pub async fn search_messages(db_path: PathBuf, account_id: String, query: String, limit: i64) -> AppResult<Vec<MessageSearchResultDto>> {
    let rows = task::spawn_blocking(move || {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let conn = open_readonly(&db_path)?;
        let mut stmt = conn.prepare(
            "SELECT f.node_id, f.message_id, f.conversation_id, c.title, c.update_at,
                    snippet(message_fts, 0, '', '', '...', 12)
             FROM message_fts f
             JOIN conversationentity c ON c.id = f.conversation_id
             WHERE message_fts MATCH ?1 AND c.account_id = ?2
             ORDER BY c.update_at DESC
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![query.trim(), account_id, limit], |row| {
                Ok(MessageSearchResultDto {
                    node_id: row.get(0)?,
                    message_id: row.get(1)?,
                    conversation_id: row.get(2)?,
                    title: row.get(3)?,
                    update_at: row.get(4)?,
                    snippet: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>();

        match rows {
            Ok(items) => Ok(items),
            Err(_) => search_messages_like(&conn, &account_id, query.trim(), limit),
        }
    })
    .await
    .map_err(|error| AppError::internal(format!("message search task failed: {error}")))??;
    Ok(rows)
}

pub async fn get_file_by_id(db_path: PathBuf, account_id: String, id: i64) -> AppResult<ManagedFileRecord> {
    task::spawn_blocking(move || {
        let conn = open_readonly(&db_path)?;
        conn.query_row(
            "SELECT id, relative_path, storage_provider, remote_url, display_name, mime_type, size_bytes, account_id
             FROM managed_files
             WHERE id = ?1 AND account_id = ?2",
            params![id, account_id],
            row_to_file,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("File not found"))
    })
    .await
    .map_err(|error| AppError::internal(format!("file lookup task failed: {error}")))?
}

pub async fn get_file_by_path(db_path: PathBuf, account_id: String, relative_path: String) -> AppResult<ManagedFileRecord> {
    task::spawn_blocking(move || {
        let conn = open_readonly(&db_path)?;
        conn.query_row(
            "SELECT id, relative_path, storage_provider, remote_url, display_name, mime_type, size_bytes, account_id
             FROM managed_files
             WHERE relative_path = ?1 AND account_id = ?2",
            params![relative_path, account_id],
            row_to_file,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("File not found"))
    })
    .await
    .map_err(|error| AppError::internal(format!("file path lookup task failed: {error}")))?
}

fn open_readonly(path: &PathBuf) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
}

fn read_conversation_list<I>(rows: I) -> rusqlite::Result<Vec<ConversationListDto>>
where
    I: Iterator<Item = rusqlite::Result<ConversationListDto>>,
{
    rows.collect::<Result<Vec<_>, _>>()
}

fn row_to_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationListDto> {
    Ok(ConversationListDto {
        id: row.get(0)?,
        assistant_id: row.get::<_, Option<String>>(1)?.unwrap_or_else(|| DEFAULT_ASSISTANT_ID.to_string()),
        title: row.get(2)?,
        is_pinned: row.get::<_, i64>(3)? != 0,
        create_at: row.get(4)?,
        update_at: row.get(5)?,
        is_generating: false,
    })
}

fn row_to_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedFileRecord> {
    Ok(ManagedFileRecord {
        id: row.get(0)?,
        relative_path: row.get(1)?,
        storage_provider: row.get(2)?,
        remote_url: row.get(3)?,
        display_name: row.get(4)?,
        mime_type: row.get(5)?,
        size_bytes: row.get(6)?,
        account_id: row.get(7)?,
    })
}

fn parse_string_array(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn parse_messages(raw: &str) -> Vec<MessageDto> {
    let Ok(values) = serde_json::from_str::<Vec<Value>>(raw) else {
        return Vec::new();
    };
    values.into_iter().filter_map(parse_message).collect()
}

fn parse_message(value: Value) -> Option<MessageDto> {
    let object = value.as_object()?;
    Some(MessageDto {
        id: object.get("id")?.as_str()?.to_string(),
        role: object.get("role")?.as_str()?.to_string(),
        parts: object
            .get("parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        annotations: object
            .get("annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        created_at: object
            .get("createdAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        finished_at: object.get("finishedAt").and_then(Value::as_str).map(str::to_string),
        model_id: object.get("modelId").and_then(Value::as_str).map(str::to_string),
        usage: object.get("usage").cloned().filter(|value| !value.is_null()),
        translation: object.get("translation").and_then(Value::as_str).map(str::to_string),
    })
}

fn search_messages_like(
    conn: &Connection,
    account_id: &str,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<MessageSearchResultDto>> {
    let keyword = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT n.id, n.messages, n.conversation_id, c.title, c.update_at
         FROM message_node n
         JOIN conversationentity c ON c.id = n.conversation_id
         WHERE c.account_id = ?1 AND n.messages LIKE ?2
         ORDER BY c.update_at DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![account_id, keyword, limit], |row| {
        let node_id: String = row.get(0)?;
        let messages_raw: String = row.get(1)?;
        let snippet = make_snippet(&messages_raw, query);
        Ok(MessageSearchResultDto {
            node_id,
            message_id: String::new(),
            conversation_id: row.get(2)?,
            title: row.get(3)?,
            update_at: row.get(4)?,
            snippet,
        })
    })?
    .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn make_snippet(raw: &str, query: &str) -> String {
    let haystack = raw.replace(['\n', '\r', '\t'], " ");
    let start = haystack.find(query).unwrap_or(0).saturating_sub(40);
    let end = (start + 160).min(haystack.len());
    haystack.get(start..end).unwrap_or(&haystack).to_string()
}

struct ConversationRow {
    id: String,
    assistant_id: String,
    title: String,
    truncate_index: i64,
    suggestions: Vec<String>,
    is_pinned: bool,
    create_at: i64,
    update_at: i64,
}
