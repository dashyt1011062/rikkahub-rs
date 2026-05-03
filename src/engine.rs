use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::db::{self, ConversationDto, MessageNodeDto};
use crate::error::{AppError, AppResult};
use crate::events::AppEvent;
use crate::llm;
use crate::mcp;
use crate::settings_store;
use crate::AppState;

const MAX_TOOL_ROUNDS: usize = 4;

#[derive(Clone, Default)]
pub struct EngineState {
    generation_jobs: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl EngineState {
    pub async fn is_generating(&self, account_id: &str, conversation_id: &str) -> bool {
        self.generation_jobs
            .lock()
            .await
            .contains_key(&job_key(account_id, conversation_id))
    }

    async fn insert_job(&self, account_id: &str, conversation_id: &str, handle: JoinHandle<()>) {
        let mut jobs = self.generation_jobs.lock().await;
        if let Some(existing) = jobs.remove(&job_key(account_id, conversation_id)) {
            existing.abort();
        }
        jobs.insert(job_key(account_id, conversation_id), handle);
    }

    pub async fn stop_generation(&self, state: &AppState, account_id: &str, conversation_id: &str) {
        if let Some(handle) = self.generation_jobs.lock().await.remove(&job_key(account_id, conversation_id)) {
            handle.abort();
        }
        state.events.emit(AppEvent::ConversationChanged {
            account_id: account_id.to_string(),
            conversation_id: conversation_id.to_string(),
        });
    }

    async fn remove_job(&self, account_id: &str, conversation_id: &str) {
        self.generation_jobs
            .lock()
            .await
            .remove(&job_key(account_id, conversation_id));
    }
}

pub async fn send_message(
    state: AppState,
    account_id: String,
    conversation_id: String,
    mut parts: Vec<Value>,
    image_generation_mode: Option<String>,
) -> AppResult<()> {
    if parts.is_empty() {
        return Err(AppError::bad_request("parts must not be empty"));
    }
    apply_image_generation_mode(&mut parts, image_generation_mode.as_deref());

    let assistant_id = settings_store::current_assistant_id(&state.config, &account_id).await;
    let mut conversation = db::ensure_conversation(
        state.config.db_path.clone(),
        account_id.clone(),
        conversation_id.clone(),
        assistant_id,
    )
    .await?;
    let user = db::new_message("USER", parts, None, true);
    conversation.messages.push(db::new_node(user));
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(&state, &account_id, &conversation);
    start_generation(state, account_id, conversation_id).await;
    Ok(())
}

pub async fn edit_message(
    state: AppState,
    account_id: String,
    conversation_id: String,
    message_id: String,
    parts: Vec<Value>,
) -> AppResult<()> {
    if parts.is_empty() {
        return Err(AppError::bad_request("parts must not be empty"));
    }
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id.clone()).await?;
    let mut edited_role = None;
    for node in &mut conversation.messages {
        if let Some(original) = node.messages.iter().find(|message| message.id == message_id).cloned() {
            let mut edited = original.clone();
            edited.id = db::random_id();
            edited.parts = parts;
            edited.created_at = db::now_iso();
            edited.finished_at = Some(db::now_iso());
            node.messages.push(edited);
            node.select_index = node.messages.len() as i64 - 1;
            edited_role = Some(original.role);
            break;
        }
    }
    let Some(role) = edited_role else {
        return Err(AppError::bad_request("Message not found"));
    };
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(&state, &account_id, &conversation);
    if role.eq_ignore_ascii_case("USER") {
        start_generation(state, account_id, conversation_id).await;
    }
    Ok(())
}

