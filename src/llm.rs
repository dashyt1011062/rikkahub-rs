use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bytes::Bytes;
use futures_util::StreamExt;
use regex::Regex;
use reqwest::multipart::{Form, Part};
use reqwest::RequestBuilder;
use serde_json::{json, Map, Value};

use crate::db::{self, ConversationDto, MessageDto};
use crate::document_parser;
use crate::error::{AppError, AppResult};
use crate::file_storage;
use crate::mcp::{self, AvailableTool};
use crate::AppState;

const DEFAULT_IMAGE_GENERATION_PROMPT: &str = "Generate an image.";
const DEFAULT_IMAGE_EDIT_PROMPT: &str = "Edit the image according to the prompt.";
const DEFAULT_TITLE_PROMPT: &str = r#"I will give you some dialogue content in the <content> block.
You need to summarize the conversation between user and assistant into a short title.
1. The title language should be consistent with the user's primary language
2. Do not use punctuation or other special symbols
3. Reply directly with the title
4. Summarize using {locale} language
5. The title should not exceed 10 characters

<content>
{content}
</content>"#;
const IMAGE_REFERENCE_MAX_BYTES: usize = 25 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AssistantGenerationResult {
    pub parts: Vec<Value>,
    pub model_id: Option<String>,
    pub usage: Option<Value>,
}

#[derive(Clone, Debug)]
struct Selection {
    provider: Value,
    provider_type: String,
    model: Value,
    assistant: Value,
    api_key: String,
}

#[derive(Clone, Debug)]
struct ImageUpload {
    file_name: String,
    mime_type: String,
    bytes: Bytes,
}

#[derive(Clone, Debug, Default)]
struct StreamingToolCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

#[derive(Clone, Debug)]
struct ProviderChatMessage {
    role: String,
    text: String,
    image_urls: Vec<String>,
}

pub async fn generate_reply_streaming<F, Fut>(
    state: &AppState,
    account_id: &str,
    settings: &Value,
    conversation: &ConversationDto,
    mut on_partial: F,
) -> AppResult<AssistantGenerationResult>
where
    F: FnMut(AssistantGenerationResult) -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let selection = select_model(settings, conversation)?;
    if is_image_generation_model(&selection.model) {
        return generate_image(state, account_id, &selection, conversation).await;
    }
    if selection.provider_type == "anthropic" {
        return generate_anthropic(state, account_id, &selection, conversation).await;
    }
    if selection.provider_type == "google" {
        return generate_google(state, account_id, &selection, conversation).await;
    }

    if use_openai_responses_api(&selection.provider) {
        return generate_openai_responses_streaming(state, account_id, settings, &selection, conversation, on_partial).await;
    }

    let messages = build_openai_messages(state, account_id, conversation, &selection.assistant).await?;
    let available_tools = mcp::build_available_tools(settings, &selection.assistant);
    let model_id = selection.model.get("id").and_then(Value::as_str).map(str::to_string);
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    add_generation_options(&mut payload, &selection.assistant);
    add_openai_chat_reasoning(&mut payload, &selection.assistant, &selection.model);
    add_openai_tools(&mut payload, &available_tools);
    add_custom_bodies(&mut payload, &selection.model);

    let url = join_url(
        selection
            .provider
            .get("baseUrl")
            .and_then(Value::as_str)
            .unwrap_or("https://api.openai.com/v1"),
        selection
            .provider
            .get("chatCompletionsPath")
            .and_then(Value::as_str)
            .unwrap_or("/chat/completions"),
    );

    let request = state
        .http
        .post(url)
        .bearer_auth(selection.api_key.clone());
    let response = apply_custom_headers(request, &selection.model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::bad_request(format!(
            "Provider request failed ({}): {}",
            status.as_u16(),
            body.chars().take(400).collect::<String>()
        )));
    }

    let mut content = String::new();
    let mut reasoning = String::new();
    let mut usage = None;
    let mut buffer = String::new();
    let mut tool_calls = std::collections::BTreeMap::<usize, StreamingToolCall>::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| AppError::bad_request(format!("Provider stream failed: {error}")))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim().to_string();
            buffer = buffer[index + 1..].to_string();
            if !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk_json) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(value) = chunk_json.get("usage").filter(|value| !value.is_null()) {
                usage = Some(value.clone());
            }
            let Some(delta) = chunk_json
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|choice| choice.get("delta"))
            else {
                continue;
            };

            let mut changed = false;
            let text_delta = extract_delta_text(delta);
            if !text_delta.is_empty() {
                content.push_str(&text_delta);
                changed = true;
            }
            let reasoning_delta = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !reasoning_delta.is_empty() {
                reasoning.push_str(reasoning_delta);
                changed = true;
            }
            collect_streaming_tool_calls(delta, &mut tool_calls);
            if changed {
                on_partial(AssistantGenerationResult {
                    parts: build_streaming_parts(&content, &reasoning),
                    model_id: model_id.clone(),
                    usage: None,
                })
                .await?;
            }
        }
    }

    if content.trim().is_empty() && reasoning.trim().is_empty() && tool_calls.is_empty() {
        return generate_reply_non_streaming(state, account_id, settings, &selection, conversation, true).await;
    }

    Ok(AssistantGenerationResult {
        parts: build_final_parts_with_tools(&content, &reasoning, &tool_calls, &available_tools),
        model_id,
        usage,
    })
}

async fn generate_openai_responses_streaming<F, Fut>(
    state: &AppState,
    account_id: &str,
    settings: &Value,
    selection: &Selection,
    conversation: &ConversationDto,
    mut on_partial: F,
) -> AppResult<AssistantGenerationResult>
where
    F: FnMut(AssistantGenerationResult) -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let input = build_openai_responses_input(state, account_id, conversation, &selection.assistant).await?;
    let available_tools = mcp::build_available_tools(settings, &selection.assistant);
    let model_id = selection.model.get("id").and_then(Value::as_str).map(str::to_string);
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "input": input,
        "stream": true,
    });
    add_responses_generation_options(&mut payload, &selection.assistant);
    add_openai_responses_reasoning(&mut payload, &selection.assistant, &selection.model);
    add_openai_responses_tools(&mut payload, &available_tools);
    add_custom_bodies(&mut payload, &selection.model);

    let request = state
        .http
        .post(join_url(
            selection.provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.openai.com/v1"),
            "/responses",
        ))
        .bearer_auth(selection.api_key.clone());
    let response = apply_custom_headers(request, &selection.model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::bad_request(format!(
            "Provider request failed ({}): {}",
            status.as_u16(),
            body.chars().take(400).collect::<String>()
        )));
    }

    let mut content = String::new();
    let mut reasoning = String::new();
    let mut usage = None;
    let mut buffer = String::new();
    let mut tool_calls = std::collections::BTreeMap::<usize, StreamingToolCall>::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| AppError::bad_request(format!("Provider stream failed: {error}")))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim().to_string();
            buffer = buffer[index + 1..].to_string();
            if !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk_json) = serde_json::from_str::<Value>(data) else {
                continue;
            };

            if let Some(value) = chunk_json.get("response").and_then(|response| response.get("usage")).filter(|value| !value.is_null()) {
                usage = Some(value.clone());
            } else if let Some(value) = chunk_json.get("usage").filter(|value| !value.is_null()) {
                usage = Some(value.clone());
            }

            let mut changed = false;
            let event_type = chunk_json.get("type").and_then(Value::as_str).unwrap_or_default();
            match event_type {
                "response.output_text.delta" | "response.text.delta" => {
                    if let Some(delta) = chunk_json.get("delta").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                        content.push_str(delta);
                        changed = true;
                    }
                }
                "response.output_text.done" | "response.text.done" => {
                    if content.trim().is_empty() {
                        if let Some(text) = chunk_json.get("text").and_then(Value::as_str).filter(|value| !value.trim().is_empty()) {
                            content.push_str(text);
                            changed = true;
                        }
                    }
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    if let Some(delta) = chunk_json.get("delta").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                        reasoning.push_str(delta);
                        changed = true;
                    }
                }
                "response.reasoning_summary_text.done" | "response.reasoning_text.done" => {
                    if reasoning.trim().is_empty() {
                        if let Some(text) = chunk_json.get("text").and_then(Value::as_str).filter(|value| !value.trim().is_empty()) {
                            reasoning.push_str(text);
                            changed = true;
                        }
                    }
                }
                "response.function_call_arguments.delta" => {
                    collect_responses_function_arguments_delta(&chunk_json, &mut tool_calls);
                }
                "response.output_item.added" | "response.output_item.done" => {
                    if let Some(item) = chunk_json.get("item") {
                        let output_index = chunk_json
                            .get("output_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(tool_calls.len() as u64) as usize;
                        collect_responses_output_item(
                            item,
                            output_index,
                            &mut content,
                            &mut reasoning,
                            &mut tool_calls,
                        );
                    }
                }
                "response.completed" => {
                    if let Some(response) = chunk_json.get("response") {
                        let before = (content.len(), reasoning.len(), tool_calls.len());
                        collect_responses_output(response, &mut content, &mut reasoning, &mut tool_calls);
                        changed |= before != (content.len(), reasoning.len(), tool_calls.len());
                    }
                }
                _ => {}
            }

            if changed {
                on_partial(AssistantGenerationResult {
                    parts: build_streaming_parts(&content, &reasoning),
                    model_id: model_id.clone(),
                    usage: None,
                })
                .await?;
            }
        }
    }

    if content.trim().is_empty() && reasoning.trim().is_empty() && tool_calls.is_empty() {
        return generate_openai_responses_non_streaming(state, account_id, settings, selection, conversation, true).await;
    }

    Ok(AssistantGenerationResult {
        parts: build_final_parts_with_tools(&content, &reasoning, &tool_calls, &available_tools),
        model_id,
        usage,
    })
}

