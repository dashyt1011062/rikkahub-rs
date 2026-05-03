use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Extension, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::StreamExt;
use futures_util::Stream;
use serde_json::{json, Value};

use crate::auth::AccountId;
use crate::error::{AppError, AppResult};
use crate::mcp;
use crate::settings_store;
use crate::AppState;

pub async fn replace(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(value): Json<Value>,
) -> AppResult<Json<Value>> {
    settings_store::write_settings(&state.config, &account.0, &value).await?;
    Ok(Json(json!({ "status": "ok" })))
}

pub async fn assistant(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let assistant_id = request.get("assistantId").and_then(Value::as_str).unwrap_or_default();
    update_settings(&state, &account.0, |settings| {
        settings["assistantId"] = Value::String(assistant_id.to_string());
        Ok(())
    })
    .await
}

pub async fn assistant_model(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let assistant_id = request.get("assistantId").and_then(Value::as_str).unwrap_or_default().to_string();
    let model_id = request.get("modelId").and_then(Value::as_str).unwrap_or_default().to_string();
    update_settings(&state, &account.0, |settings| {
        mutate_assistant(settings, &assistant_id, |assistant| {
            assistant["chatModelId"] = Value::String(model_id);
        });
        Ok(())
    })
    .await
}

pub async fn assistant_thinking_budget(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let assistant_id = request.get("assistantId").and_then(Value::as_str).unwrap_or_default().to_string();
    let budget = request.get("thinkingBudget").cloned();
    update_settings(&state, &account.0, |settings| {
        mutate_assistant(settings, &assistant_id, |assistant| {
            if let Some(value) = budget {
                assistant["thinkingBudget"] = value;
            } else if let Some(object) = assistant.as_object_mut() {
                object.remove("thinkingBudget");
            }
        });
        Ok(())
    })
    .await
}

pub async fn assistant_mcp(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let assistant_id = request.get("assistantId").and_then(Value::as_str).unwrap_or_default().to_string();
    let servers = request.get("mcpServerIds").cloned().unwrap_or_else(|| json!([]));
    update_settings(&state, &account.0, |settings| {
        mutate_assistant(settings, &assistant_id, |assistant| {
            assistant["mcpServers"] = servers;
        });
        Ok(())
    })
    .await
}

pub async fn assistant_injections(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let assistant_id = request.get("assistantId").and_then(Value::as_str).unwrap_or_default().to_string();
    let mode = request.get("modeInjectionIds").cloned().unwrap_or_else(|| json!([]));
    let lore = request.get("lorebookIds").cloned().unwrap_or_else(|| json!([]));
    update_settings(&state, &account.0, |settings| {
        mutate_assistant(settings, &assistant_id, |assistant| {
            assistant["modeInjections"] = mode;
            assistant["lorebooks"] = lore;
        });
        Ok(())
    })
    .await
}

pub async fn search_enabled(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let enabled = request.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    update_settings(&state, &account.0, |settings| {
        settings["enableWebSearch"] = Value::Bool(enabled);
        Ok(())
    })
    .await
}

pub async fn search_service(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let index = request.get("index").and_then(Value::as_i64).unwrap_or(0);
    update_settings(&state, &account.0, |settings| {
        settings["searchServiceSelected"] = Value::Number(index.into());
        Ok(())
    })
    .await
}

pub async fn favorite_models(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let models = request.get("modelIds").cloned().unwrap_or_else(|| json!([]));
    update_settings(&state, &account.0, |settings| {
        settings["favoriteModels"] = models;
        Ok(())
    })
    .await
}

pub async fn model_builtin_tool(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let model_id = request.get("modelId").and_then(Value::as_str).unwrap_or_default().to_string();
    let tool = request.get("tool").and_then(Value::as_str).unwrap_or_default().to_string();
    let enabled = request.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    update_settings(&state, &account.0, |settings| {
        for provider in settings.get_mut("providers").and_then(Value::as_array_mut).into_iter().flatten() {
            for model in provider.get_mut("models").and_then(Value::as_array_mut).into_iter().flatten() {
                if model.get("id").and_then(Value::as_str) != Some(model_id.as_str()) {
                    continue;
                }
                if !model.get("tools").map(Value::is_array).unwrap_or(false) {
                    model["tools"] = json!([]);
                }
                let tools = model.get_mut("tools").and_then(Value::as_array_mut).unwrap();
                if enabled {
                    if !tools.iter().any(|item| item.get("type").and_then(Value::as_str) == Some(tool.as_str())) {
                        tools.push(json!({ "type": tool }));
                    }
                } else {
                    tools.retain(|item| item.get("type").and_then(Value::as_str) != Some(tool.as_str()));
                }
            }
        }
        Ok(())
    })
    .await
}

