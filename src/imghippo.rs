use bytes::Bytes;
use reqwest::multipart::{Form, Part};
use serde_json::Value;

use crate::error::{AppError, AppResult};

const IMGHIPPO_UPLOAD_URL: &str = "https://api.imghippo.com/v1/upload";

#[derive(Clone, Debug)]
pub struct ImghippoUpload {
    pub original_url: String,
    pub page_url: Option<String>,
    pub delete_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub size_bytes: i64,
}

pub async fn upload_bytes(
    http: reqwest::Client,
    api_key: String,
    bytes: Bytes,
    file_name: String,
    mime_type: String,
) -> AppResult<ImghippoUpload> {
    if api_key.trim().is_empty() {
        return Err(AppError::bad_request("Imghippo storage is not configured"));
    }

    let size_bytes = bytes.len() as i64;
    let part = Part::bytes(bytes.to_vec())
        .file_name(file_name.clone())
        .mime_str(&mime_type)
        .map_err(|error| AppError::bad_request(format!("invalid mime type: {error}")))?;
    let response = http
        .post(IMGHIPPO_UPLOAD_URL)
        .multipart(Form::new().text("api_key", api_key).text("title", file_name).part("file", part))
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Imghippo upload failed: {error}")))?;

    if !response.status().is_success() {
        return Err(AppError::bad_request(format!(
            "Imghippo upload failed: HTTP {}",
            response.status().as_u16()
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|error| AppError::bad_request(format!("Imghippo upload response failed: {error}")))?;
    parse_upload_response(&body, size_bytes)
}

fn parse_upload_response(body: &str, fallback_size_bytes: i64) -> AppResult<ImghippoUpload> {
    let root: Value = serde_json::from_str(body).map_err(|_| AppError::bad_request("Imghippo upload failed: invalid response"))?;
    if root.get("success").and_then(Value::as_bool) == Some(false) {
        let message = root
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("upload rejected");
        return Err(AppError::bad_request(format!("Imghippo upload failed: {message}")));
    }

    let data = root
        .get("data")
        .ok_or_else(|| AppError::bad_request("Imghippo upload failed: missing data"))?;
    let original_url = data
        .get("view_url")
        .or_else(|| data.get("url"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AppError::bad_request("Imghippo upload failed: missing URL"))?;
    let size_bytes = data
        .get("size")
        .and_then(Value::as_i64)
        .unwrap_or(fallback_size_bytes);

    Ok(ImghippoUpload {
        original_url,
        page_url: data.get("page_url").and_then(Value::as_str).map(str::to_string),
        delete_url: data.get("delete_url").and_then(Value::as_str).map(str::to_string),
        thumbnail_url: data.get("thumbnail_url").and_then(Value::as_str).map(str::to_string),
        size_bytes,
    })
}