pub async fn store_generated_images(state: &AppState, account_id: &str, parts: Vec<Value>) -> AppResult<Vec<Value>> {
    let mut stored = Vec::with_capacity(parts.len());
    let mut index = 0usize;
    for part in parts {
        if !is_generated_image_part(&part) {
            stored.push(part);
            continue;
        }
        let raw_url = part
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if part
            .get("metadata")
            .and_then(|metadata| metadata.get("fileId"))
            .is_some()
        {
            stored.push(part);
            continue;
        }
        if file_storage::uses_imgpile_storage(&state.config.file_storage)
            && (raw_url.starts_with("https://cdn.imgpile.com/") || raw_url.starts_with("http://cdn.imgpile.com/"))
        {
            let mime_type = detect_mime_from_url(&raw_url).to_string();
            let file_name = format!("generated-image-{}-{}.{}", db::now_millis(), index, extension_for_mime(&mime_type));
            let record = db::insert_remote_file(
                state.config.db_path.clone(),
                account_id.to_string(),
                file_name,
                mime_type,
                0,
                "imgpile".to_string(),
                raw_url.clone(),
                None,
                None,
                None,
            )
            .await?;
            stored.push(with_generated_file_metadata(part, record.id, &raw_url, "imgpile"));
            index += 1;
            continue;
        }
        let Some((bytes, mime_type)) = image_bytes_from_url(state, &raw_url).await? else {
            stored.push(part);
            continue;
        };
        let file_name = format!("generated-image-{}-{}.{}", db::now_millis(), index, extension_for_mime(&mime_type));
        let stored_file = file_storage::store_bytes(
            state,
            account_id.to_string(),
            file_name.clone(),
            mime_type.clone(),
            bytes,
        )
        .await?;
        stored.push(with_generated_file_metadata(
            part,
            stored_file.record.id,
            &stored_file.url,
            &stored_file.record.storage_provider,
        ));
        index += 1;
    }
    Ok(stored)
}

pub fn normalize_usage(usage: Option<Value>) -> Option<Value> {
    let usage = usage?;
    if usage.is_null() {
        return None;
    }

    let prompt = first_i64(
        &usage,
        &[
            &["promptTokens"],
            &["prompt_tokens"],
            &["input_tokens"],
            &["promptTokenCount"],
            &["usageMetadata", "promptTokenCount"],
        ],
    )
    .unwrap_or(0);
    let cached = first_i64(
        &usage,
        &[
            &["cachedTokens"],
            &["prompt_tokens_details", "cached_tokens"],
            &["cache_read_input_tokens"],
            &["cachedContentTokenCount"],
            &["usageMetadata", "cachedContentTokenCount"],
        ],
    )
    .unwrap_or(0);
    let explicit_total = first_i64(
        &usage,
        &[
            &["totalTokens"],
            &["total_tokens"],
            &["totalTokenCount"],
            &["usageMetadata", "totalTokenCount"],
        ],
    );
    let completion = first_i64(
        &usage,
        &[
            &["completionTokens"],
            &["completion_tokens"],
            &["output_tokens"],
            &["candidatesTokenCount"],
            &["usageMetadata", "candidatesTokenCount"],
        ],
    )
    .or_else(|| explicit_total.map(|total| total.saturating_sub(prompt).max(0)))
    .unwrap_or(0);
    let total = explicit_total.unwrap_or_else(|| prompt.saturating_add(completion));

    Some(json!({
        "promptTokens": prompt.max(0),
        "completionTokens": completion.max(0),
        "cachedTokens": cached.max(0),
        "totalTokens": total.max(0),
    }))
}

pub async fn generate_title(
    state: &AppState,
    account_id: &str,
    settings: &Value,
    conversation: &ConversationDto,
) -> AppResult<Option<String>> {
    let input = build_title_input(conversation);
    if input.trim().is_empty() {
        return Ok(None);
    }
    let prompt_template = settings
        .get("titlePrompt")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_TITLE_PROMPT);
    let prompt = prompt_template
        .replace("{locale}", "中文")
        .replace("{content}", &input);
    let preferred = settings
        .get("titleModelId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut refs = Vec::<Option<&str>>::new();
    match preferred {
        Some(value) if !value.eq_ignore_ascii_case("auto") => {
            refs.push(Some(value));
            refs.push(Some("auto"));
        }
        Some(value) => refs.push(Some(value)),
        None => refs.push(None),
    }

    for model_ref in refs {
        let Ok(selection) = select_model_ref(settings, &conversation.assistant_id, model_ref) else {
            continue;
        };
        let raw = generate_title_with_selection(state, account_id, &selection, &prompt)
            .await
            .unwrap_or_default();
        let normalized = normalize_generated_title(&raw);
        if !normalized.is_empty() {
            return Ok(Some(normalized));
        }
    }
    Ok(None)
}

async fn generate_title_with_selection(
    state: &AppState,
    _account_id: &str,
    selection: &Selection,
    prompt: &str,
) -> AppResult<String> {
    let messages = vec![ProviderChatMessage {
        role: "user".to_string(),
        text: prompt.to_string(),
        image_urls: Vec::new(),
    }];
    let result = match selection.provider_type.as_str() {
        "anthropic" => generate_anthropic_messages(state, selection, None, &messages).await?,
        "google" => generate_google_messages(state, selection, None, &messages).await?,
        _ => generate_openai_title(state, selection, prompt).await?,
    };
    Ok(result
        .parts
        .iter()
        .filter(|part| part_type(part) == "text")
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n"))
}

async fn generate_openai_title(state: &AppState, selection: &Selection, prompt: &str) -> AppResult<AssistantGenerationResult> {
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "messages": [{ "role": "user", "content": prompt }],
        "max_tokens": 80,
    });
    add_custom_bodies(&mut payload, &selection.model);
    let url = join_url(
        selection.provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.openai.com/v1"),
        selection.provider.get("chatCompletionsPath").and_then(Value::as_str).unwrap_or("/chat/completions"),
    );
    let response = apply_custom_headers(state.http.post(url).bearer_auth(selection.api_key.clone()), &selection.model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    let body = read_provider_json(response).await?;
    let message = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|choice| choice.get("message"))
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    Ok(AssistantGenerationResult {
        parts: extract_message_parts(&message),
        model_id: selection.model.get("id").and_then(Value::as_str).map(str::to_string),
        usage: body.get("usage").cloned(),
    })
}

async fn generate_reply_non_streaming(
    state: &AppState,
    account_id: &str,
    settings: &Value,
    selection: &Selection,
    conversation: &ConversationDto,
    include_tools: bool,
) -> AppResult<AssistantGenerationResult> {
    match selection.provider_type.as_str() {
        "anthropic" => generate_anthropic(state, account_id, selection, conversation).await,
        "google" => generate_google(state, account_id, selection, conversation).await,
        _ if use_openai_responses_api(&selection.provider) => {
            generate_openai_responses_non_streaming(state, account_id, settings, selection, conversation, include_tools).await
        }
        _ => generate_openai_non_streaming(state, account_id, settings, selection, conversation, include_tools).await,
    }
}