pub async fn regenerate_at_message(
    state: AppState,
    account_id: String,
    conversation_id: String,
    message_id: String,
) -> AppResult<()> {
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id.clone()).await?;
    let Some(index) = conversation
        .messages
        .iter()
        .position(|node| node.messages.iter().any(|message| message.id == message_id))
    else {
        return Err(AppError::bad_request("Message not found"));
    };
    let role = conversation.messages[index]
        .messages
        .iter()
        .find(|message| message.id == message_id)
        .map(|message| message.role.clone())
        .unwrap_or_default();
    let keep = if role.eq_ignore_ascii_case("ASSISTANT") {
        index
    } else {
        index + 1
    };
    conversation.messages.truncate(keep);
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(&state, &account_id, &conversation);
    start_generation(state, account_id, conversation_id).await;
    Ok(())
}

pub async fn delete_message(
    state: &AppState,
    account_id: String,
    conversation_id: String,
    message_id: String,
) -> AppResult<()> {
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id).await?;
    let before = conversation.messages.len();
    conversation
        .messages
        .retain(|node| !node.messages.iter().any(|message| message.id == message_id));
    if conversation.messages.len() == before {
        return Err(AppError::bad_request("Message not found"));
    }
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(state, &account_id, &conversation);
    Ok(())
}

pub async fn select_node(
    state: &AppState,
    account_id: String,
    conversation_id: String,
    node_id: String,
    select_index: i64,
) -> AppResult<()> {
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id).await?;
    let Some(node) = conversation.messages.iter_mut().find(|node| node.id == node_id) else {
        return Err(AppError::bad_request("Message node not found"));
    };
    if select_index < 0 || select_index as usize >= node.messages.len() {
        return Err(AppError::bad_request("Invalid selectIndex"));
    }
    node.select_index = select_index;
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(state, &account_id, &conversation);
    Ok(())
}

pub async fn fork_conversation(
    state: &AppState,
    account_id: String,
    conversation_id: String,
    message_id: String,
) -> AppResult<String> {
    let conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id).await?;
    let Some(index) = conversation
        .messages
        .iter()
        .position(|node| node.messages.iter().any(|message| message.id == message_id))
    else {
        return Err(AppError::bad_request("Message not found"));
    };
    let now = db::now_millis();
    let fork_id = db::random_id();
    let mut fork_nodes = Vec::new();
    for node in conversation.messages.iter().take(index + 1) {
        fork_nodes.push(MessageNodeDto {
            id: db::random_id(),
            messages: node.messages.clone(),
            select_index: node.select_index,
        });
    }
    let fork = ConversationDto {
        id: fork_id.clone(),
        assistant_id: conversation.assistant_id,
        title: if conversation.title.trim().is_empty() {
            "Fork".to_string()
        } else {
            format!("{} (Fork)", conversation.title)
        },
        messages: fork_nodes,
        truncate_index: conversation.truncate_index,
        chat_suggestions: conversation.chat_suggestions,
        is_pinned: false,
        create_at: now,
        update_at: now,
        is_generating: false,
    };
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), fork.clone()).await?;
    emit_changed(state, &account_id, &fork);
    Ok(fork_id)
}

pub async fn update_title(
    state: &AppState,
    account_id: String,
    conversation_id: String,
    title: String,
) -> AppResult<()> {
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id).await?;
    conversation.title = title;
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(state, &account_id, &conversation);
    Ok(())
}

pub async fn regenerate_title(state: &AppState, account_id: String, conversation_id: String) -> AppResult<()> {
    let task_state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = generate_and_store_title(task_state.clone(), account_id.clone(), conversation_id.clone(), true).await {
            task_state.events.emit(AppEvent::ConversationError {
                account_id,
                conversation_id,
                message: error.message,
            });
        }
    });
    Ok(())
}

pub async fn toggle_pin(state: &AppState, account_id: String, conversation_id: String) -> AppResult<()> {
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id).await?;
    conversation.is_pinned = !conversation.is_pinned;
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(state, &account_id, &conversation);
    Ok(())
}

pub async fn move_conversation(
    state: &AppState,
    account_id: String,
    conversation_id: String,
    assistant_id: String,
) -> AppResult<()> {
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id).await?;
    conversation.assistant_id = assistant_id;
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(state, &account_id, &conversation);
    Ok(())
}

