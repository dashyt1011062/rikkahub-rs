use std::path::PathBuf;

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task;
use uuid::Uuid;

use crate::config::DEFAULT_ASSISTANT_ID;
use crate::error::{AppError, AppResult};

#[derive(Clone, Debug, Serialize)]
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub next_offset: Option<i64>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDto {
    pub id: String,
    pub assistant_id: String,
    pub title: String,
    pub messages: Vec<MessageNodeDto>,
    #[serde(default)]
    pub pending_messages: Vec<PendingMessageDto>,
    pub truncate_index: i64,
    pub chat_suggestions: Vec<String>,
    pub is_pinned: bool,
    pub create_at: i64,
    pub update_at: i64,
    pub is_generating: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageNodeDto {
    pub id: String,
    pub messages: Vec<MessageDto>,
    pub select_index: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMessageDto {
    pub id: i64,
    pub parts: Vec<Value>,
    pub image_generation_mode: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSearchResultDto {
    pub conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub title: String,
    pub update_at: i64,
    pub is_pinned: bool,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedFileDto {
    pub id: i64,
    pub url: String,
    pub file_name: String,
    pub mime: String,
    pub size: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UploadFilesResponseDto {
    pub files: Vec<UploadedFileDto>,
}

#[derive(Clone, Debug)]
pub struct PendingMessageRecord {
    pub parts: Vec<Value>,
    pub image_generation_mode: Option<String>,
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
            .query_row(params![&conversation_id, &account_id], |row| {
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
        let pending_messages = read_pending_messages(&conn, &account_id, &row.id)?;

        Ok(ConversationDto {
            id: row.id,
            assistant_id: row.assistant_id,
            title: row.title,
            messages: nodes,
            pending_messages,
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

pub async fn search_conversations(
    db_path: PathBuf,
    account_id: String,
    assistant_id: String,
    query: String,
    limit: i64,
) -> AppResult<Vec<ConversationSearchResultDto>> {
    let rows = task::spawn_blocking(move || -> AppResult<Vec<ConversationSearchResultDto>> {
        let query = query.trim().to_string();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = open_readonly(&db_path)?;
        let title_matches = search_conversation_titles(&conn, &account_id, &assistant_id, &query, limit)?;
        let content_limit = (limit.saturating_mul(5)).clamp(limit, 500);
        let content_matches = search_conversation_content(&conn, &account_id, &assistant_id, &query, content_limit)
            .or_else(|_| search_conversation_content_like(&conn, &account_id, &assistant_id, &query, content_limit))?;

        let mut merged = std::collections::HashMap::<String, ConversationSearchResultDto>::new();
        for item in title_matches.into_iter().chain(content_matches) {
            let key = item.conversation_id.clone();
            match merged.get_mut(&key) {
                Some(existing) => {
                    if existing.snippet.trim().is_empty() || existing.snippet == existing.title {
                        existing.snippet = item.snippet;
                    }
                    if existing.node_id.is_none() {
                        existing.node_id = item.node_id;
                    }
                    if existing.message_id.is_none() {
                        existing.message_id = item.message_id;
                    }
                    existing.update_at = existing.update_at.max(item.update_at);
                    existing.is_pinned = existing.is_pinned || item.is_pinned;
                }
                None => {
                    merged.insert(key, item);
                }
            }
        }

        let mut items = merged.into_values().collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .is_pinned
                .cmp(&left.is_pinned)
                .then_with(|| right.update_at.cmp(&left.update_at))
        });
        items.truncate(limit.max(0) as usize);
        Ok(items)
    })
    .await
    .map_err(|error| AppError::internal(format!("conversation search task failed: {error}")))??;
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

pub async fn ensure_conversation(
    db_path: PathBuf,
    account_id: String,
    conversation_id: String,
    assistant_id: String,
) -> AppResult<ConversationDto> {
    let existing = get_conversation(db_path.clone(), account_id.clone(), conversation_id.clone()).await;
    match existing {
        Ok(conversation) => Ok(conversation),
        Err(error) if error.status == axum::http::StatusCode::NOT_FOUND => {
            let now = now_millis();
            let conversation = ConversationDto {
                id: conversation_id,
                assistant_id,
                title: String::new(),
                messages: Vec::new(),
                pending_messages: Vec::new(),
                truncate_index: -1,
                chat_suggestions: Vec::new(),
                is_pinned: false,
                create_at: now,
                update_at: now,
                is_generating: false,
            };
            upsert_conversation(db_path, account_id, conversation.clone()).await?;
            Ok(conversation)
        }
        Err(error) => Err(error),
    }
}

pub async fn upsert_conversation(db_path: PathBuf, account_id: String, conversation: ConversationDto) -> AppResult<()> {
    task::spawn_blocking(move || {
        let mut conn = open_connection(&db_path)?;
        let tx = conn.transaction()?;
        ensure_conversation_writable(&tx, &conversation.id, &account_id)?;
        tx.execute(
            "INSERT INTO conversationentity (
                id, account_id, assistant_id, title, nodes, create_at, update_at, truncate_index, suggestions, is_pinned
             ) VALUES (?1, ?2, ?3, ?4, '[]', ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                assistant_id = excluded.assistant_id,
                title = excluded.title,
                update_at = excluded.update_at,
                truncate_index = excluded.truncate_index,
                suggestions = excluded.suggestions,
                is_pinned = excluded.is_pinned",
            params![
                conversation.id,
                account_id,
                conversation.assistant_id,
                conversation.title,
                conversation.create_at,
                conversation.update_at,
                conversation.truncate_index,
                serde_json::to_string(&conversation.chat_suggestions).unwrap_or_else(|_| "[]".to_string()),
                if conversation.is_pinned { 1 } else { 0 },
            ],
        )?;
        tx.execute("DELETE FROM message_node WHERE conversation_id = ?1", params![conversation.id])?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO message_node (id, conversation_id, node_index, messages, select_index)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (index, node) in conversation.messages.iter().enumerate() {
                stmt.execute(params![
                    node.id,
                    conversation.id,
                    index as i64,
                    serde_json::to_string(&node.messages)?,
                    node.select_index,
                ])?;
            }
        }

        replace_message_search_index(&tx, &conversation)?;
        tx.commit()?;
        Ok::<(), AppError>(())
    })
    .await
    .map_err(|error| AppError::internal(format!("conversation upsert task failed: {error}")))?
}

pub async fn upsert_message_node(
    db_path: PathBuf,
    account_id: String,
    conversation_id: String,
    node: MessageNodeDto,
    insert_index: i64,
    update_at: i64,
) -> AppResult<()> {
    task::spawn_blocking(move || {
        let mut conn = open_connection(&db_path)?;
        let tx = conn.transaction()?;
        ensure_conversation_writable(&tx, &conversation_id, &account_id)?;

        let title = tx
            .query_row(
                "SELECT title FROM conversationentity WHERE id = ?1 AND account_id = ?2",
                params![&conversation_id, &account_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Conversation not found"))?;
        tx.execute(
            "UPDATE conversationentity SET update_at = ?1 WHERE id = ?2 AND account_id = ?3",
            params![update_at, &conversation_id, &account_id],
        )?;

        let messages = serde_json::to_string(&node.messages)?;
        let updated = tx.execute(
            "UPDATE message_node
             SET messages = ?1, select_index = ?2
             WHERE id = ?3 AND conversation_id = ?4",
            params![&messages, node.select_index, &node.id, &conversation_id],
        )? > 0;

        if !updated {
            let node_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM message_node WHERE conversation_id = ?1",
                params![&conversation_id],
                |row| row.get(0),
            )?;
            let index = insert_index.clamp(0, node_count);
            tx.execute(
                "UPDATE message_node
                 SET node_index = node_index + 1
                 WHERE conversation_id = ?1 AND node_index >= ?2",
                params![&conversation_id, index],
            )?;
            tx.execute(
                "INSERT INTO message_node (id, conversation_id, node_index, messages, select_index)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![&node.id, &conversation_id, index, &messages, node.select_index],
            )?;
        }

        replace_node_search_index(&tx, &conversation_id, &title, update_at, &node)?;
        tx.commit()?;
        Ok::<(), AppError>(())
    })
    .await
    .map_err(|error| AppError::internal(format!("message node upsert task failed: {error}")))?
}

pub async fn enqueue_pending_message(
    db_path: PathBuf,
    account_id: String,
    conversation_id: String,
    parts: Vec<Value>,
    image_generation_mode: Option<String>,
) -> AppResult<i64> {
    task::spawn_blocking(move || {
        let mut conn = open_connection(&db_path)?;
        let tx = conn.transaction()?;
        ensure_pending_message_queue(&tx)?;
        ensure_conversation_writable(&tx, &conversation_id, &account_id)?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM conversationentity WHERE id = ?1 AND account_id = ?2",
                params![&conversation_id, &account_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::not_found("Conversation not found"));
        }
        tx.execute(
            "INSERT INTO pending_message_queue (account_id, conversation_id, parts, image_generation_mode, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &account_id,
                &conversation_id,
                serde_json::to_string(&parts)?,
                image_generation_mode,
                now_millis(),
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok::<i64, AppError>(id)
    })
    .await
    .map_err(|error| AppError::internal(format!("pending message enqueue task failed: {error}")))?
}

pub async fn pop_pending_message(
    db_path: PathBuf,
    account_id: String,
    conversation_id: String,
) -> AppResult<Option<PendingMessageRecord>> {
    task::spawn_blocking(move || {
        let mut conn = open_connection(&db_path)?;
        let tx = conn.transaction()?;
        ensure_pending_message_queue(&tx)?;
        let pending = tx
            .query_row(
                "SELECT id, parts, image_generation_mode
                 FROM pending_message_queue
                 WHERE account_id = ?1 AND conversation_id = ?2
                 ORDER BY id ASC
                 LIMIT 1",
                params![&account_id, &conversation_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, parts_raw, image_generation_mode)) = pending else {
            tx.commit()?;
            return Ok(None);
        };
        tx.execute("DELETE FROM pending_message_queue WHERE id = ?1", params![id])?;
        tx.commit()?;
        let parts = serde_json::from_str::<Vec<Value>>(&parts_raw)?;
        Ok(Some(PendingMessageRecord {
            parts,
            image_generation_mode,
        }))
    })
    .await
    .map_err(|error| AppError::internal(format!("pending message pop task failed: {error}")))?
}

pub async fn delete_pending_message(
    db_path: PathBuf,
    account_id: String,
    conversation_id: String,
    pending_id: i64,
) -> AppResult<bool> {
    task::spawn_blocking(move || {
        let mut conn = open_connection(&db_path)?;
        let tx = conn.transaction()?;
        ensure_pending_message_queue(&tx)?;
        let deleted = tx.execute(
            "DELETE FROM pending_message_queue
             WHERE id = ?1 AND account_id = ?2 AND conversation_id = ?3",
            params![pending_id, &account_id, &conversation_id],
        )? > 0;
        tx.commit()?;
        Ok::<bool, AppError>(deleted)
    })
    .await
    .map_err(|error| AppError::internal(format!("pending message delete task failed: {error}")))?
}

pub async fn delete_conversation(db_path: PathBuf, account_id: String, conversation_id: String) -> AppResult<bool> {
    task::spawn_blocking(move || {
        let mut conn = open_connection(&db_path)?;
        let tx = conn.transaction()?;
        ensure_pending_message_queue(&tx)?;
        let deleted = tx.execute(
            "DELETE FROM conversationentity WHERE id = ?1 AND account_id = ?2",
            params![&conversation_id, &account_id],
        )? > 0;
        if deleted {
            tx.execute("DELETE FROM message_fts WHERE conversation_id = ?1", params![&conversation_id])?;
            tx.execute("DELETE FROM pending_message_queue WHERE conversation_id = ?1", params![&conversation_id])?;
        }
        tx.commit()?;
        Ok::<bool, AppError>(deleted)
    })
    .await
    .map_err(|error| AppError::internal(format!("conversation delete task failed: {error}")))?
}

pub async fn insert_remote_file(
    db_path: PathBuf,
    account_id: String,
    display_name: String,
    mime_type: String,
    size_bytes: i64,
    storage_provider: String,
    remote_url: String,
    page_url: Option<String>,
    delete_url: Option<String>,
    thumbnail_url: Option<String>,
) -> AppResult<ManagedFileRecord> {
    task::spawn_blocking(move || {
        let conn = open_connection(&db_path)?;
        let safe_name = sanitize_display_name(&display_name);
        let relative_path = unique_upload_relative_path(&safe_name);
        let now = now_millis();
        conn.execute(
            "INSERT INTO managed_files (
                account_id, folder, relative_path, storage_provider, remote_url, page_url, delete_url, thumbnail_url,
                display_name, mime_type, size_bytes, created_at, updated_at
             ) VALUES (?1, 'upload', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                account_id,
                relative_path,
                storage_provider,
                remote_url,
                page_url,
                delete_url,
                thumbnail_url,
                safe_name,
                mime_type,
                size_bytes,
                now,
                now,
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(ManagedFileRecord {
            id,
            relative_path,
            storage_provider,
            remote_url: Some(remote_url),
            display_name: safe_name,
            mime_type,
            size_bytes,
            account_id,
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("file insert task failed: {error}")))?
}

pub async fn insert_local_file(
    db_path: PathBuf,
    account_id: String,
    display_name: String,
    mime_type: String,
    size_bytes: i64,
    storage_provider: String,
) -> AppResult<ManagedFileRecord> {
    task::spawn_blocking(move || {
        let conn = open_connection(&db_path)?;
        let safe_name = sanitize_display_name(&display_name);
        let relative_path = unique_upload_relative_path(&safe_name);
        let now = now_millis();
        conn.execute(
            "INSERT INTO managed_files (
                account_id, folder, relative_path, storage_provider, remote_url, page_url, delete_url, thumbnail_url,
                display_name, mime_type, size_bytes, created_at, updated_at
             ) VALUES (?1, 'upload', ?2, ?3, NULL, NULL, NULL, NULL, ?4, ?5, ?6, ?7, ?8)",
            params![
                account_id,
                relative_path,
                storage_provider,
                safe_name,
                mime_type,
                size_bytes,
                now,
                now,
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(ManagedFileRecord {
            id,
            relative_path,
            storage_provider,
            remote_url: None,
            display_name: safe_name,
            mime_type,
            size_bytes,
            account_id,
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("local file insert task failed: {error}")))?
}

pub async fn delete_file_record(db_path: PathBuf, account_id: String, id: i64) -> AppResult<bool> {
    task::spawn_blocking(move || {
        let conn = open_connection(&db_path)?;
        Ok::<bool, AppError>(
            conn.execute(
                "DELETE FROM managed_files WHERE id = ?1 AND account_id = ?2",
                params![id, account_id],
            )? > 0,
        )
    })
    .await
    .map_err(|error| AppError::internal(format!("file delete task failed: {error}")))?
}

pub async fn rebuild_message_search_index(db_path: PathBuf) -> AppResult<()> {
    task::spawn_blocking(move || {
        let mut conn = open_connection(&db_path)?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM message_fts", [])?;
        {
            let mut select = tx.prepare(
                "SELECT n.id, n.conversation_id, n.messages, c.title, c.update_at
                 FROM message_node n
                 JOIN conversationentity c ON c.id = n.conversation_id
                 ORDER BY n.node_index ASC",
            )?;
            let mut insert = tx.prepare(
                "INSERT INTO message_fts(text, node_id, message_id, conversation_id, title, update_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let rows = select.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?;
            for row in rows {
                let (node_id, conversation_id, messages_raw, title, update_at) = row?;
                for message in parse_messages(&messages_raw) {
                    let text = build_search_text(&title, &message);
                    if text.trim().is_empty() {
                        continue;
                    }
                    insert.execute(params![text, node_id, message.id, conversation_id, title, update_at])?;
                }
            }
        }
        tx.commit()?;
        Ok::<(), AppError>(())
    })
    .await
    .map_err(|error| AppError::internal(format!("search index rebuild task failed: {error}")))?
}

fn open_readonly(path: &PathBuf) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
}

fn open_connection(path: &PathBuf) -> rusqlite::Result<Connection> {
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

pub fn new_message(role: &str, parts: Vec<Value>, model_id: Option<String>, finished: bool) -> MessageDto {
    let now = now_iso();
    MessageDto {
        id: random_id(),
        role: role.to_string(),
        parts,
        annotations: Vec::new(),
        created_at: now.clone(),
        finished_at: if finished { Some(now) } else { None },
        model_id,
        usage: None,
        translation: None,
    }
}

pub fn new_node(message: MessageDto) -> MessageNodeDto {
    MessageNodeDto {
        id: random_id(),
        messages: vec![message],
        select_index: 0,
    }
}

pub fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub fn random_id() -> String {
    Uuid::new_v4().to_string()
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
        finished_at: object
            .get("finishedAt")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        model_id: object
            .get("modelId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        usage: object.get("usage").cloned().filter(|value| !value.is_null()),
        translation: object
            .get("translation")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn ensure_conversation_writable(conn: &Connection, id: &str, account_id: &str) -> AppResult<()> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT account_id FROM conversationentity WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        if existing != account_id {
            return Err(AppError::forbidden("Conversation belongs to another account"));
        }
    }
    Ok(())
}

fn ensure_pending_message_queue(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS pending_message_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            parts TEXT NOT NULL,
            image_generation_mode TEXT,
            created_at INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_pending_message_queue_conversation
         ON pending_message_queue(account_id, conversation_id, id)",
        [],
    )?;
    Ok(())
}

fn read_pending_messages(conn: &Connection, account_id: &str, conversation_id: &str) -> AppResult<Vec<PendingMessageDto>> {
    let table_exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'pending_message_queue'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !table_exists {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT id, parts, image_generation_mode, created_at
         FROM pending_message_queue
         WHERE account_id = ?1 AND conversation_id = ?2
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![account_id, conversation_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    let mut pending_messages = Vec::new();
    for row in rows {
        let (id, parts_raw, image_generation_mode, created_at) = row?;
        pending_messages.push(PendingMessageDto {
            id,
            parts: serde_json::from_str::<Vec<Value>>(&parts_raw)?,
            image_generation_mode,
            created_at,
        });
    }
    Ok(pending_messages)
}

fn replace_message_search_index(conn: &Connection, conversation: &ConversationDto) -> AppResult<()> {
    conn.execute(
        "DELETE FROM message_fts WHERE conversation_id = ?1",
        params![conversation.id],
    )?;
    let mut stmt = conn.prepare(
        "INSERT INTO message_fts(text, node_id, message_id, conversation_id, title, update_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for node in &conversation.messages {
        for message in &node.messages {
            let text = build_search_text(&conversation.title, message);
            if text.trim().is_empty() {
                continue;
            }
            stmt.execute(params![
                text,
                node.id,
                message.id,
                conversation.id,
                conversation.title,
                conversation.update_at,
            ])?;
        }
    }
    Ok(())
}

fn replace_node_search_index(
    conn: &Connection,
    conversation_id: &str,
    title: &str,
    update_at: i64,
    node: &MessageNodeDto,
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM message_fts WHERE conversation_id = ?1 AND node_id = ?2",
        params![conversation_id, node.id],
    )?;
    let mut stmt = conn.prepare(
        "INSERT INTO message_fts(text, node_id, message_id, conversation_id, title, update_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for message in &node.messages {
        let text = build_search_text(title, message);
        if text.trim().is_empty() {
            continue;
        }
        stmt.execute(params![text, node.id, message.id, conversation_id, title, update_at])?;
    }
    Ok(())
}

fn extract_message_search_text(message: &MessageDto) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| {
            let object = part.as_object()?;
            let kind = object.get("type")?.as_str()?.to_ascii_lowercase();
            if kind == "text" {
                object.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(10_000)
        .collect()
}

fn build_search_text(title: &str, message: &MessageDto) -> String {
    let message_text = extract_message_search_text(message);
    match (title.trim().is_empty(), message_text.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => title.trim().to_string(),
        (true, false) => message_text,
        (false, false) => format!("{}\n{}", title.trim(), message_text),
    }
}

fn sanitize_display_name(file_name: &str) -> String {
    let base = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("file")
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>();
    if base.trim().is_empty() {
        "file".to_string()
    } else {
        base
    }
}

fn unique_upload_relative_path(display_name: &str) -> String {
    let extension = display_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.trim())
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    format!("upload/{}{}", random_id(), extension)
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

fn search_conversation_titles(
    conn: &Connection,
    account_id: &str,
    assistant_id: &str,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<ConversationSearchResultDto>> {
    let keyword = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT id, title, update_at, is_pinned
         FROM conversationentity
         WHERE account_id = ?1 AND assistant_id = ?2 AND title LIKE ?3
         ORDER BY is_pinned DESC, update_at DESC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![account_id, assistant_id, keyword, limit], |row| {
        let title: String = row.get(1)?;
        Ok(ConversationSearchResultDto {
            conversation_id: row.get(0)?,
            node_id: None,
            message_id: None,
            snippet: title.clone(),
            title,
            update_at: row.get(2)?,
            is_pinned: row.get::<_, i64>(3)? != 0,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn search_conversation_content(
    conn: &Connection,
    account_id: &str,
    assistant_id: &str,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<ConversationSearchResultDto>> {
    let mut stmt = conn.prepare(
        "SELECT f.conversation_id, f.node_id, f.message_id, c.title, c.update_at, c.is_pinned,
                snippet(message_fts, 0, '', '', '...', 12)
         FROM message_fts f
         JOIN conversationentity c ON c.id = f.conversation_id
         WHERE message_fts MATCH ?1 AND c.account_id = ?2 AND c.assistant_id = ?3
         ORDER BY c.is_pinned DESC, c.update_at DESC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![query, account_id, assistant_id, limit], |row| {
        Ok(ConversationSearchResultDto {
            conversation_id: row.get(0)?,
            node_id: Some(row.get(1)?),
            message_id: Some(row.get(2)?),
            title: row.get(3)?,
            update_at: row.get(4)?,
            is_pinned: row.get::<_, i64>(5)? != 0,
            snippet: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn search_conversation_content_like(
    conn: &Connection,
    account_id: &str,
    assistant_id: &str,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<ConversationSearchResultDto>> {
    let keyword = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT n.id, n.messages, n.conversation_id, c.title, c.update_at, c.is_pinned
         FROM message_node n
         JOIN conversationentity c ON c.id = n.conversation_id
         WHERE c.account_id = ?1 AND c.assistant_id = ?2 AND n.messages LIKE ?3
         ORDER BY c.is_pinned DESC, c.update_at DESC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![account_id, assistant_id, keyword, limit], |row| {
        let messages_raw: String = row.get(1)?;
        Ok(ConversationSearchResultDto {
            node_id: Some(row.get(0)?),
            message_id: None,
            conversation_id: row.get(2)?,
            title: row.get(3)?,
            update_at: row.get(4)?,
            is_pinned: row.get::<_, i64>(5)? != 0,
            snippet: make_snippet(&messages_raw, query),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
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