async fn generate_openai_responses_non_streaming(
    state: &AppState,
    account_id: &str,
    settings: &Value,
    selection: &Selection,
    conversation: &ConversationDto,
    include_tools: bool,
) -> AppResult<AssistantGenerationResult> {
    let input = build_openai_responses_input(state, account_id, conversation, &selection.assistant).await?;
    let available_tools = if include_tools {
        mcp::build_available_tools(settings, &selection.assistant)
    } else {
        Vec::new()
    };
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "input": input,
    });
    add_responses_generation_options(&mut payload, &selection.assistant);
    add_openai_responses_reasoning(&mut payload, &selection.assistant, &selection.model);
    add_openai_responses_tools(&mut payload, &available_tools);
    add_custom_bodies(&mut payload, &selection.model);

    let request = state
        .http
        .post(join_url(
            selection.provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.openai.com/v1"),
            "/responses",
        ))
        .bearer_auth(selection.api_key.clone());
    let response = apply_custom_headers(request, &selection.model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    let body = read_provider_json(response).await?;
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = std::collections::BTreeMap::<usize, StreamingToolCall>::new();
    collect_responses_output(&body, &mut content, &mut reasoning, &mut tool_calls);
    Ok(AssistantGenerationResult {
        parts: build_final_parts_with_tools(&content, &reasoning, &tool_calls, &available_tools),
        model_id: selection.model.get("id").and_then(Value::as_str).map(str::to_string),
        usage: body.get("usage").cloned(),
    })
}

async fn generate_openai_non_streaming(
    state: &AppState,
    account_id: &str,
    settings: &Value,
    selection: &Selection,
    conversation: &ConversationDto,
    include_tools: bool,
) -> AppResult<AssistantGenerationResult> {
    let messages = build_openai_messages(state, account_id, conversation, &selection.assistant).await?;
    let available_tools = if include_tools {
        mcp::build_available_tools(settings, &selection.assistant)
    } else {
        Vec::new()
    };
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "messages": messages,
    });
    add_generation_options(&mut payload, &selection.assistant);
    add_openai_chat_reasoning(&mut payload, &selection.assistant, &selection.model);
    add_openai_tools(&mut payload, &available_tools);
    add_custom_bodies(&mut payload, &selection.model);
    let url = join_url(
        selection.provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.openai.com/v1"),
        selection.provider.get("chatCompletionsPath").and_then(Value::as_str).unwrap_or("/chat/completions"),
    );
    let request = state
        .http
        .post(url)
        .bearer_auth(selection.api_key.clone());
    let response = apply_custom_headers(request, &selection.model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::bad_request(format!(
            "Provider request failed ({}): {}",
            status.as_u16(),
            body.chars().take(400).collect::<String>()
        )));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider returned invalid JSON: {error}")))?;
    let message = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|choice| choice.get("message"))
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let mut parts = extract_message_parts(&message);
    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !reasoning.is_empty() {
        parts.insert(0, reasoning_part(reasoning));
    }
    parts.extend(extract_openai_tool_parts(&message, &available_tools));
    if parts.is_empty() {
        parts.push(text_part("Model returned empty response"));
    }
    Ok(AssistantGenerationResult {
        parts,
        model_id: selection.model.get("id").and_then(Value::as_str).map(str::to_string),
        usage: body.get("usage").cloned(),
    })
}

async fn generate_image(
    state: &AppState,
    account_id: &str,
    selection: &Selection,
    conversation: &ConversationDto,
) -> AppResult<AssistantGenerationResult> {
    let (prompt, refs) = build_image_request(state, account_id, conversation).await?;
    let base = selection.provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.openai.com/v1");
    let model = selection.model.get("modelId").and_then(Value::as_str).unwrap_or("gpt-image-2");
    let mut payload = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "response_format": "b64_json",
    });
    add_custom_bodies(&mut payload, &selection.model);
    let response = if refs.is_empty() {
        let request = state
            .http
            .post(join_url(base, "/images/generations"))
            .bearer_auth(selection.api_key.clone());
        apply_custom_headers(request, &selection.model)?
            .json(&payload)
            .send()
            .await
            .map_err(|error| AppError::bad_request(format!("Image provider request failed: {error}")))?
    } else {
        let mut form = Form::new();
        if let Some(object) = payload.as_object() {
            for (key, value) in object {
                form = form.text(key.clone(), value_to_form_text(value));
            }
        }
        for item in refs {
            let part = Part::bytes(item.bytes.to_vec())
                .file_name(item.file_name)
                .mime_str(&item.mime_type)
                .map_err(|error| AppError::bad_request(format!("invalid image reference: {error}")))?;
            form = form.part("image", part);
        }
        let request = state
            .http
            .post(join_url(base, "/images/edits"))
            .bearer_auth(selection.api_key.clone());
        apply_custom_headers(request, &selection.model)?
            .multipart(form)
            .send()
            .await
            .map_err(|error| AppError::bad_request(format!("Image provider request failed: {error}")))?
    };
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::bad_request(format!(
            "Image provider request failed ({}): {}",
            status.as_u16(),
            body.chars().take(400).collect::<String>()
        )));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| AppError::bad_request(format!("Image provider returned invalid JSON: {error}")))?;
    let parts = extract_image_response_parts(&body);
    if parts.is_empty() {
        return Err(AppError::bad_request("Image provider returned no images"));
    }
    Ok(AssistantGenerationResult {
        parts,
        model_id: selection.model.get("id").and_then(Value::as_str).map(str::to_string),
        usage: body.get("usage").cloned(),
    })
}

async fn generate_anthropic(
    state: &AppState,
    account_id: &str,
    selection: &Selection,
    conversation: &ConversationDto,
) -> AppResult<AssistantGenerationResult> {
    let (system_prompt, messages) = build_provider_messages(state, account_id, conversation, &selection.assistant).await?;
    generate_anthropic_messages(state, selection, system_prompt.as_deref(), &messages).await
}

async fn generate_google(
    state: &AppState,
    account_id: &str,
    selection: &Selection,
    conversation: &ConversationDto,
) -> AppResult<AssistantGenerationResult> {
    let (system_prompt, messages) = build_provider_messages(state, account_id, conversation, &selection.assistant).await?;
    generate_google_messages(state, selection, system_prompt.as_deref(), &messages).await
}

async fn generate_anthropic_messages(
    state: &AppState,
    selection: &Selection,
    system_prompt: Option<&str>,
    messages: &[ProviderChatMessage],
) -> AppResult<AssistantGenerationResult> {
    let mut payload_messages = Vec::new();
    for message in messages.iter().filter(|message| message.role != "system") {
        payload_messages.push(json!({
            "role": if message.role == "assistant" { "assistant" } else { "user" },
            "content": anthropic_content_for_message(state, message).await?,
        }));
    }
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("claude"),
        "messages": payload_messages,
        "max_tokens": selection.assistant.get("maxTokens").and_then(Value::as_i64).unwrap_or(1024).max(1),
    });
    if let Some(system_prompt) = system_prompt.filter(|value| !value.trim().is_empty()) {
        payload["system"] = Value::String(system_prompt.to_string());
    }
    add_generation_options(&mut payload, &selection.assistant);
    add_anthropic_thinking(&mut payload, &selection.assistant, &selection.model);
    add_custom_bodies(&mut payload, &selection.model);

    let request = state
        .http
        .post(join_url(
            selection.provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.anthropic.com/v1"),
            "/messages",
        ))
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", selection.api_key.clone());
    let response = apply_custom_headers(request, &selection.model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    let body = read_provider_json(response).await?;
    let (content, reasoning) = extract_anthropic_text_and_reasoning(&body);
    Ok(AssistantGenerationResult {
        parts: build_final_parts(&content, &reasoning),
        model_id: selection.model.get("id").and_then(Value::as_str).map(str::to_string),
        usage: body.get("usage").cloned(),
    })
}