pub async fn provider_models_fetch(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let provider_id = request.get("providerId").and_then(Value::as_str).unwrap_or_default();
    let settings = settings_store::read_settings(&state.config, &account.0).await?;
    let provider = find_provider(&settings, provider_id).ok_or_else(|| AppError::not_found("Provider not found"))?;
    let models = match normalize_provider_type(provider).as_str() {
        "google" => fetch_google_models(&state, provider).await?,
        "anthropic" => fetch_anthropic_models(&state, provider).await?,
        _ => fetch_openai_models(&state, provider).await?,
    };
    Ok(Json(json!({ "providerId": provider_id, "models": models })))
}

pub async fn provider_model_test(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let provider_id = request.get("providerId").and_then(Value::as_str).unwrap_or_default();
    let model_ref = request.get("modelId").and_then(Value::as_str).unwrap_or_default();
    let settings = settings_store::read_settings(&state.config, &account.0).await?;
    let provider = find_provider(&settings, provider_id).ok_or_else(|| AppError::not_found("Provider not found"))?;
    let model = provider
        .get("models")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|item| item.get("id").and_then(Value::as_str) == Some(model_ref)))
        .ok_or_else(|| AppError::not_found("Model not found"))?;
    let provider_type = normalize_provider_type(provider);
    let non_streaming = if provider_type == "openai" {
        run_chat_model_test(&state, provider, model, false).await
    } else {
        run_direct_model_test(&state, provider, model, &provider_type).await
    };
    let streaming = if provider_type == "openai" {
        run_chat_model_test(&state, provider, model, true).await
    } else {
        json!({ "status": "skipped", "output": "此供应商使用非流式测试" })
    };
    let tool_call = if provider_type == "openai" {
        run_tool_model_test(&state, provider, model).await
    } else {
        json!({ "status": "skipped", "output": "此供应商暂不做工具调用测试" })
    };
    Ok(Json(json!({
        "providerId": provider_id,
        "modelId": model_ref,
        "nonStreaming": non_streaming,
        "streaming": streaming,
        "toolCall": tool_call
    })))
}