pub async fn delete_conversation(state: &AppState, account_id: String, conversation_id: String) -> AppResult<bool> {
    state.engine.stop_generation(state, &account_id, &conversation_id).await;
    let existing = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id.clone()).await.ok();
    let deleted = db::delete_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id.clone()).await?;
    if let Some(conversation) = existing {
        state.events.emit(AppEvent::ConversationListInvalidated {
            account_id: account_id.clone(),
            assistant_id: conversation.assistant_id,
        });
    }
    state.events.emit(AppEvent::ConversationChanged {
        account_id,
        conversation_id,
    });
    Ok(deleted)
}

pub async fn start_generation(state: AppState, account_id: String, conversation_id: String) {
    let task_state = state.clone();
    let task_account = account_id.clone();
    let task_conversation = conversation_id.clone();
    let handle = tokio::spawn(async move {
        task_state.events.emit(AppEvent::ConversationChanged {
            account_id: task_account.clone(),
            conversation_id: task_conversation.clone(),
        });
        if let Err(error) = run_generation(task_state.clone(), task_account.clone(), task_conversation.clone()).await {
            task_state.events.emit(AppEvent::ConversationError {
                account_id: task_account.clone(),
                conversation_id: task_conversation.clone(),
                message: error.message,
            });
        }
        task_state.engine.remove_job(&task_account, &task_conversation).await;
        task_state.events.emit(AppEvent::ConversationChanged {
            account_id: task_account,
            conversation_id: task_conversation,
        });
    });
    state.engine.insert_job(&account_id, &conversation_id, handle).await;
    state.events.emit(AppEvent::ConversationChanged {
        account_id,
        conversation_id,
    });
}

async fn run_generation(state: AppState, account_id: String, conversation_id: String) -> AppResult<()> {
    let settings = settings_store::read_settings(&state.config, &account_id).await?;
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id.clone()).await?;

    for _ in 0..MAX_TOOL_ROUNDS {
        let stream_node_id = Arc::new(Mutex::new(None::<String>));

        let result = llm::generate_reply_streaming(&state, &account_id, &settings, &conversation, |partial| {
            let state = state.clone();
            let account_id = account_id.clone();
            let conversation_id = conversation_id.clone();
            let stream_node_id = Arc::clone(&stream_node_id);
            async move {
                let mut guard = stream_node_id.lock().await;
                upsert_assistant_message(
                    &state,
                    &account_id,
                    &conversation_id,
                    &mut *guard,
                    partial,
                    false,
                )
                .await?;
                Ok(())
            }
        })
        .await?;

        let mut stream_node_id = stream_node_id.lock().await;
        conversation = upsert_assistant_message(
            &state,
            &account_id,
            &conversation_id,
            &mut *stream_node_id,
            result,
            true,
        )
        .await?;

        if execute_auto_tools(&state, &account_id, &settings, &mut conversation).await? {
            conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id.clone()).await?;
            continue;
        }
        break;
    }

    if conversation.title.trim().is_empty() {
        start_title_generation(state.clone(), account_id, conversation_id, false).await;
    }
    Ok(())
}