async fn generate_google_messages(
    state: &AppState,
    selection: &Selection,
    system_prompt: Option<&str>,
    messages: &[ProviderChatMessage],
) -> AppResult<AssistantGenerationResult> {
    let mut contents = Vec::new();
    for message in messages.iter().filter(|message| message.role != "system") {
        contents.push(json!({
            "role": if message.role == "assistant" { "model" } else { "user" },
            "parts": google_parts_for_message(state, message).await?,
        }));
    }
    let mut payload = json!({ "contents": contents });
    if let Some(system_prompt) = system_prompt.filter(|value| !value.trim().is_empty()) {
        payload["systemInstruction"] = json!({ "parts": [{ "text": system_prompt }] });
    }
    let mut generation_config = Map::new();
    if let Some(value) = selection.assistant.get("temperature").and_then(Value::as_f64) {
        generation_config.insert("temperature".to_string(), json!(value));
    }
    if let Some(value) = selection.assistant.get("topP").and_then(Value::as_f64) {
        generation_config.insert("topP".to_string(), json!(value));
    }
    if let Some(value) = selection.assistant.get("maxTokens").and_then(Value::as_i64) {
        generation_config.insert("maxOutputTokens".to_string(), json!(value));
    }
    if !generation_config.is_empty() {
        payload["generationConfig"] = Value::Object(generation_config);
    }
    add_google_thinking_config(&mut payload, &selection.assistant, &selection.model);
    add_custom_bodies(&mut payload, &selection.model);

    let model = selection.model.get("modelId").and_then(Value::as_str).unwrap_or("gemini-2.0-flash");
    let encoded_model = encode_google_model_id(model);
    let base = selection.provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://generativelanguage.googleapis.com/v1beta");
    let url = format!("{}/models/{}:generateContent?key={}", base.trim_end_matches('/'), encoded_model, selection.api_key);
    let response = apply_custom_headers(state.http.post(url), &selection.model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    let body = read_provider_json(response).await?;
    let (content, reasoning) = extract_google_text_and_reasoning(&body);
    Ok(AssistantGenerationResult {
        parts: build_final_parts(&content, &reasoning),
        model_id: selection.model.get("id").and_then(Value::as_str).map(str::to_string),
        usage: body.get("usageMetadata").cloned(),
    })
}

fn select_model(settings: &Value, conversation: &ConversationDto) -> AppResult<Selection> {
    select_model_ref(settings, &conversation.assistant_id, None)
}

fn select_model_ref(settings: &Value, assistant_id: &str, model_ref_override: Option<&str>) -> AppResult<Selection> {
    let assistant = settings
        .get("assistants")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(assistant_id))
        })
        .cloned()
        .unwrap_or_else(|| json!({}));
    let model_id = match model_ref_override.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if !value.eq_ignore_ascii_case("auto") => value,
        _ => assistant
            .get("chatModelId")
            .or_else(|| settings.get("chatModelId"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty() && *value != "auto")
            .ok_or_else(|| AppError::bad_request("No chat model selected"))?,
    };

    let providers = settings
        .get("providers")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::bad_request("No providers configured"))?;
    for provider in providers {
        if provider.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let Some(models) = provider.get("models").and_then(Value::as_array) else {
            continue;
        };
        if let Some(model) = models.iter().find(|item| item.get("id").and_then(Value::as_str) == Some(model_id)) {
            let api_key = provider
                .get("apiKey")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            return Ok(Selection {
                provider: provider.clone(),
                provider_type: normalize_provider_type(provider),
                model: model.clone(),
                assistant,
                api_key,
            });
        }
    }
    Err(AppError::bad_request("Selected model not found"))
}

async fn build_openai_messages(
    state: &AppState,
    account_id: &str,
    conversation: &ConversationDto,
    assistant: &Value,
) -> AppResult<Vec<Value>> {
    let mut messages = Vec::new();
    if let Some(system_prompt) = assistant
        .get("systemPrompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({ "role": "system", "content": system_prompt }));
    }

    let selected = selected_messages(conversation);
    let window_size = assistant.get("contextMessageSize").and_then(Value::as_i64).unwrap_or(0).max(0) as usize;
    let start = if window_size > 0 && selected.len() > window_size {
        selected.len() - window_size
    } else {
        0
    };

    for message in selected.into_iter().skip(start) {
        let role = match message.role.trim().to_ascii_uppercase().as_str() {
            "ASSISTANT" => "assistant",
            "SYSTEM" => "system",
            _ => "user",
        };
        if role == "assistant" {
            let tool_calls = openai_tool_calls_for_message(message);
            let content = parts_to_plain_text(&message.parts);
            if content.trim().is_empty() && tool_calls.is_empty() {
                continue;
            }
            let mut item = json!({
                "role": "assistant",
                "content": content,
            });
            if !tool_calls.is_empty() {
                item["tool_calls"] = Value::Array(tool_calls);
            }
            messages.push(item);
            for part in message.parts.iter().filter(|part| part_type(part) == "tool") {
                let Some(call_id) = part.get("toolCallId").and_then(Value::as_str).filter(|value| !value.is_empty()) else {
                    continue;
                };
                let output = tool_output_text(part);
                if output.trim().is_empty() {
                    continue;
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output,
                }));
            }
            continue;
        }

        let content = openai_user_content_for_message(state, account_id, message, role).await?;
        if content.as_str().map(|value| value.trim().is_empty()).unwrap_or(false) {
            continue;
        }
        messages.push(json!({
            "role": role,
            "content": content,
        }));
    }
    Ok(messages)
}

async fn openai_user_content_for_message(state: &AppState, account_id: &str, message: &MessageDto, role: &str) -> AppResult<Value> {
    if role == "system" {
        return Ok(Value::String(prompt_text_with_documents(state, account_id, &message.parts).await?));
    }
    let mut blocks = Vec::new();
    for part in &message.parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
        match kind.as_str() {
            "text" => {
                if let Some(text) = part.get("text").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
            }
            "image" => {
                if let Some(url) = resolve_part_image_reference(state, account_id, part).await? {
                    blocks.push(json!({ "type": "image_url", "image_url": { "url": url } }));
                }
            }
            "document" => {
                let text = document_context_for_part(state, account_id, part).await?;
                blocks.push(json!({ "type": "text", "text": text }));
            }
            _ => {}
        }
    }
    if blocks.len() == 1 && blocks[0].get("type").and_then(Value::as_str) == Some("text") {
        return Ok(blocks[0].get("text").cloned().unwrap_or(Value::String(String::new())));
    }
    Ok(Value::Array(blocks))
}

async fn resolve_part_image_url(state: &AppState, account_id: &str, part: &Value) -> AppResult<Option<String>> {
    if let Some(file_id) = part
        .get("metadata")
        .and_then(|metadata| metadata.get("fileId"))
        .and_then(|value| value.as_i64().or_else(|| value.as_str().and_then(|text| text.parse().ok())))
    {
        if let Ok(file) = db::get_file_by_id(
            state.config.db_path.clone(),
            account_id.to_string(),
            file_id,
        )
        .await
        {
            if let Some(remote) = file
                .remote_url
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
            {
                return Ok(Some(remote));
            }
            if let Some((bytes, mime)) = file_storage::read_local_file_bytes(&state.config.data_dir, &file).await? {
                return Ok(Some(format!("data:{};base64,{}", mime, BASE64.encode(&bytes))));
            }
        }
    }
    Ok(part
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string))
}

async fn resolve_part_image_reference(state: &AppState, account_id: &str, part: &Value) -> AppResult<Option<String>> {
    let Some(url) = resolve_part_image_url(state, account_id, part).await? else {
        return Ok(None);
    };
    match image_bytes_from_url(state, &url).await {
        Ok(Some((bytes, mime))) if bytes.len() <= IMAGE_REFERENCE_MAX_BYTES => {
            Ok(Some(format!("data:{};base64,{}", mime, BASE64.encode(&bytes))))
        }
        _ => Ok(Some(url)),
    }
}

async fn build_image_request(state: &AppState, account_id: &str, conversation: &ConversationDto) -> AppResult<(String, Vec<ImageUpload>)> {
    let selected = selected_messages(conversation);
    let user = selected
        .iter()
        .rev()
        .find(|message| message.role.eq_ignore_ascii_case("USER"))
        .ok_or_else(|| AppError::bad_request("No user message found"))?;
    let prompt = prompt_text(&user.parts);
    let mode = requested_image_mode(&user.parts);
    let current_images = image_urls_from_parts(state, account_id, &user.parts).await?;
    let mut latest_assistant_image = None;
    'messages: for message in selected.iter().rev().skip(1) {
        if !message.role.eq_ignore_ascii_case("ASSISTANT") {
            continue;
        }
        for part in message.parts.iter().rev() {
            if part
                .get("type")
                .and_then(Value::as_str)
                .map(|value| value.eq_ignore_ascii_case("image"))
                != Some(true)
            {
                continue;
            }
            latest_assistant_image = resolve_part_image_url(state, account_id, part).await?;
            if latest_assistant_image.is_some() {
                break 'messages;
            }
        }
    }
    let refs = if mode == "continue_image" {
        latest_assistant_image.into_iter().chain(current_images.into_iter()).collect::<Vec<_>>()
    } else {
        current_images
    };
    let mut uploads = Vec::new();
    for (index, url) in refs.into_iter().enumerate() {
        if let Some((bytes, mime)) = image_bytes_from_url(state, &url).await? {
            if bytes.len() <= IMAGE_REFERENCE_MAX_BYTES {
                uploads.push(ImageUpload {
                    file_name: format!("reference-{}.{}", index, extension_for_mime(&mime)),
                    mime_type: mime,
                    bytes,
                });
            }
        }
    }
    let prompt = if prompt.trim().is_empty() {
        if uploads.is_empty() {
            DEFAULT_IMAGE_GENERATION_PROMPT.to_string()
        } else {
            DEFAULT_IMAGE_EDIT_PROMPT.to_string()
        }
    } else {
        prompt
    };
    Ok((prompt, uploads))
}

