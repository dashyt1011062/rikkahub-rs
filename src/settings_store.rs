use std::io::ErrorKind;
use std::time::SystemTime;

use serde_json::{json, Value};
use tokio::fs;

use crate::config::{AppConfig, DEFAULT_ASSISTANT_ID};
use crate::error::{AppError, AppResult};

pub async fn read_settings(config: &AppConfig, account_id: &str) -> AppResult<Value> {
    let path = config.settings_path_for_account(account_id);
    let raw = match fs::read_to_string(&path).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let settings = default_settings();
            write_settings(config, account_id, &settings).await?;
            return Ok(settings);
        }
        Err(error) => {
            return Err(AppError::internal(format!(
                "settings read failed: {} ({error})",
                path.display()
            )))
        }
    };
    serde_json::from_str::<Value>(&raw).map_err(AppError::from)
}

pub async fn write_settings(config: &AppConfig, account_id: &str, value: &Value) -> AppResult<()> {
    let path = config.settings_path_for_account(account_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let raw = serde_json::to_vec(value)?;
    fs::write(path, raw).await?;
    Ok(())
}

pub async fn settings_modified_at(config: &AppConfig, account_id: &str) -> Option<SystemTime> {
    let path = config.settings_path_for_account(account_id);
    fs::metadata(path).await.ok()?.modified().ok()
}

pub async fn current_assistant_id(config: &AppConfig, account_id: &str) -> String {
    read_settings(config, account_id)
        .await
        .ok()
        .and_then(|settings| settings.get("assistantId").and_then(Value::as_str).map(str::to_string))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ASSISTANT_ID.to_string())
}

/// Settings used when an account has no settings.json yet (fresh data dir or new account),
/// shaped like the Settings type the web UI expects.
fn default_settings() -> Value {
    json!({
        "dynamicColor": true,
        "themeId": "default",
        "developerMode": false,
        "displaySetting": {
            "userNickname": "",
            "showUserAvatar": true,
            "showModelIcon": true,
            "showModelName": false,
            "showTokenUsage": true,
            "autoCloseThinking": true,
            "codeBlockAutoWrap": false,
            "codeBlockAutoCollapse": false,
            "showLineNumbers": false,
            "sendOnEnter": false,
            "enableAutoScroll": true,
            "fontSizeRatio": 1.0
        },
        "enableWebSearch": false,
        "favoriteModels": [],
        "chatModelId": "",
        "titleModelId": "",
        "assistantId": DEFAULT_ASSISTANT_ID,
        "providers": [],
        "assistants": [{
            "id": DEFAULT_ASSISTANT_ID,
            "name": "默认助手",
            "tags": [],
            "systemPrompt": "",
            "messageTemplate": "{{ message }}",
            "quickMessages": [],
            "customHeaders": [],
            "customBodies": [],
            "mcpServers": [],
            "modeInjectionIds": [],
            "lorebookIds": [],
            "localTools": ["time_info"],
            "streamOutput": true
        }],
        "assistantTags": [],
        "modeInjections": [],
        "lorebooks": [],
        "modelLibrary": [],
        "mcpServers": [],
        "searchServices": [],
        "searchServiceSelected": 0
    })
}
