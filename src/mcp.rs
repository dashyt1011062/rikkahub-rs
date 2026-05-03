use std::sync::atomic::{AtomicI64, Ordering};

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::RequestBuilder;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::AppState;

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

#[derive(Clone, Debug)]
pub struct AvailableTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub needs_approval: bool,
}

#[derive(Clone, Debug)]
pub struct ResolvedMcpTool {
    pub server: Value,
    pub tool_name: String,
}

#[derive(Clone, Debug)]
struct McpSession {
    session_id: Option<String>,
}

pub async fn sync_server_tools(state: &AppState, server: &Value) -> Value {
    let server_id = server.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let server_name = server
        .get("commonOptions")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match list_tools(state, server).await {
        Ok(tools) => {
            let old_tools = server
                .get("commonOptions")
                .and_then(|value| value.get("tools"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let next_tools = tools
                .into_iter()
                .map(|tool| {
                    let existing = old_tools.iter().find(|item| item.get("name").and_then(Value::as_str) == Some(tool.name.as_str()));
                    json!({
                        "enable": existing.and_then(|item| item.get("enable")).cloned().unwrap_or(Value::Bool(true)),
                        "name": tool.name,
                        "description": tool.description,
                        "needsApproval": existing.and_then(|item| item.get("needsApproval")).cloned().unwrap_or(Value::Bool(false)),
                        "inputSchema": tool.parameters,
                    })
                })
                .collect::<Vec<_>>();
            let mut updated = server.clone();
            let common = updated
                .get_mut("commonOptions")
                .and_then(Value::as_object_mut);
            if let Some(common) = common {
                common.insert("tools".to_string(), Value::Array(next_tools.clone()));
            }
            json!({
                "serverId": server_id,
                "serverName": server_name,
                "updatedServer": updated,
                "toolsCount": next_tools.len(),
                "error": null,
            })
        }
        Err(error) => json!({
            "serverId": server_id,
            "serverName": server_name,
            "updatedServer": null,
            "toolsCount": 0,
            "error": error.message,
        }),
    }
}

pub fn build_available_tools(settings: &Value, assistant: &Value) -> Vec<AvailableTool> {
    let mut out = Vec::new();
    let server_ids = assistant
        .get("mcpServers")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if server_ids.is_empty() {
        return out;
    }
    for server in settings.get("mcpServers").and_then(Value::as_array).into_iter().flatten() {
        let id = server.get("id").and_then(Value::as_str).unwrap_or_default();
        if !server_ids.iter().any(|item| item == id) {
            continue;
        }
        let Some(common) = server.get("commonOptions") else {
            continue;
        };
        if common.get("enable").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        for tool in common.get("tools").and_then(Value::as_array).into_iter().flatten() {
            if tool.get("enable").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let Some(name) = tool.get("name").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) else {
                continue;
            };
            out.push(AvailableTool {
                name: format!("mcp__{name}"),
                description: tool.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                parameters: normalize_schema(tool.get("inputSchema").cloned()),
                needs_approval: tool.get("needsApproval").and_then(Value::as_bool).unwrap_or(false),
            });
        }
    }
    out
}

pub fn resolve_tool(settings: &Value, assistant: &Value, tool_name: &str) -> Option<ResolvedMcpTool> {
    let bare = tool_name.trim().strip_prefix("mcp__").unwrap_or(tool_name.trim());
    if bare.is_empty() {
        return None;
    }
    let server_ids = assistant
        .get("mcpServers")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    for server in settings.get("mcpServers").and_then(Value::as_array)? {
        let id = server.get("id").and_then(Value::as_str).unwrap_or_default();
        if !server_ids.iter().any(|item| item == id) {
            continue;
        }
        let common = server.get("commonOptions")?;
        if common.get("enable").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let found = common
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|tool| {
                tool.get("enable").and_then(Value::as_bool) != Some(false)
                    && tool.get("name").and_then(Value::as_str) == Some(bare)
            });
        if found {
            return Some(ResolvedMcpTool {
                server: server.clone(),
                tool_name: bare.to_string(),
            });
        }
    }
    None
}

pub async fn call_tool(state: &AppState, resolved: &ResolvedMcpTool, arguments: Value) -> AppResult<Value> {
    let session = initialize(state, &resolved.server).await?;
    let response = mcp_request(
        state,
        &resolved.server,
        Some(&session),
        "tools/call",
        json!({
            "name": resolved.tool_name,
            "arguments": arguments.as_object().cloned().map(Value::Object).unwrap_or_else(|| json!({})),
        }),
    )
    .await?;
    Ok(response.get("result").cloned().unwrap_or(response))
}