async fn image_urls_from_parts(state: &AppState, account_id: &str, parts: &[Value]) -> AppResult<Vec<String>> {
    let mut urls = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str).map(|v| v.eq_ignore_ascii_case("image")) == Some(true) {
            if let Some(url) = resolve_part_image_url(state, account_id, part).await? {
                urls.push(url);
            }
        }
    }
    Ok(urls)
}

async fn build_provider_messages(
    state: &AppState,
    account_id: &str,
    conversation: &ConversationDto,
    assistant: &Value,
) -> AppResult<(Option<String>, Vec<ProviderChatMessage>)> {
    let system_prompt = assistant
        .get("systemPrompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let selected = selected_messages(conversation);
    let window_size = assistant.get("contextMessageSize").and_then(Value::as_i64).unwrap_or(0).max(0) as usize;
    let start = if window_size > 0 && selected.len() > window_size {
        selected.len() - window_size
    } else {
        0
    };
    let mut out = Vec::new();
    for message in selected.into_iter().skip(start) {
        let role = match message.role.trim().to_ascii_uppercase().as_str() {
            "ASSISTANT" => "assistant",
            "SYSTEM" => "system",
            _ => "user",
        }
        .to_string();
        if role == "system" {
            let text = prompt_text_with_documents(state, account_id, &message.parts).await?;
            if !text.trim().is_empty() {
                out.push(ProviderChatMessage {
                    role,
                    text,
                    image_urls: Vec::new(),
                });
            }
            continue;
        }
        let text = if role == "assistant" {
            parts_to_plain_text(&message.parts)
        } else {
            prompt_text_with_documents(state, account_id, &message.parts).await?
        };
        let image_urls = if role == "user" {
            image_urls_from_parts(state, account_id, &message.parts).await?
        } else {
            Vec::new()
        };
        if text.trim().is_empty() && image_urls.is_empty() {
            continue;
        }
        out.push(ProviderChatMessage {
            role,
            text,
            image_urls,
        });
    }
    Ok((system_prompt, out))
}

async fn anthropic_content_for_message(state: &AppState, message: &ProviderChatMessage) -> AppResult<Value> {
    if message.role == "assistant" || message.image_urls.is_empty() {
        return Ok(Value::String(message.text.clone()));
    }
    let mut blocks = Vec::new();
    if !message.text.trim().is_empty() {
        blocks.push(json!({ "type": "text", "text": message.text.clone() }));
    }
    for url in &message.image_urls {
        if let Some((bytes, mime)) = image_bytes_from_url(state, url).await? {
            if bytes.len() <= IMAGE_REFERENCE_MAX_BYTES {
                blocks.push(json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": mime,
                        "data": BASE64.encode(&bytes),
                    }
                }));
                continue;
            }
        }
        blocks.push(json!({ "type": "text", "text": "[image]" }));
    }
    Ok(Value::Array(blocks))
}

async fn google_parts_for_message(state: &AppState, message: &ProviderChatMessage) -> AppResult<Vec<Value>> {
    let mut parts = Vec::new();
    if !message.text.trim().is_empty() {
        parts.push(json!({ "text": message.text.clone() }));
    }
    if message.role != "assistant" {
        for url in &message.image_urls {
            if let Some((bytes, mime)) = image_bytes_from_url(state, url).await? {
                if bytes.len() <= IMAGE_REFERENCE_MAX_BYTES {
                    parts.push(json!({
                        "inlineData": {
                            "mimeType": mime,
                            "data": BASE64.encode(&bytes),
                        }
                    }));
                    continue;
                }
            }
            parts.push(json!({ "text": "[image]" }));
        }
    }
    if parts.is_empty() {
        parts.push(json!({ "text": message.text.clone() }));
    }
    Ok(parts)
}

async fn build_openai_responses_input(
    state: &AppState,
    account_id: &str,
    conversation: &ConversationDto,
    assistant: &Value,
) -> AppResult<Vec<Value>> {
    let messages = build_openai_messages(state, account_id, conversation, assistant).await?;
    Ok(messages
        .into_iter()
        .flat_map(openai_message_to_responses_items)
        .collect())
}

fn openai_message_to_responses_items(message: Value) -> Vec<Value> {
    let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
    if role == "tool" {
        return vec![json!({
            "type": "function_call_output",
            "call_id": message.get("tool_call_id").and_then(Value::as_str).unwrap_or_default(),
            "output": message.get("content").and_then(Value::as_str).unwrap_or_default(),
        })];
    }

    let mut items = Vec::new();
    let content = message.get("content").cloned().unwrap_or(Value::String(String::new()));
    let response_content = responses_content_for_openai_content(&content, role);
    if !response_content.is_empty() {
        items.push(json!({
            "type": "message",
            "role": role,
            "content": response_content,
        }));
    }

    for call in message.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
        let Some(function) = call.get("function") else {
            continue;
        };
        let name = function.get("name").and_then(Value::as_str).unwrap_or_default();
        if name.trim().is_empty() {
            continue;
        }
        items.push(json!({
            "type": "function_call",
            "call_id": call.get("id").and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or_else(|| call.get("call_id").and_then(Value::as_str).unwrap_or_default()),
            "name": name,
            "arguments": function.get("arguments").and_then(Value::as_str).unwrap_or("{}"),
        }));
    }

    items
}

fn responses_content_for_openai_content(content: &Value, role: &str) -> Vec<Value> {
    let text_type = if role.eq_ignore_ascii_case("assistant") {
        "output_text"
    } else {
        "input_text"
    };
    match content {
        Value::String(text) => {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![json!({ "type": text_type, "text": text })]
            }
        }
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
                match kind {
                    "text" | "input_text" | "output_text" => item
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| json!({ "type": text_type, "text": text })),
                    "image_url" | "input_image" => {
                        let url = item
                            .get("image_url")
                            .and_then(|value| value.get("url"))
                            .and_then(Value::as_str)
                            .or_else(|| item.get("image_url").and_then(Value::as_str))
                            .or_else(|| item.get("url").and_then(Value::as_str))
                            .filter(|value| !value.trim().is_empty())?;
                        Some(json!({ "type": "input_image", "image_url": url }))
                    }
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

async fn image_bytes_from_url(state: &AppState, raw_url: &str) -> AppResult<Option<(Bytes, String)>> {
    let url = raw_url.trim();
    if url.is_empty() {
        return Ok(None);
    }
    if let Some((mime, payload)) = parse_data_image(url) {
        let bytes = BASE64
            .decode(payload.as_bytes())
            .map_err(|_| AppError::bad_request("Invalid data image"))?;
        return Ok(Some((Bytes::from(bytes), mime)));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let response = state
            .http
            .get(url)
            .send()
            .await
            .map_err(|error| AppError::bad_request(format!("Image download failed: {error}")))?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .filter(|value| value.starts_with("image/"))
            .unwrap_or_else(|| detect_mime_from_url(url))
            .to_string();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| AppError::bad_request(format!("Image download failed: {error}")))?;
        return Ok(Some((bytes, mime)));
    }
    Ok(None)
}

fn selected_messages(conversation: &ConversationDto) -> Vec<&MessageDto> {
    conversation
        .messages
        .iter()
        .filter_map(|node| {
            let index = node.select_index.max(0) as usize;
            node.messages.get(index).or_else(|| node.messages.first())
        })
        .collect()
}

fn add_openai_tools(payload: &mut Value, tools: &[AvailableTool]) {
    if tools.is_empty() {
        return;
    }
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    object.insert(
        "tools".to_string(),
        Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name.clone(),
                            "description": tool.description.clone(),
                            "parameters": tool.parameters.clone(),
                        }
                    })
                })
                .collect(),
        ),
    );
    object.insert("tool_choice".to_string(), Value::String("auto".to_string()));
}

fn add_openai_responses_tools(payload: &mut Value, tools: &[AvailableTool]) {
    if tools.is_empty() {
        return;
    }
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    object.insert(
        "tools".to_string(),
        Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "name": tool.name.clone(),
                        "description": tool.description.clone(),
                        "parameters": tool.parameters.clone(),
                    })
                })
                .collect(),
        ),
    );
    object.insert("tool_choice".to_string(), Value::String("auto".to_string()));
}

