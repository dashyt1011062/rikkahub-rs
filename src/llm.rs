use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bytes::Bytes;
use futures_util::StreamExt;
use regex::Regex;
use reqwest::multipart::{Form, Part};
use reqwest::RequestBuilder;
use serde_json::{json, Map, Value};

use crate::db::{self, ConversationDto, MessageDto};
use crate::error::{AppError, AppResult};
use crate::imgpile;
use crate::AppState;

const DEFAULT_IMAGE_GENERATION_PROMPT: &str = "Generate an image.";
const DEFAULT_IMAGE_EDIT_PROMPT: &str = "Edit the image according to the prompt.";
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

    let messages = build_openai_messages(state, account_id, conversation).await?;
    let model_id = selection.model.get("id").and_then(Value::as_str).map(str::to_string);
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    add_generation_options(&mut payload, &selection.assistant);
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

    if content.trim().is_empty() && reasoning.trim().is_empty() {
        return generate_reply_non_streaming(state, account_id, &selection, conversation).await;
    }

    Ok(AssistantGenerationResult {
        parts: build_final_parts(&content, &reasoning),
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
        if raw_url.starts_with("https://cdn.imgpile.com/") || raw_url.starts_with("http://cdn.imgpile.com/") {
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
            stored.push(with_generated_file_metadata(part, record.id, &raw_url));
            index += 1;
            continue;
        }
        let Some((bytes, mime_type)) = image_bytes_from_url(state, &raw_url).await? else {
            stored.push(part);
            continue;
        };
        let file_name = format!("generated-image-{}-{}.{}", db::now_millis(), index, extension_for_mime(&mime_type));
        let upload = imgpile::upload_bytes(
            state.http.clone(),
            state.config.imgpile_key.clone(),
            bytes.clone(),
            file_name.clone(),
            mime_type.clone(),
        )
        .await?;
        let record = db::insert_remote_file(
            state.config.db_path.clone(),
            account_id.to_string(),
            file_name,
            mime_type,
            upload.size_bytes,
            "imgpile".to_string(),
            upload.original_url.clone(),
            upload.page_url,
            upload.delete_url,
            upload.thumbnail_url,
        )
        .await?;
        stored.push(with_generated_file_metadata(part, record.id, &upload.original_url));
        index += 1;
    }
    Ok(stored)
}

async fn generate_reply_non_streaming(
    state: &AppState,
    account_id: &str,
    selection: &Selection,
    conversation: &ConversationDto,
) -> AppResult<AssistantGenerationResult> {
    let messages = build_openai_messages(state, account_id, conversation).await?;
    let mut payload = json!({
        "model": selection.model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "messages": messages,
    });
    add_generation_options(&mut payload, &selection.assistant);
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

fn select_model(settings: &Value, conversation: &ConversationDto) -> AppResult<Selection> {
    let assistant = settings
        .get("assistants")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(conversation.assistant_id.as_str()))
        })
        .cloned()
        .unwrap_or_else(|| json!({}));
    let model_id = assistant
        .get("chatModelId")
        .or_else(|| settings.get("chatModelId"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && *value != "auto")
        .ok_or_else(|| AppError::bad_request("No chat model selected"))?;

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
                model: model.clone(),
                assistant,
                api_key,
            });
        }
    }
    Err(AppError::bad_request("Selected model not found"))
}

async fn build_openai_messages(state: &AppState, account_id: &str, conversation: &ConversationDto) -> AppResult<Vec<Value>> {
    let mut messages = Vec::new();
    for node in &conversation.messages {
        let index = node.select_index.max(0) as usize;
        let Some(message) = node.messages.get(index).or_else(|| node.messages.first()) else {
            continue;
        };
        let role = match message.role.trim().to_ascii_uppercase().as_str() {
            "ASSISTANT" => "assistant",
            "SYSTEM" => "system",
            _ => "user",
        };
        let content = openai_content_for_message(state, account_id, message, role).await?;
        messages.push(json!({
            "role": role,
            "content": content,
        }));
    }
    Ok(messages)
}

async fn openai_content_for_message(state: &AppState, account_id: &str, message: &MessageDto, role: &str) -> AppResult<Value> {
    if role == "assistant" {
        return Ok(Value::String(parts_to_plain_text(&message.parts)));
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
                if let Some(url) = resolve_part_image_url(state, account_id, part).await? {
                    blocks.push(json!({ "type": "image_url", "image_url": { "url": url } }));
                }
            }
            "document" => {
                let name = part.get("fileName").and_then(Value::as_str).unwrap_or("document");
                blocks.push(json!({ "type": "text", "text": format!("[document: {name}]") }));
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
            if let Some(remote) = file.remote_url.filter(|value| !value.trim().is_empty()) {
                return Ok(Some(remote));
            }
        }
    }
    Ok(part
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string))
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
    json!({ "type": "reasoning", "text": text })
}

fn image_part(url: &str, generated: bool) -> Value {
    if generated {
        json!({ "type": "image", "url": url, "metadata": { "generatedImage": true } })
    } else {
        json!({ "type": "image", "url": url })
    }
}

fn with_generated_file_metadata(part: Value, file_id: i64, url: &str) -> Value {
    let mut next = part.as_object().cloned().unwrap_or_default();
    next.insert("url".to_string(), Value::String(url.to_string()));
    let mut metadata = next
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    metadata.insert("generatedImage".to_string(), Value::Bool(true));
    metadata.insert("fileId".to_string(), Value::Number(file_id.into()));
    metadata.insert("storageProvider".to_string(), Value::String("imgpile".to_string()));
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
