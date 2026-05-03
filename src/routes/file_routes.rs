use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::header::{ACCEPT, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::Redirect;
use futures_util::{StreamExt, TryStreamExt};
use mime_guess::mime;
use serde::Deserialize;
use tokio_util::io::ReaderStream;

use crate::auth::AccountId;
use crate::db::{self, ManagedFileRecord};
use crate::error::{AppError, AppResult};
use crate::AppState;

#[derive(Deserialize)]
pub struct FileQuery {
    proxy: Option<String>,
}

pub async fn by_id(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Query(query): Query<FileQuery>,
) -> AppResult<Response<Body>> {
    let file = db::get_file_by_id(state.config.db_path.clone(), account.0, id).await?;
    respond_file_record(state, file, truthy(query.proxy.as_deref())).await
}

pub async fn by_path(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> AppResult<Response<Body>> {
    let relative_path = sanitize_relative_path(&path)?;
    let file = db::get_file_by_path(state.config.db_path.clone(), account.0, relative_path).await?;
    respond_local_file(&state.config.data_dir, &file).await
}

pub async fn upload_not_implemented() -> AppResult<StatusCode> {
    Err(AppError::not_implemented(
        "File upload is not implemented in the Rust preview yet",
    ))
}

async fn respond_file_record(state: AppState, file: ManagedFileRecord, proxy: bool) -> AppResult<Response<Body>> {
    if let Some(remote_url) = file.remote_url.as_deref().filter(|value| !value.trim().is_empty()) {
        if proxy {
            return proxy_remote_file(state, remote_url.to_string(), file.mime_type).await;
        }
        return Ok(Redirect::temporary(remote_url).into_response());
    }
    respond_local_file(&state.config.data_dir, &file).await
}

async fn proxy_remote_file(state: AppState, remote_url: String, fallback_mime: String) -> AppResult<Response<Body>> {
    if !remote_url.starts_with("http://") && !remote_url.starts_with("https://") {
        return Err(AppError::bad_request("Unsupported remote file url"));
    }

    let _permit = state
        .file_proxy_permits
        .acquire()
        .await
        .map_err(|error| AppError::internal(format!("proxy semaphore closed: {error}")))?;

    let response = state
        .http
        .get(&remote_url)
        .timeout(Duration::from_secs(45))
        .header(ACCEPT, "image/*,*/*;q=0.8")
        .send()
        .await
        .map_err(|error| AppError::not_found(format!("Remote file unavailable: {error}")))?;

    if !response.status().is_success() {
        return Err(AppError::not_found("Remote file unavailable"));
    }

    if let Some(length) = response.content_length() {
        if length > state.config.max_remote_file_proxy_bytes {
            return Err(AppError::bad_request("Remote file is too large"));
        }
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .cloned()
        .or_else(|| HeaderValue::from_str(&fallback_mime).ok())
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"));

    let limit = state.config.max_remote_file_proxy_bytes;
    let mut total = 0u64;
    let stream = response.bytes_stream().map_ok(move |chunk| {
        total = total.saturating_add(chunk.len() as u64);
        chunk
    }).map_err(|error| io::Error::new(io::ErrorKind::Other, error));

    let body = Body::from_stream(stream.take_while(move |result| {
        let allowed = result
            .as_ref()
            .map(|chunk| chunk.len() as u64 <= limit)
            .unwrap_or(true);
        async move { allowed }
    }));

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, content_type);
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));

    Ok((StatusCode::OK, headers, body).into_response())
}

async fn respond_local_file(data_dir: &Path, file: &ManagedFileRecord) -> AppResult<Response<Body>> {
    let relative_path = sanitize_relative_path(&file.relative_path)?;
    let path = data_dir.join(relative_path);
    ensure_safe_child(data_dir, &path)?;
    let opened = tokio::fs::File::open(&path)
        .await
        .map_err(|_| AppError::not_found("File not found on disk"))?;
    let metadata = opened.metadata().await.ok();
    let stream = ReaderStream::new(opened);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    let content_type = if file.mime_type.trim().is_empty() {
        mime_guess::from_path(&path).first_or(mime::APPLICATION_OCTET_STREAM).to_string()
    } else {
        file.mime_type.clone()
    };
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&content_type).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    if let Some(length) = metadata.map(|meta| meta.len()) {
        if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
            headers.insert(CONTENT_LENGTH, value);
        }
    }
    Ok((StatusCode::OK, headers, body).into_response())
}

fn truthy(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "TRUE" | "yes" | "YES"))
}

fn sanitize_relative_path(raw: &str) -> AppResult<String> {
    let path = PathBuf::from(raw.trim_start_matches('/'));
    if path.components().any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return Err(AppError::bad_request("Invalid file path"));
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.is_empty() {
        return Err(AppError::bad_request("Missing file path"));
    }
    Ok(normalized)
}

fn ensure_safe_child(root: &Path, child: &Path) -> AppResult<()> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let parent = child.parent().unwrap_or(root.as_path());
    let parent = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
    if !parent.starts_with(root) {
        return Err(AppError::bad_request("Invalid file path"));
    }
    Ok(())
}

trait IntoResponseExt {
    fn into_response(self) -> Response<Body>;
}

impl IntoResponseExt for Redirect {
    fn into_response(self) -> Response<Body> {
        axum::response::IntoResponse::into_response(self)
    }
}

impl IntoResponseExt for (StatusCode, HeaderMap, Body) {
    fn into_response(self) -> Response<Body> {
        axum::response::IntoResponse::into_response(self)
    }
}