fn collect_streaming_tool_calls(delta: &Value, out: &mut std::collections::BTreeMap<usize, StreamingToolCall>) {
    for item in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
        let index = item.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let entry = out.entry(index).or_default();
        if let Some(id) = item.get("id").and_then(Value::as_str).filter(|value| !value.is_empty()) {
            entry.id = Some(id.to_string());
        }
        if let Some(function) = item.get("function") {
            if let Some(name) = function.get("name").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                entry.name.push_str(name);
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                entry.arguments.push_str(arguments);
            }
        }
    }
}

fn collect_responses_function_arguments_delta(event: &Value, out: &mut std::collections::BTreeMap<usize, StreamingToolCall>) {
    let index = event
        .get("output_index")
        .and_then(Value::as_u64)
        .unwrap_or(out.len() as u64) as usize;
    let entry = out.entry(index).or_default();
    if let Some(delta) = event.get("delta").and_then(Value::as_str).filter(|value| !value.is_empty()) {
        entry.arguments.push_str(delta);
    }
}

fn collect_responses_output(
    response: &Value,
    content: &mut String,
    reasoning: &mut String,
    tool_calls: &mut std::collections::BTreeMap<usize, StreamingToolCall>,
) {
    for (index, item) in response.get("output").and_then(Value::as_array).into_iter().flatten().enumerate() {
        collect_responses_output_item(item, index, content, reasoning, tool_calls);
    }
}

fn collect_responses_output_item(
    item: &Value,
    index: usize,
    content: &mut String,
    reasoning: &mut String,
    tool_calls: &mut std::collections::BTreeMap<usize, StreamingToolCall>,
) {
    match item.get("type").and_then(Value::as_str).unwrap_or_default() {
        "message" => {
            if !content.trim().is_empty() {
                return;
            }
            let text = item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(content_part_text)
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if !text.trim().is_empty() {
                content.push_str(&text);
            }
        }
        "reasoning" => {
            if !reasoning.trim().is_empty() {
                return;
            }
            let text = responses_reasoning_text(item);
            if !text.trim().is_empty() {
                reasoning.push_str(&text);
            }
        }
        "function_call" => {
            let entry = tool_calls.entry(index).or_default();
            if let Some(id) = item
                .get("call_id")
                .and_then(Value::as_str)
                .or_else(|| item.get("id").and_then(Value::as_str))
                .filter(|value| !value.is_empty())
            {
                entry.id = Some(id.to_string());
            }
            if let Some(name) = item.get("name").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                entry.name = name.to_string();
            }
            if let Some(arguments) = item.get("arguments").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                entry.arguments = arguments.to_string();
            }
        }
        _ => {}
    }
}

fn content_part_text(part: &Value) -> Option<&str> {
    part.get("text")
        .and_then(Value::as_str)
        .or_else(|| part.get("content").and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
}

fn responses_reasoning_text(item: &Value) -> String {
    let mut chunks = Vec::new();
    collect_text_value(item.get("summary"), &mut chunks);
    collect_text_value(item.get("content"), &mut chunks);
    collect_text_value(item.get("text"), &mut chunks);
    collect_text_value(item.get("reasoning"), &mut chunks);
    collect_text_value(item.get("reasoning_content"), &mut chunks);
    chunks.join("\n")
}

fn collect_text_value<'a>(value: Option<&'a Value>, out: &mut Vec<&'a str>) {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() => out.push(text),
        Some(Value::Array(items)) => {
            for item in items {
                collect_text_value(Some(item), out);
            }
        }
        Some(Value::Object(object)) => {
            for key in ["text", "content", "summary_text", "thinking", "reasoning", "reasoning_content"] {
                collect_text_value(object.get(key), out);
            }
        }
        _ => {}
    }
}

fn build_final_parts_with_tools(
    content: &str,
    reasoning: &str,
    tool_calls: &std::collections::BTreeMap<usize, StreamingToolCall>,
    available_tools: &[AvailableTool],
) -> Vec<Value> {
    let mut parts = build_final_parts(content, reasoning);
    if parts.len() == 1
        && parts[0].get("type").and_then(Value::as_str) == Some("text")
        && parts[0].get("text").and_then(Value::as_str) == Some("Model returned empty response")
        && !tool_calls.is_empty()
    {
        parts.clear();
    }
    let approval_map = available_tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool.needs_approval))
        .collect::<std::collections::HashMap<_, _>>();
    for call in tool_calls.values() {
        let tool_name = call.name.trim();
        if tool_name.is_empty() {
            continue;
        }
        let approval_type = if approval_map.get(tool_name).copied().unwrap_or(false) {
            "pending"
        } else {
            "auto"
        };
        parts.push(json!({
            "type": "tool",
            "toolCallId": call.id.clone().unwrap_or_else(db::random_id),
            "toolName": tool_name,
            "input": if call.arguments.trim().is_empty() { "{}" } else { call.arguments.trim() },
            "output": [],
            "approvalState": { "type": approval_type },
        }));
    }
    if parts.is_empty() {
        parts.push(text_part("Model returned empty response"));
    }
    parts
}

fn extract_openai_tool_parts(message: &Value, available_tools: &[AvailableTool]) -> Vec<Value> {
    let approval_map = available_tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool.needs_approval))
        .collect::<std::collections::HashMap<_, _>>();
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|call| {
            let function = call.get("function")?;
            let tool_name = function.get("name").and_then(Value::as_str)?.trim();
            if tool_name.is_empty() {
                return None;
            }
            let approval_type = if approval_map.get(tool_name).copied().unwrap_or(false) {
                "pending"
            } else {
                "auto"
            };
            Some(json!({
                "type": "tool",
                "toolCallId": call.get("id").and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_string).unwrap_or_else(db::random_id),
                "toolName": tool_name,
                "input": function.get("arguments").and_then(Value::as_str).filter(|value| !value.trim().is_empty()).unwrap_or("{}"),
                "output": [],
                "approvalState": { "type": approval_type },
            }))
        })
        .collect()
}

fn openai_tool_calls_for_message(message: &MessageDto) -> Vec<Value> {
    message
        .parts
        .iter()
        .filter(|part| part_type(part) == "tool")
        .filter_map(|part| {
            let tool_name = part.get("toolName").and_then(Value::as_str)?.trim();
            if tool_name.is_empty() {
                return None;
            }
            Some(json!({
                "id": part.get("toolCallId").and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_string).unwrap_or_else(db::random_id),
                "type": "function",
                "function": {
                    "name": tool_name,
                    "arguments": part.get("input").and_then(Value::as_str).filter(|value| !value.trim().is_empty()).unwrap_or("{}"),
                }
            }))
        })
        .collect()
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

fn part_type(part: &Value) -> String {
    part.get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn first_i64(value: &Value, paths: &[&[&str]]) -> Option<i64> {
    paths.iter().find_map(|path| value_at_path(value, path).and_then(value_to_i64))
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value.round() as i64))
        .or_else(|| value.as_str().and_then(|value| value.trim().parse::<i64>().ok()))
}

fn normalize_provider_type(provider: &Value) -> String {
    let raw = provider
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("openai")
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_");
    match raw.as_str() {
        "claude" | "anthropic" => "anthropic".to_string(),
        "google" | "gemini" => "google".to_string(),
        _ => "openai".to_string(),
    }
}

async fn read_provider_json(response: reqwest::Response) -> AppResult<Value> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::bad_request(format!(
            "Provider request failed ({}): {}",
            status.as_u16(),
            body.chars().take(400).collect::<String>()
        )));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider returned invalid JSON: {error}")))
}

fn extract_anthropic_text_and_reasoning(body: &Value) -> (String, String) {
    let mut content = Vec::new();
    let mut reasoning = Vec::new();
    for item in body.get("content").and_then(Value::as_array).into_iter().flatten() {
        match item.get("type").and_then(Value::as_str).unwrap_or_default() {
            "thinking" => {
                if let Some(text) = item
                    .get("thinking")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("text").and_then(Value::as_str))
                    .filter(|text| !text.trim().is_empty())
                {
                    reasoning.push(text);
                }
            }
            "text" | "" => {
                if let Some(text) = item.get("text").and_then(Value::as_str).filter(|text| !text.trim().is_empty()) {
                    content.push(text);
                }
            }
            _ => {}
        }
    }
    (content.join("\n"), reasoning.join("\n"))
}

fn extract_google_text_and_reasoning(body: &Value) -> (String, String) {
    let mut content = Vec::new();
    let mut reasoning = Vec::new();
    for part in body
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(text) = part.get("text").and_then(Value::as_str).filter(|text| !text.trim().is_empty()) else {
            continue;
        };
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            reasoning.push(text);
        } else {
            content.push(text);
        }
    }
    (content.join("\n"), reasoning.join("\n"))
}

