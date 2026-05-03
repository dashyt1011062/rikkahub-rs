use bytes::Bytes;
use reqwest::multipart::{Form, Part};
use serde_json::Value;

use crate::error::{AppError, AppResult};

const IMGPILE_UPLOAD_URL: &str = "https://cdn.imgpile.com/api/v1/media";
const IMGPILE_MEDIA_API_BASE_URL: &str = "https://imgpile.com/api/v1/media";
const IMGPILE_CDN_FILE_BASE_URL: &str = "https://cdn.imgpile.com/f";

#[derive(Clone, Debug)]
pub struct ImgpileUpload {
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
) -> AppResult<ImgpileUpload> {
    if api_key.trim().is_empty() {
        return Err(AppError::bad_request("Imgpile storage is not configured"));
    }
    let size_bytes = bytes.len() as i64;
    let part = Part::bytes(bytes.to_vec())
        .file_name(file_name)
        .mime_str(&mime_type)
        .map_err(|error| AppError::bad_request(format!("invalid mime type: {error}")))?;
    let response = http
        .post(IMGPILE_UPLOAD_URL)
        .bearer_auth(api_key)
        .multipart(Form::new().part("file", part))
        .send()
        .await
        .map_err(|error| AppError::bad_request(format!("Imgpile upload failed: {error}")))?;

    if !response.status().is_success() {
        return Err(AppError::bad_request(format!(
            "Imgpile upload failed: HTTP {}",
            response.status().as_u16()
        )));
    }
    let body = response
        .text()
        .await
        .map_err(|error| AppError::bad_request(format!("Imgpile upload response failed: {error}")))?;
    parse_upload_response(&body, size_bytes)
}

fn parse_upload_response(body: &str, size_bytes: i64) -> AppResult<ImgpileUpload> {
    let root: Value = serde_json::from_str(body).map_err(|_| AppError::bad_request("Imgpile upload failed: invalid response"))?;
    let media = root
        .get("media")
        .or_else(|| root.get("data"))
        .ok_or_else(|| AppError::bad_request("Imgpile upload failed: missing media"))?;
    let slug = media
        .get("slug")
        .or_else(|| media.get("filename"))
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::bad_request("Imgpile upload failed: missing slug"))?;
    let urls = media.get("urls");
    let original_url = urls
        .and_then(|item| item.get("original"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| fallback_url(slug, media))
        .ok_or_else(|| AppError::bad_request("Imgpile upload failed: missing original URL"))?;
    let thumbnail_url = urls
        .and_then(|item| item.get("thumb").or_else(|| item.get("xs")).or_else(|| item.get("sm")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let page_url = urls
        .and_then(|item| item.get("page"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(ImgpileUpload {
        original_url,
        page_url,
        delete_url: Some(format!("{IMGPILE_MEDIA_API_BASE_URL}/{slug}")),
        thumbnail_url,
        size_bytes,
    })
}

fn fallback_url(slug: &str, media: &Value) -> Option<String> {
    let ext = media
        .get("ext")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .trim_start_matches('.');
    Some(format!("{IMGPILE_CDN_FILE_BASE_URL}/{slug}.{ext}"))
}
