use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::extract::{Extension, Multipart, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::Json;
use serde_json::{json, Value};
use tokio::task;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::auth::AccountId;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::AppState;

const BACKUP_MAX_BYTES: usize = 512 * 1024 * 1024;

pub async fn export_backup(
    Extension(_account): Extension<AccountId>,
    State(state): State<AppState>,
) -> AppResult<Response<Body>> {
    let data_dir = state.config.data_dir.clone();
    let db_path = state.config.db_path.clone();
    let bytes = task::spawn_blocking(move || build_backup_zip(&data_dir, &db_path))
        .await
        .map_err(|error| AppError::internal(format!("backup task failed: {error}")))??;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let file_name = format!("rikkahub-rust-backup-{}.zip", db::now_iso().replace([':', '.'], "-"));
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment; filename=\"rikkahub-backup.zip\"")),
    );

    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response())
}

pub async fn import_backup(
    Extension(_account): Extension<AccountId>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    let mut archive = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::bad_request(format!("Invalid multipart upload: {error}")))?
    {
        if field.file_name().is_none() {
            continue;
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|error| AppError::bad_request(format!("backup read failed: {error}")))?;
        if bytes.len() > BACKUP_MAX_BYTES {
            return Err(AppError::bad_request("Backup file is too large"));
        }
        archive = Some(bytes.to_vec());
        break;
    }

    let Some(bytes) = archive else {
        return Err(AppError::bad_request("No backup file uploaded"));
    };
    let data_dir = state.config.data_dir.clone();
    let db_path = state.config.db_path.clone();
    let report = task::spawn_blocking(move || apply_backup_zip(&data_dir, &db_path, bytes))
        .await
        .map_err(|error| AppError::internal(format!("backup import task failed: {error}")))??;

    Ok(Json(report))
}

fn build_backup_zip(data_dir: &Path, db_path: &Path) -> AppResult<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    add_file_if_exists(&mut writer, db_path, "rikka_hub.db", options)?;
    add_file_if_exists(&mut writer, &data_dir.join("settings.json"), "settings.json", options)?;
    add_tree_if_exists(&mut writer, &data_dir.join("accounts"), data_dir, options)?;
    add_tree_if_exists(&mut writer, &data_dir.join("upload"), data_dir, options)?;

    let cursor = writer
        .finish()
        .map_err(|error| AppError::internal(format!("zip finish failed: {error}")))?;
    Ok(cursor.into_inner())
}

fn add_file_if_exists<W: Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    path: &Path,
    name: &str,
    options: SimpleFileOptions,
) -> AppResult<()> {
    if !path.is_file() {
        return Ok(());
    }
    writer
        .start_file(name, options)
        .map_err(|error| AppError::internal(format!("zip write failed: {error}")))?;
    let bytes = fs::read(path)?;
    writer.write_all(&bytes)?;
    Ok(())
}

fn add_tree_if_exists<W: Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    root: &Path,
    data_dir: &Path,
    options: SimpleFileOptions,
) -> AppResult<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            add_tree_if_exists(writer, &path, data_dir, options)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(data_dir)
                .map_err(|error| AppError::internal(format!("backup path failed: {error}")))?
                .to_string_lossy()
                .replace('\\', "/");
            add_file_if_exists(writer, &path, &relative, options)?;
        }
    }
    Ok(())
}

fn apply_backup_zip(data_dir: &Path, db_path: &Path, bytes: Vec<u8>) -> AppResult<Value> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::bad_request(format!("Invalid backup zip: {error}")))?;
    let backup_suffix = db::now_iso().replace([':', '.'], "-");
    let mut imported = Vec::new();
    let mut backups = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| AppError::bad_request(format!("Invalid backup entry: {error}")))?;
        if file.is_dir() {
            continue;
        }
        let Some(relative) = allowed_backup_path(file.name())? else {
            continue;
        };

        let target = if relative == PathBuf::from("rikka_hub.db") {
            db_path.to_path_buf()
        } else {
            data_dir.join(&relative)
        };
        if target.exists() {
            let backup_path = backup_path_for(&target, &backup_suffix);
            if let Some(parent) = backup_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&target, &backup_path)?;
            backups.push(backup_path.to_string_lossy().to_string());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&target)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        out.write_all(&buf)?;
        imported.push(relative.to_string_lossy().replace('\\', "/"));
    }

    Ok(json!({
        "success": true,
        "imported": imported,
        "backups": backups,
    }))
}

fn allowed_backup_path(raw: &str) -> AppResult<Option<PathBuf>> {
    let normalized = raw.trim_start_matches('/').replace('\\', "/");
    if normalized.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(&normalized);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(AppError::bad_request("Invalid backup path"));
    }
    let allowed = normalized == "rikka_hub.db"
        || normalized == "settings.json"
        || (normalized.starts_with("accounts/") && normalized.ends_with("/settings.json"))
        || normalized.starts_with("upload/");
    Ok(allowed.then_some(path))
}

fn backup_path_for(target: &Path, suffix: &str) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("backup");
    target.with_file_name(format!("{file_name}.bak-before-rust-import-{suffix}"))
}

trait IntoResponseExt {
    fn into_response(self) -> Response<Body>;
}

impl IntoResponseExt for (StatusCode, HeaderMap, Body) {
    fn into_response(self) -> Response<Body> {
        axum::response::IntoResponse::into_response(self)
    }
}