fn build_title_input(conversation: &ConversationDto) -> String {
    selected_messages(conversation)
        .into_iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter_map(|message| {
            let text = prompt_text(&message.parts);
            if text.trim().is_empty() {
                None
            } else {
                Some(format!("{}: {}", message.role.trim().to_ascii_lowercase(), text.trim()))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_generated_title(raw: &str) -> String {
    raw.replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .chars()
        .take(96)
        .collect::<String>()
        .trim()
        .to_string()
}

fn encode_google_model_id(model: &str) -> String {
    let mut out = String::new();
    for ch in model.trim().chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' => out.push(ch),
            other => {
                let mut buf = [0u8; 4];
                for byte in other.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}

fn use_openai_responses_api(provider: &Value) -> bool {
    provider.get("useResponseApi").and_then(Value::as_bool) == Some(true)
}

fn model_supports_reasoning(model: &Value) -> bool {
    if model
        .get("abilities")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .any(|value| value.eq_ignore_ascii_case("REASONING"))
        })
        .unwrap_or(false)
    {
        return true;
    }

    let name = [
        model.get("modelId").and_then(Value::as_str),
        model.get("displayName").and_then(Value::as_str),
        model.get("id").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    model_name_implies_reasoning(&name)
}

fn model_name_implies_reasoning(name: &str) -> bool {
    if name.contains("reason") || name.contains("thinking") || name.contains("think") {
        return true;
    }
    if name.contains("gpt-5") || name.contains("gemini-pro-agent") {
        return true;
    }
    if name.contains("gemini-2.5") || name.contains("gemini-3") {
        return true;
    }
    if name.contains("claude-4")
        || name.contains("claude-opus-4")
        || name.contains("claude-sonnet-4")
        || name.contains("claude-3-7")
    {
        return true;
    }
    name.split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|part| matches!(part, "o1" | "o3" | "o4" | "o5"))
}

fn thinking_budget_for_model(assistant: &Value, model: &Value) -> Option<i64> {
    if !model_supports_reasoning(model) {
        return None;
    }
    assistant.get("thinkingBudget").and_then(value_to_i64)
}

fn reasoning_effort_from_budget(budget: i64) -> Option<&'static str> {
    match budget {
        i64::MIN..=-2 => None,
        -1 => Some("auto"),
        0 => None,
        1..=1024 => Some("low"),
        1025..=16000 => Some("medium"),
        _ => Some("high"),
    }
}

fn add_openai_chat_reasoning(payload: &mut Value, assistant: &Value, model: &Value) {
    let Some(effort) = thinking_budget_for_model(assistant, model).and_then(reasoning_effort_from_budget) else {
        return;
    };
    if let Some(object) = payload.as_object_mut() {
        object.insert("reasoning_effort".to_string(), Value::String(effort.to_string()));
    }
}

fn add_openai_responses_reasoning(payload: &mut Value, assistant: &Value, model: &Value) {
    let Some(effort) = thinking_budget_for_model(assistant, model).and_then(reasoning_effort_from_budget) else {
        return;
    };
    if let Some(object) = payload.as_object_mut() {
        object.insert("reasoning".to_string(), json!({ "effort": effort }));
    }
}

fn add_anthropic_thinking(payload: &mut Value, assistant: &Value, model: &Value) {
    let Some(budget) = thinking_budget_for_model(assistant, model) else {
        return;
    };
    let Some(object) = payload.as_object_mut() else {
        return;
    };

    match budget {
        i64::MIN..=-2 => {}
        0 => {
            object.insert("thinking".to_string(), json!({ "type": "disabled" }));
        }
        -1 => {
            object.insert("thinking".to_string(), json!({ "type": "enabled" }));
        }
        value => {
            object.insert("thinking".to_string(), json!({ "type": "enabled", "budget_tokens": value }));
            let min_max_tokens = value.saturating_add(1024);
            let current = object.get("max_tokens").and_then(value_to_i64).unwrap_or(0);
            if current <= value {
                object.insert("max_tokens".to_string(), json!(min_max_tokens));
            }
        }
    }
}

fn add_google_thinking_config(payload: &mut Value, assistant: &Value, model: &Value) {
    let Some(budget) = thinking_budget_for_model(assistant, model) else {
        return;
    };
    if budget < -1 {
        return;
    }
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    let generation_config = object
        .entry("generationConfig".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !generation_config.is_object() {
        *generation_config = Value::Object(Map::new());
    }
    let Some(generation_config) = generation_config.as_object_mut() else {
        return;
    };
    generation_config.insert(
        "thinkingConfig".to_string(),
        json!({
            "thinkingBudget": budget,
            "includeThoughts": budget != 0,
        }),
    );
}

fn add_generation_options(payload: &mut Value, assistant: &Value) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    if let Some(value) = assistant.get("temperature").and_then(Value::as_f64) {
        object.insert("temperature".to_string(), json!(value));
    }
    if let Some(value) = assistant.get("topP").and_then(Value::as_f64) {
        object.insert("top_p".to_string(), json!(value));
    }
    if let Some(value) = assistant.get("maxTokens").and_then(Value::as_i64) {
        object.insert("max_tokens".to_string(), json!(value));
    }
}

fn add_responses_generation_options(payload: &mut Value, assistant: &Value) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    if let Some(value) = assistant.get("temperature").and_then(Value::as_f64) {
        object.insert("temperature".to_string(), json!(value));
    }
    if let Some(value) = assistant.get("topP").and_then(Value::as_f64) {
        object.insert("top_p".to_string(), json!(value));
    }
    if let Some(value) = assistant.get("maxTokens").and_then(Value::as_i64) {
        object.insert("max_output_tokens".to_string(), json!(value));
    }
}

fn add_custom_bodies(payload: &mut Value, model: &Value) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    for item in model.get("customBodies").and_then(Value::as_array).into_iter().flatten() {
        let Some(key) = item
            .get("key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let value = item
            .get("value")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        object.insert(key.to_string(), parse_custom_body_value(value));
    }
}

fn parse_custom_body_value(value: Value) -> Value {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Value::String(String::new())
            } else {
                serde_json::from_str::<Value>(trimmed).unwrap_or(Value::String(text))
            }
        }
        other => other,
    }
}

fn apply_custom_headers(mut request: RequestBuilder, model: &Value) -> AppResult<RequestBuilder> {
    for item in model.get("customHeaders").and_then(Value::as_array).into_iter().flatten() {
        let Some(name) = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let value = item
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if value.is_empty() {
            continue;
        }
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| AppError::bad_request(format!("Invalid custom header name: {error}")))?;
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|error| AppError::bad_request(format!("Invalid custom header value: {error}")))?;
        request = request.header(name, value);
    }
    Ok(request)
}

fn value_to_form_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn is_image_generation_model(model: &Value) -> bool {
    model.get("imageGenerationMode").and_then(Value::as_bool) == Some(true)
        && model
            .get("outputModalities")
            .and_then(Value::as_array)
            .map(|items| items.iter().any(|item| item.as_str() == Some("IMAGE")))
            .unwrap_or(false)
}

fn extract_delta_text(delta: &Value) -> String {
    match delta.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("text").and_then(|value| value.get("value")).and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn extract_message_parts(message: &Value) -> Vec<Value> {
    let mut parts = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) => append_text_or_image_parts(&mut parts, text),
        Some(Value::Array(items)) => {
            for item in items {
                append_content_item(&mut parts, item);
            }
        }
        _ => {}
    }
    if let Some(b64) = message.get("b64_json").and_then(Value::as_str).filter(|value| !value.is_empty()) {
        parts.push(image_part(&format!("data:image/png;base64,{b64}"), true));
    }
    parts
}

fn extract_image_response_parts(response: &Value) -> Vec<Value> {
    let mut parts = Vec::new();
    if let Some(items) = response.get("data").and_then(Value::as_array) {
        for item in items {
            if let Some(b64) = item.get("b64_json").and_then(Value::as_str) {
                parts.push(image_part(&format!("data:image/png;base64,{b64}"), true));
            } else if let Some(url) = item.get("url").and_then(Value::as_str) {
                parts.push(image_part(url, true));
            }
        }
    }
    parts
}