async fn list_tools(state: &AppState, server: &Value) -> AppResult<Vec<AvailableTool>> {
    let session = initialize(state, server).await?;
    let response = mcp_request(state, server, Some(&session), "tools/list", json!({})).await?;
    let tools = response
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?.to_string();
            Some(AvailableTool {
                name,
                description: tool.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                parameters: normalize_schema(tool.get("inputSchema").cloned()),
                needs_approval: false,
            })
        })
        .collect::<Vec<_>>();
    Ok(tools)
}

async fn initialize(state: &AppState, server: &Value) -> AppResult<McpSession> {
    let response = mcp_request(
        state,
        server,
        None,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "RikkaHub-Rust", "version": "0.1" },
        }),
    )
    .await?;
    Ok(McpSession {
        session_id: response
            .get("_headers")
            .and_then(|value| value.get("mcp-session-id"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

async fn mcp_request(
    state: &AppState,
    server: &Value,
    session: Option<&McpSession>,
    method: &str,
    params: Value,
) -> AppResult<Value> {
    let url = server
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::bad_request("MCP server url is empty"))?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let request = state
        .http
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", "2024-11-05");
    let request = apply_headers(request, server, session)?;
    let response = request
        .json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("MCP request failed: {error}")))?;
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), Value::String(value.to_string())))
        })
        .collect::<serde_json::Map<_, _>>();
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::bad_request(format!("MCP request failed ({}): {}", status.as_u16(), body.chars().take(300).collect::<String>())));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let body = response.text().await.map_err(|error| AppError::bad_request(format!("MCP response failed: {error}")))?;
    let mut parsed = if content_type.contains("text/event-stream") || body.trim_start().starts_with("event:") || body.contains("\ndata:") {
        parse_sse_json(&body)?
    } else {
        serde_json::from_str::<Value>(&body).map_err(|error| AppError::bad_request(format!("MCP returned invalid JSON: {error}")))?
    };
    if let Some(error) = parsed.get("error") {
        return Err(AppError::bad_request(format!("MCP error: {}", compact_json(error))));
    }
    if let Some(object) = parsed.as_object_mut() {
        object.insert("_headers".to_string(), Value::Object(headers));
    }
    Ok(parsed)
}

fn apply_headers(mut request: RequestBuilder, server: &Value, session: Option<&McpSession>) -> AppResult<RequestBuilder> {
    if let Some(session_id) = session.and_then(|session| session.session_id.as_deref()) {
        request = request.header("mcp-session-id", session_id);
    }
    let common = server.get("commonOptions").unwrap_or(&Value::Null);
    for header in common.get("headers").and_then(Value::as_array).into_iter().flatten() {
        let (name, value) = if let Some(items) = header.as_array() {
            (
                items.first().and_then(Value::as_str).unwrap_or_default(),
                items.get(1).and_then(Value::as_str).unwrap_or_default(),
            )
        } else {
            (
                header
                    .get("first")
                    .or_else(|| header.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                header
                    .get("second")
                    .or_else(|| header.get("value"))
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| AppError::bad_request(format!("Invalid MCP header name: {error}")))?;
        let header_value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|error| AppError::bad_request(format!("Invalid MCP header value: {error}")))?;
        request = request.header(header_name, header_value);
    }
    Ok(request)
}

fn parse_sse_json(body: &str) -> AppResult<Value> {
    let mut data_lines = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_end();
        if let Some(data) = trimmed.strip_prefix("data:") {
            data_lines.push(data.trim().to_string());
        } else if trimmed.is_empty() && !data_lines.is_empty() {
            let raw = data_lines.join("\n");
            return serde_json::from_str::<Value>(&raw)
                .map_err(|error| AppError::bad_request(format!("MCP returned invalid SSE JSON: {error}")));
        }
    }
    if !data_lines.is_empty() {
        let raw = data_lines.join("\n");
        return serde_json::from_str::<Value>(&raw)
            .map_err(|error| AppError::bad_request(format!("MCP returned invalid SSE JSON: {error}")));
    }
    Err(AppError::bad_request("MCP returned empty SSE response"))
}

fn normalize_schema(schema: Option<Value>) -> Value {
    let Some(schema) = schema else {
        return json!({ "type": "object", "properties": {} });
    };
    if schema.get("type").is_some() {
        return schema;
    }
    json!({
        "type": "object",
        "properties": schema.get("properties").cloned().unwrap_or_else(|| json!({})),
        "required": schema.get("required").cloned().unwrap_or_else(|| json!([])),
    })
}

fn compact_json(value: &Value) -> String {
    value.to_string().chars().take(240).collect()
}