async fn upsert_assistant_message(
    state: &AppState,
    account_id: &str,
    conversation_id: &str,
    stream_node_id: &mut Option<String>,
    result: llm::AssistantGenerationResult,
    finished: bool,
) -> AppResult<ConversationDto> {
    let mut conversation = db::get_conversation(
        state.config.db_path.clone(),
        account_id.to_string(),
        conversation_id.to_string(),
    )
    .await?;
    let parts = if finished {
        llm::store_generated_images(state, account_id, result.parts).await?
    } else {
        result.parts
    };
    let now = db::now_millis();
    if stream_node_id.is_none() {
        let node_id = db::random_id();
        let mut message = db::new_message("ASSISTANT", parts, result.model_id, finished);
        if finished {
            message.usage = llm::normalize_usage(result.usage);
        }
        conversation.messages.push(MessageNodeDto {
            id: node_id.clone(),
            messages: vec![message],
            select_index: 0,
        });
        *stream_node_id = Some(node_id);
    } else if let Some(node_id) = stream_node_id.as_ref() {
        if let Some(node) = conversation.messages.iter_mut().find(|node| &node.id == node_id) {
            let mut message = node
                .messages
                .get(node.select_index.max(0) as usize)
                .cloned()
                .unwrap_or_else(|| db::new_message("ASSISTANT", Vec::new(), result.model_id.clone(), false));
            message.parts = parts;
            if finished {
                message.finished_at = Some(db::now_iso());
                message.usage = llm::normalize_usage(result.usage);
            }
            message.model_id = result.model_id.or(message.model_id);
            node.messages = vec![message];
            node.select_index = 0;
        }
    }
    conversation.update_at = now;
    db::upsert_conversation(state.config.db_path.clone(), account_id.to_string(), conversation.clone()).await?;
    emit_changed(state, account_id, &conversation);
    Ok(conversation)
}

async fn execute_auto_tools(
    state: &AppState,
    account_id: &str,
    settings: &Value,
    conversation: &mut ConversationDto,
) -> AppResult<bool> {
    let assistant = find_assistant(settings, &conversation.assistant_id).unwrap_or_else(|| json!({}));
    let Some((node_index, message_index)) = conversation
        .messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(node_index, node)| {
            let selected_index = node.select_index.max(0) as usize;
            let message_index = if node.messages.get(selected_index).is_some() { selected_index } else { 0 };
            let message = node.messages.get(message_index)?;
            message
                .role
                .eq_ignore_ascii_case("ASSISTANT")
                .then_some((node_index, message_index))
        })
    else {
        return Ok(false);
    };

    let Some(message) = conversation
        .messages
        .get(node_index)
        .and_then(|node| node.messages.get(message_index))
    else {
        return Ok(false);
    };

    let calls = message
        .parts
        .iter()
        .enumerate()
        .filter_map(|(part_index, part)| {
            if part.get("type").and_then(Value::as_str).map(|value| value.eq_ignore_ascii_case("tool")) != Some(true) {
                return None;
            }
            if !tool_output_text(part).trim().is_empty() {
                return None;
            }
            let approval = part
                .get("approvalState")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("auto");
            if approval != "auto" && approval != "approved" {
                return None;
            }
            let tool_name = part.get("toolName").and_then(Value::as_str)?.trim().to_string();
            if !tool_name.starts_with("mcp__") {
                return None;
            }
            let input = part.get("input").and_then(Value::as_str).unwrap_or("{}").to_string();
            Some((part_index, tool_name, input))
        })
        .collect::<Vec<_>>();

    if calls.is_empty() {
        return Ok(false);
    }

    let mut outputs = Vec::new();
    for (part_index, tool_name, input) in calls {
        let arguments = serde_json::from_str::<Value>(&input).unwrap_or_else(|_| json!({}));
        let output = match mcp::resolve_tool(settings, &assistant, &tool_name) {
            Some(resolved) => match mcp::call_tool(state, &resolved, arguments).await {
                Ok(value) => value,
                Err(error) => json!({ "error": error.message, "toolName": tool_name }),
            },
            None => json!({ "error": "mcp tool not found or server not configured", "toolName": tool_name }),
        };
        let text = serde_json::to_string(&output).unwrap_or_else(|_| output.to_string());
        outputs.push((part_index, json!([{ "type": "text", "text": text }])));
    }

    if let Some(message) = conversation
        .messages
        .get_mut(node_index)
        .and_then(|node| node.messages.get_mut(message_index))
    {
        for (part_index, output) in outputs {
            if let Some(part) = message.parts.get_mut(part_index).and_then(Value::as_object_mut) {
                part.insert("output".to_string(), output);
            }
        }
        message.finished_at.get_or_insert_with(db::now_iso);
    }
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.to_string(), conversation.clone()).await?;
    emit_changed(state, account_id, conversation);
    Ok(true)
}