fn append_content_item(parts: &mut Vec<Value>, item: &Value) {
    let b64 = item
        .get("b64_json")
        .or_else(|| item.get("image_base64"))
        .or_else(|| item.get("image").and_then(|image| image.get("b64_json")))
        .and_then(Value::as_str);
    if let Some(b64) = b64 {
        parts.push(image_part(&format!("data:image/png;base64,{b64}"), true));
        return;
    }
    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
    if matches!(kind, "image_url" | "output_image" | "image") {
        let url = item
            .get("image_url")
            .and_then(|value| value.get("url"))
            .and_then(Value::as_str)
            .or_else(|| item.get("url").and_then(Value::as_str));
        if let Some(url) = url {
            parts.push(image_part(url, true));
            return;
        }
    }
    let text = item
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| item.get("text").and_then(|value| value.get("value")).and_then(Value::as_str))
        .or_else(|| item.get("content").and_then(Value::as_str));
    if let Some(text) = text {
        append_text_or_image_parts(parts, text);
    }
}

fn append_text_or_image_parts(parts: &mut Vec<Value>, raw: &str) {
    let text = raw.trim();
    if text.is_empty() {
        return;
    }
    if text.starts_with("data:image/") && text.contains(";base64,") {
        let compact = text.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
        parts.push(image_part(&compact, true));
        return;
    }
    if let Ok(markdown) = Regex::new(r"!\[[^\]]*\]\(([^)\s]+)\)") {
        let mut cursor = 0usize;
        let mut found = false;
        for cap in markdown.captures_iter(text) {
            let Some(m) = cap.get(0) else { continue };
            let before = text[cursor..m.start()].trim();
            if !before.is_empty() {
                parts.push(text_part(before));
            }
            if let Some(url) = cap.get(1).map(|item| item.as_str()) {
                parts.push(image_part(url, true));
            }
            cursor = m.end();
            found = true;
        }
        if found {
            let after = text[cursor..].trim();
            if !after.is_empty() {
                parts.push(text_part(after));
            }
            return;
        }
    }
    parts.push(text_part(text));
}

fn build_streaming_parts(content: &str, reasoning: &str) -> Vec<Value> {
    let mut parts = Vec::new();
    if !reasoning.trim().is_empty() {
        parts.push(reasoning_part(reasoning));
    }
    if !content.trim().is_empty() {
        parts.push(text_part(content));
    }
    parts
}

fn build_final_parts(content: &str, reasoning: &str) -> Vec<Value> {
    let mut parts = Vec::new();
    if !reasoning.trim().is_empty() {
        parts.push(reasoning_part(reasoning));
    }
    append_text_or_image_parts(&mut parts, content);
    if parts.is_empty() {
        parts.push(text_part("Model returned empty response"));
    }
    parts
}

fn text_part(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

fn reasoning_part(text: &str) -> Value {
    json!({ "type": "reasoning", "reasoning": text })
}

fn image_part(url: &str, generated: bool) -> Value {
    if generated {
        json!({ "type": "image", "url": url, "metadata": { "generatedImage": true } })
    } else {
        json!({ "type": "image", "url": url })
    }
}

fn with_generated_file_metadata(part: Value, file_id: i64, url: &str, storage_provider: &str) -> Value {
    let mut next = part.as_object().cloned().unwrap_or_default();
    next.insert("url".to_string(), Value::String(url.to_string()));
    let mut metadata = next
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    metadata.insert("generatedImage".to_string(), Value::Bool(true));
    metadata.insert("fileId".to_string(), Value::Number(file_id.into()));
    metadata.insert("storageProvider".to_string(), Value::String(storage_provider.to_string()));
    next.insert("metadata".to_string(), Value::Object(metadata));
    Value::Object(next)
}

fn is_generated_image_part(part: &Value) -> bool {
    part.get("type")
        .and_then(Value::as_str)
        .map(|value| value.eq_ignore_ascii_case("image"))
        .unwrap_or(false)
        && part
            .get("metadata")
            .and_then(|metadata| metadata.get("generatedImage"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn parts_to_plain_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter_map(|part| match part.get("type").and_then(Value::as_str).unwrap_or_default() {
            "text" => part.get("text").and_then(Value::as_str),
            "image" => Some("[image]"),
            "reasoning" => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prompt_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter_map(|part| {
            let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
            match kind {
                "text" => part.get("text").and_then(Value::as_str),
                "document" => part.get("fileName").and_then(Value::as_str),
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

async fn prompt_text_with_documents(state: &AppState, account_id: &str, parts: &[Value]) -> AppResult<String> {
    let mut items = Vec::new();
    for part in parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "text" => {
                if let Some(text) = part.get("text").and_then(Value::as_str).filter(|value| !value.trim().is_empty()) {
                    items.push(text.to_string());
                }
            }
            "document" => items.push(document_context_for_part(state, account_id, part).await?),
            _ => {}
        }
    }
    Ok(items.join("\n").trim().to_string())
}

async fn document_context_for_part(state: &AppState, account_id: &str, part: &Value) -> AppResult<String> {
    let fallback_name = part
        .get("fileName")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("document");
    let fallback_mime = part
        .get("mime")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("application/octet-stream");

    let Some(file_id) = part_file_id(part) else {
        return Ok(format_document_unavailable(
            fallback_name,
            fallback_mime,
            "the file id is missing, so the server cannot read and parse this document",
        ));
    };

    let file = match db::get_file_by_id(
        state.config.db_path.clone(),
        account_id.to_string(),
        file_id,
    )
    .await
    {
        Ok(file) => file,
        Err(error) => {
            return Ok(format_document_unavailable(
                fallback_name,
                fallback_mime,
                &format!("file lookup failed: {}", error.message),
            ));
        }
    };

    let name = if file.display_name.trim().is_empty() {
        fallback_name
    } else {
        file.display_name.as_str()
    };
    let mime = if file.mime_type.trim().is_empty() {
        fallback_mime
    } else {
        file.mime_type.as_str()
    };

    let bytes_result = match file_storage::read_local_file_bytes(&state.config.data_dir, &file).await {
        Ok(value) => value,
        Err(error) => {
            return Ok(format_document_unavailable(
                name,
                mime,
                &format!("local file read failed: {}", error.message),
            ));
        }
    };
    let Some((bytes, detected_mime)) = bytes_result else {
        return Ok(format_document_unavailable(
            name,
            mime,
            "the document is stored remotely and cannot be parsed locally; re-upload it to store a local copy",
        ));
    };
    let parse_mime = if detected_mime.trim().is_empty() {
        mime
    } else {
        detected_mime.as_str()
    };

    match document_parser::extract_document_text(name, parse_mime, &bytes) {
        Ok(extracted) => Ok(format_document_context(name, parse_mime, &extracted.text, extracted.truncated)),
        Err(error) => Ok(format_document_unavailable(name, parse_mime, &error)),
    }
}

fn part_file_id(part: &Value) -> Option<i64> {
    part.get("metadata")
        .and_then(|metadata| metadata.get("fileId"))
        .and_then(|value| value.as_i64().or_else(|| value.as_str().and_then(|text| text.parse().ok())))
}

fn format_document_context(name: &str, mime: &str, text: &str, truncated: bool) -> String {
    let note = if truncated {
        "\nNote: the document text was truncated before sending."
    } else {
        ""
    };
    format!(
        "[Document]\nName: {}\nMIME: {}\nContent:\n{}\n[/Document]{}",
        clean_prompt_label(name),
        clean_prompt_label(mime),
        if text.trim().is_empty() { "[No extractable text was found.]" } else { text.trim() },
        note
    )
}

fn format_document_unavailable(name: &str, mime: &str, reason: &str) -> String {
    format!(
        "[Document]\nName: {}\nMIME: {}\nContent:\n[Unable to extract document text locally: {}]\n[/Document]",
        clean_prompt_label(name),
        clean_prompt_label(mime),
        clean_prompt_label(reason)
    )
}

fn clean_prompt_label(value: &str) -> String {
    value
        .replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn requested_image_mode(parts: &[Value]) -> String {
    parts
        .iter()
        .find_map(|part| {
            part.get("metadata")
                .and_then(|metadata| metadata.get("imageGenerationMode"))
                .and_then(Value::as_str)
                .map(|value| value.trim().to_ascii_lowercase())
        })
        .filter(|value| value == "continue_image" || value == "image_to_image")
        .map(|_| "continue_image".to_string())
        .unwrap_or_else(|| "new_image".to_string())
}

fn parse_data_image(url: &str) -> Option<(String, String)> {
    if !url.starts_with("data:image/") {
        return None;
    }
    let comma = url.find(',')?;
    let mime = url[5..comma].split(';').next()?.to_string();
    let payload = url[comma + 1..]
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    Some((mime, payload))
}

fn detect_mime_from_url(url: &str) -> &'static str {
    match url
        .split('?')
        .next()
        .unwrap_or(url)
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        _ => "image/png",
    }
}

pub fn extension_for_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/avif" => "avif",
        "image/heic" => "heic",
        "image/heif" => "heif",
        _ => "png",
    }
}

fn join_url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}
