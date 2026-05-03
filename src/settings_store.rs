use std::time::SystemTime;

use serde_json::Value;
use tokio::fs;

use crate::config::{AppConfig, DEFAULT_ASSISTANT_ID};
use crate::error::{AppError, AppResult};

pub async fn read_settings(config: &AppConfig, account_id: &str) -> AppResult<Value> {
    let path = config.settings_path_for_account(account_id);
    let raw = fs::read_to_string(&path)
        .await
        .map_err(|error| AppError::not_found(format!("settings not found: {} ({error})", path.display())))?;
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