async fn start_title_generation(state: AppState, account_id: String, conversation_id: String, force: bool) {
    tokio::spawn(async move {
        if let Err(error) = generate_and_store_title(state.clone(), account_id.clone(), conversation_id.clone(), force).await {
            state.events.emit(AppEvent::ConversationError {
                account_id,
                conversation_id,
                message: error.message,
            });
        }
    });
}

async fn generate_and_store_title(
    state: AppState,
    account_id: String,
    conversation_id: String,
    force: bool,
) -> AppResult<()> {
    let settings = settings_store::read_settings(&state.config, &account_id).await?;
    let mut conversation = db::get_conversation(state.config.db_path.clone(), account_id.clone(), conversation_id).await?;
    if !force && !conversation.title.trim().is_empty() {
        return Ok(());
    }
    let generated = llm::generate_title(&state, &account_id, &settings, &conversation)
        .await
        .ok()
        .flatten();
    let title = generated
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| derive_title(&conversation));
    if title.trim().is_empty() {
        return Ok(());
    }
    conversation.title = title;
    conversation.update_at = db::now_millis();
    db::upsert_conversation(state.config.db_path.clone(), account_id.clone(), conversation.clone()).await?;
    emit_changed(&state, &account_id, &conversation);
    Ok(())
}

fn find_assistant(settings: &Value, assistant_id: &str) -> Option<Value> {
    settings
        .get("assistants")
        .and_then(Value::as_array)?
        .iter()
        .find(|assistant| assistant.get("id").and_then(Value::as_str) == Some(assistant_id))
        .cloned()
}

fn tool_output_text(part: &Value) -> String {
    part.get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn emit_changed(state: &AppState, account_id: &str, conversation: &ConversationDto) {
    state.events.emit(AppEvent::ConversationChanged {
        account_id: account_id.to_string(),
        conversation_id: conversation.id.clone(),
    });
    state.events.emit(AppEvent::ConversationListInvalidated {
        account_id: account_id.to_string(),
        assistant_id: conversation.assistant_id.clone(),
    });
}

fn derive_title(conversation: &ConversationDto) -> String {
    selected_text(conversation)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(48)
        .collect::<String>()
        .trim()
        .to_string()
        .if_empty_else(|| format!("Conversation {}", db::now_iso().chars().take(19).collect::<String>()))
}

fn selected_text(conversation: &ConversationDto) -> String {
    conversation
        .messages
        .iter()
        .filter_map(|node| node.messages.get(node.select_index.max(0) as usize).or_else(|| node.messages.first()))
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                part.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .find(|text| !text.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

fn apply_image_generation_mode(parts: &mut [Value], mode: Option<&str>) {
    let normalized = match mode.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        Some("new_image") | Some("text_to_image") => "new_image",
        Some("continue_image") | Some("image_to_image") => "continue_image",
        _ => return,
    };
    let Some(first) = parts.first_mut().and_then(Value::as_object_mut) else {
        return;
    };
    let metadata = first.entry("metadata").or_insert_with(|| json!({}));
    if !metadata.is_object() {
        *metadata = json!({});
    }
    if let Some(object) = metadata.as_object_mut() {
        object.insert("imageGenerationMode".to_string(), Value::String(normalized.to_string()));
    }
}

fn job_key(account_id: &str, conversation_id: &str) -> String {
    format!("{account_id}:{conversation_id}")
}

trait IfEmpty {
    fn if_empty_else<F: FnOnce() -> String>(self, fallback: F) -> String;
}

impl IfEmpty for String {
    fn if_empty_else<F: FnOnce() -> String>(self, fallback: F) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}