pub async fn mcp_sync(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> AppResult<Json<Value>> {
    let mut settings = settings_store::read_settings(&state.config, &account.0).await?;
    let requested = request
        .get("serverIds")
        .or_else(|| request.get("mcpServerIds"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    let servers = settings
        .get("mcpServers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut results = Vec::new();
    let mut updated_servers = servers.clone();
    for (index, server) in servers.iter().enumerate() {
        let server_id = server.get("id").and_then(Value::as_str).unwrap_or_default();
        if !requested.is_empty() && !requested.iter().any(|item| item == server_id) {
            continue;
        }
        let result = mcp::sync_server_tools(&state, server).await;
        if let Some(updated) = result.get("updatedServer").filter(|value| !value.is_null()) {
            updated_servers[index] = updated.clone();
        }
        results.push(result);
    }
    if let Some(object) = settings.as_object_mut() {
        object.insert("mcpServers".to_string(), Value::Array(updated_servers));
    }
    settings_store::write_settings(&state.config, &account.0, &settings).await?;
    Ok(Json(json!({ "status": "ok", "results": results })))
}

pub async fn stream(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let config = state.config.clone();
    let account_id = account.0;

    let stream = stream! {
        let mut last_modified = None;
        loop {
            let modified = settings_store::settings_modified_at(&config, &account_id).await;
            if last_modified.is_none() || modified != last_modified {
                last_modified = modified;
                if let Ok(settings) = settings_store::read_settings(&config, &account_id).await {
                    let data = serde_json::to_string(&settings).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("update").data(data));
                }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn update_settings<F>(state: &AppState, account_id: &str, mutate: F) -> AppResult<Json<Value>>
where
    F: FnOnce(&mut Value) -> AppResult<()>,
{
    let mut settings = settings_store::read_settings(&state.config, account_id).await?;
    mutate(&mut settings)?;
    settings_store::write_settings(&state.config, account_id, &settings).await?;
    Ok(Json(json!({ "status": "ok" })))
}

fn mutate_assistant<F>(settings: &mut Value, assistant_id: &str, mutate: F)
where
    F: FnOnce(&mut Value),
{
    if let Some(assistants) = settings.get_mut("assistants").and_then(Value::as_array_mut) {
        if let Some(assistant) = assistants
            .iter_mut()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(assistant_id))
        {
            mutate(assistant);
        }
    }
}

fn find_provider<'a>(settings: &'a Value, provider_id: &str) -> Option<&'a Value> {
    settings
        .get("providers")
        .and_then(Value::as_array)?
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(provider_id))
}

async fn fetch_openai_models(state: &AppState, provider: &Value) -> AppResult<Vec<Value>> {
    let base = provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.openai.com/v1");
    let api_key = provider.get("apiKey").and_then(Value::as_str).unwrap_or_default();
    let response = state
        .http
        .get(format!("{}/models", base.trim_end_matches('/')))
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Fetch models failed: {error}")))?;
    let body = read_json_response(response, "Fetch models failed").await?;
    Ok(body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
        .map(|id| json!({ "modelId": id, "displayName": id, "type": "CHAT" }))
        .collect())
}

async fn fetch_anthropic_models(state: &AppState, provider: &Value) -> AppResult<Vec<Value>> {
    let base = provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.anthropic.com/v1");
    let api_key = provider.get("apiKey").and_then(Value::as_str).unwrap_or_default();
    let response = state
        .http
        .get(format!("{}/models", base.trim_end_matches('/')))
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Fetch models failed: {error}")))?;
    let body = read_json_response(response, "Fetch models failed").await?;
    Ok(body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?;
            let name = item.get("display_name").and_then(Value::as_str).unwrap_or(id);
            Some(json!({ "modelId": id, "displayName": name, "type": "CHAT" }))
        })
        .collect())
}

async fn fetch_google_models(state: &AppState, provider: &Value) -> AppResult<Vec<Value>> {
    let base = provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://generativelanguage.googleapis.com/v1beta");
    let api_key = provider.get("apiKey").and_then(Value::as_str).unwrap_or_default();
    let response = state
        .http
        .get(format!("{}/models?key={}", base.trim_end_matches('/'), api_key))
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Fetch models failed: {error}")))?;
    let body = read_json_response(response, "Fetch models failed").await?;
    Ok(body
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let raw = item.get("name").and_then(Value::as_str)?;
            let id = raw.strip_prefix("models/").unwrap_or(raw);
            let display = item.get("displayName").and_then(Value::as_str).unwrap_or(id);
            let supports_generate = item
                .get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .map(|methods| methods.iter().any(|method| method.as_str() == Some("generateContent")))
                .unwrap_or(true);
            supports_generate.then(|| json!({ "modelId": id, "displayName": display, "type": "CHAT" }))
        })
        .collect())
}

async fn run_chat_model_test(state: &AppState, provider: &Value, model: &Value, stream: bool) -> Value {
    let mut payload = json!({
        "model": model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "messages": [{ "role": "user", "content": "只回复 pong" }],
        "max_tokens": 16,
        "stream": stream,
    });
    add_custom_bodies(&mut payload, model);
    payload["stream"] = Value::Bool(stream);
    let response = send_chat_test_request(state, provider, model, payload).await;
    match response {
        Ok(resp) if resp.status().is_success() && stream => match read_stream_test_output(resp).await {
            Ok(output) => json!({ "status": "success", "output": output }),
            Err(error) => json!({ "status": "error", "error": error.message }),
        },
        Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
            Ok(body) => json!({ "status": "success", "output": extract_test_text(&body) }),
            Err(error) => json!({ "status": "error", "error": format!("Invalid JSON: {error}") }),
        },
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            json!({ "status": "error", "error": format!("HTTP {} {}", status.as_u16(), body.chars().take(240).collect::<String>()) })
        }
        Err(error) => json!({ "status": "error", "error": error.message }),
    }
}

async fn run_tool_model_test(state: &AppState, provider: &Value, model: &Value) -> Value {
    let mut payload = json!({
        "model": model.get("modelId").and_then(Value::as_str).unwrap_or("auto"),
        "messages": [{ "role": "user", "content": "调用 ping 工具。" }],
        "max_tokens": 32,
        "tools": [{
            "type": "function",
            "function": {
                "name": "ping",
                "description": "connection test",
                "parameters": { "type": "object", "properties": {} }
            }
        }],
        "tool_choice": { "type": "function", "function": { "name": "ping" } },
    });
    add_custom_bodies(&mut payload, model);
    let response = send_chat_test_request(state, provider, model, payload).await;
    match response {
        Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
            Ok(body) => {
                let has_tool_call = body
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|choice| choice.get("message"))
                    .and_then(|message| message.get("tool_calls"))
                    .and_then(Value::as_array)
                    .map(|items| !items.is_empty())
                    .unwrap_or(false);
                if has_tool_call {
                    json!({ "status": "success", "output": "tool call ok" })
                } else {
                    json!({ "status": "success", "output": "request ok, no tool call returned" })
                }
            }
            Err(error) => json!({ "status": "error", "error": format!("Invalid JSON: {error}") }),
        },
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            json!({ "status": "error", "error": format!("HTTP {} {}", status.as_u16(), body.chars().take(240).collect::<String>()) })
        }
        Err(error) => json!({ "status": "error", "error": error.message }),
    }
}

async fn run_direct_model_test(state: &AppState, provider: &Value, model: &Value, provider_type: &str) -> Value {
    let result = match provider_type {
        "anthropic" => run_anthropic_model_test(state, provider, model).await,
        "google" => run_google_model_test(state, provider, model).await,
        _ => Err(AppError::bad_request("unsupported provider")),
    };
    match result {
        Ok(output) => json!({ "status": "success", "output": output }),
        Err(error) => json!({ "status": "error", "error": error.message }),
    }
}

async fn run_anthropic_model_test(state: &AppState, provider: &Value, model: &Value) -> AppResult<String> {
    let base = provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.anthropic.com/v1");
    let api_key = provider.get("apiKey").and_then(Value::as_str).unwrap_or_default();
    let payload = json!({
        "model": model.get("modelId").and_then(Value::as_str).unwrap_or("claude"),
        "messages": [{ "role": "user", "content": "只回复 pong" }],
        "max_tokens": 16,
    });
    let response = state
        .http
        .post(format!("{}/messages", base.trim_end_matches('/')))
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    let body = read_json_response(response, "Provider request failed").await?;
    Ok(body
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(160)
        .collect())
}

async fn run_google_model_test(state: &AppState, provider: &Value, model: &Value) -> AppResult<String> {
    let base = provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://generativelanguage.googleapis.com/v1beta");
    let api_key = provider.get("apiKey").and_then(Value::as_str).unwrap_or_default();
    let model_id = encode_google_model_id(model.get("modelId").and_then(Value::as_str).unwrap_or("gemini-2.0-flash"));
    let payload = json!({
        "contents": [{ "role": "user", "parts": [{ "text": "只回复 pong" }] }],
        "generationConfig": { "maxOutputTokens": 16 },
    });
    let response = state
        .http
        .post(format!("{}/models/{}:generateContent?key={}", base.trim_end_matches('/'), model_id, api_key))
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))?;
    let body = read_json_response(response, "Provider request failed").await?;
    Ok(extract_google_text(&body).chars().take(160).collect())
}

async fn send_chat_test_request(
    state: &AppState,
    provider: &Value,
    model: &Value,
    payload: Value,
) -> AppResult<reqwest::Response> {
    let base = provider.get("baseUrl").and_then(Value::as_str).unwrap_or("https://api.openai.com/v1");
    let api_key = provider.get("apiKey").and_then(Value::as_str).unwrap_or_default();
    let path = provider
        .get("chatCompletionsPath")
        .and_then(Value::as_str)
        .unwrap_or("/chat/completions");
    let request = state
        .http
        .post(format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/')))
        .bearer_auth(api_key);
    apply_custom_headers(request, model)?
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Provider request failed: {error}")))
}

async fn read_stream_test_output(response: reqwest::Response) -> AppResult<String> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut output = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| AppError::bad_request(format!("stream failed: {error}")))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim().to_string();
            buffer = buffer[index + 1..].to_string();
            if !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                return Ok(if output.trim().is_empty() { "stream ok".to_string() } else { output });
            }
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(delta) = value
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
            {
                output.push_str(delta);
            }
            if output.len() > 120 {
                output.truncate(120);
                return Ok(output);
            }
        }
    }
    Ok(if output.trim().is_empty() { "stream ok".to_string() } else { output })
}

fn extract_test_text(body: &Value) -> String {
    body.get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("ok")
        .chars()
        .take(160)
        .collect()
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

fn apply_custom_headers(mut request: reqwest::RequestBuilder, model: &Value) -> AppResult<reqwest::RequestBuilder> {
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

async fn read_json_response(response: reqwest::Response, label: &str) -> AppResult<Value> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::bad_request(format!(
            "{label}: HTTP {} {}",
            status.as_u16(),
            body.chars().take(240).collect::<String>()
        )));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| AppError::bad_request(format!("{label}: {error}")))
}

fn extract_google_text(body: &Value) -> String {
    body.get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
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
