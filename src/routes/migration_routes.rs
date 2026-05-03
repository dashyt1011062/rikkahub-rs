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
use crate::config::DEFAULT_WEB_ACCOUNT_ID;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::AppState;

const BACKUP_MAX_BYTES: usize = 512 * 1024 * 1024;

pub async fn export_backup(
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
) -> AppResult<Response<Body>> {
    ensure_primary_account(&account.0)?;
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
    Extension(account): Extension<AccountId>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    ensure_primary_account(&account.0)?;
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
    if report.get("success").and_then(Value::as_bool) == Some(true) {
        db::rebuild_message_search_index(state.config.db_path.clone()).await?;
    }

    Ok(Json(report))
}

fn build_backup_zip(data_dir: &Path, db_path: &Path) -> AppResult<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    add_required_file(&mut writer, &data_dir.join("settings.json"), "settings.json", options)?;
    add_required_file(&mut writer, db_path, "rikka_hub.db", options)?;
    add_file_if_exists(&mut writer, &data_dir.join("rikka_hub.db-wal"), "rikka_hub-wal", options)?;
    add_file_if_exists(&mut writer, &data_dir.join("rikka_hub.db-shm"), "rikka_hub-shm", options)?;
    add_tree_if_exists(&mut writer, &data_dir.join("accounts"), data_dir, options)?;
    add_tree_if_exists(&mut writer, &data_dir.join("upload"), data_dir, options)?;

    let cursor = writer
        .finish()
        .map_err(|error| AppError::internal(format!("zip finish failed: {error}")))?;
    Ok(cursor.into_inner())
}

fn add_required_file<W: Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    path: &Path,
    name: &str,
    options: SimpleFileOptions,
) -> AppResult<()> {
    if !path.is_file() {
        return Err(AppError::internal(format!("Required backup file missing: {name}")));
    }
    add_file_if_exists(writer, path, name, options)
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
    let suffix = db::now_millis().to_string();
    let import_root = data_dir.join(format!("import-rust-{suffix}"));
    let backup_root = data_dir.join("backup").join(format!("backup-{suffix}"));
    fs::create_dir_all(&import_root)?;

    let result = (|| -> AppResult<Value> {
        unzip_backup(bytes, &import_root)?;
        let settings_src = import_root.join("settings.json");
        let db_src = import_root.join("rikka_hub.db");
        if !settings_src.is_file() {
            return Ok(import_report(false, false, "Invalid backup: settings.json is missing", 0, 0, 0));
        }
        if !db_src.is_file() {
            return Ok(import_report(false, false, "Invalid backup: rikka_hub.db is missing", 0, 0, 0));
        }

        backup_current_data(data_dir, db_path, &backup_root)?;
        let apply_result = (|| -> AppResult<()> {
            fs::create_dir_all(data_dir)?;
            fs::copy(&settings_src, data_dir.join("settings.json"))?;
            fs::copy(&db_src, db_path)?;
            copy_or_delete(import_root.join("rikka_hub-wal"), data_dir.join("rikka_hub.db-wal"))?;
            copy_or_delete(import_root.join("rikka_hub-shm"), data_dir.join("rikka_hub.db-shm"))?;
            if import_root.join("upload").is_dir() {
                delete_directory(&data_dir.join("upload"))?;
                copy_directory(&import_root.join("upload"), &data_dir.join("upload"))?;
            }
            if import_root.join("accounts").is_dir() {
                delete_directory(&data_dir.join("accounts"))?;
                copy_directory(&import_root.join("accounts"), &data_dir.join("accounts"))?;
            }
            Ok(())
        })();

        if let Err(error) = apply_result {
            restore_from_backup(data_dir, db_path, &backup_root)?;
            return Ok(import_report(
                false,
                true,
                &format!("Import failed and rolled back: {}", error.message),
                0,
                0,
                0,
            ));
        }

        let (conversations, nodes, files) = count_imported(db_path)?;
        Ok(import_report(true, false, "Import completed", conversations, nodes, files))
    })();

    let _ = delete_directory(&import_root);
    result
}

fn unzip_backup(bytes: Vec<u8>, target_dir: &Path) -> AppResult<()> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::bad_request(format!("Invalid backup zip: {error}")))?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| AppError::bad_request(format!("Invalid backup entry: {error}")))?;
        let Some(relative) = allowed_backup_path(file.name())? else {
            continue;
        };
        let target = target_dir.join(relative);
        if file.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&target)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        out.write_all(&buf)?;
    }
    Ok(())
}

fn backup_current_data(data_dir: &Path, db_path: &Path, backup_root: &Path) -> AppResult<()> {
    fs::create_dir_all(backup_root)?;
    copy_if_exists(data_dir.join("settings.json"), backup_root.join("settings.json"))?;
    copy_if_exists(db_path, backup_root.join("rikka_hub.db"))?;
    copy_if_exists(data_dir.join("rikka_hub.db-wal"), backup_root.join("rikka_hub-wal"))?;
    copy_if_exists(data_dir.join("rikka_hub.db-shm"), backup_root.join("rikka_hub-shm"))?;
    if data_dir.join("upload").is_dir() {
        copy_directory(&data_dir.join("upload"), &backup_root.join("upload"))?;
    }
    if data_dir.join("accounts").is_dir() {
        copy_directory(&data_dir.join("accounts"), &backup_root.join("accounts"))?;
    }
    Ok(())
}

fn restore_from_backup(data_dir: &Path, db_path: &Path, backup_root: &Path) -> AppResult<()> {
    copy_if_exists(backup_root.join("settings.json"), data_dir.join("settings.json"))?;
    copy_if_exists(backup_root.join("rikka_hub.db"), db_path)?;
    copy_or_delete(backup_root.join("rikka_hub-wal"), data_dir.join("rikka_hub.db-wal"))?;
    copy_or_delete(backup_root.join("rikka_hub-shm"), data_dir.join("rikka_hub.db-shm"))?;
    if backup_root.join("upload").is_dir() {
        delete_directory(&data_dir.join("upload"))?;
        copy_directory(&backup_root.join("upload"), &data_dir.join("upload"))?;
    }
    if backup_root.join("accounts").is_dir() {
        delete_directory(&data_dir.join("accounts"))?;
        copy_directory(&backup_root.join("accounts"), &data_dir.join("accounts"))?;
    }
    Ok(())
}

fn copy_if_exists(from: impl AsRef<Path>, to: impl AsRef<Path>) -> AppResult<()> {
    let from = from.as_ref();
    if !from.exists() {
        return Ok(());
    }
    if let Some(parent) = to.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to)?;
    Ok(())
}

fn copy_or_delete(from: impl AsRef<Path>, to: impl AsRef<Path>) -> AppResult<()> {
    if from.as_ref().exists() {
        copy_if_exists(from, to)
    } else {
        let _ = fs::remove_file(to);
        Ok(())
    }
}

fn copy_directory(from: &Path, to: &Path) -> AppResult<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_directory(&source, &target)?;
        } else if source.is_file() {
            copy_if_exists(&source, &target)?;
        }
    }
    Ok(())
}

fn delete_directory(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            delete_directory(&child)?;
        } else {
            fs::remove_file(&child)?;
        }
    }
    fs::remove_dir(path)?;
    Ok(())
}

fn count_imported(db_path: &Path) -> AppResult<(i64, i64, i64)> {
    let conn = rusqlite::Connection::open(db_path)?;
    let conversations = conn.query_row("SELECT COUNT(*) FROM conversationentity", [], |row| row.get(0))?;
    let nodes = conn.query_row("SELECT COUNT(*) FROM message_node", [], |row| row.get(0))?;
    let files = conn.query_row("SELECT COUNT(*) FROM managed_files", [], |row| row.get(0)).unwrap_or(0);
    Ok((conversations, nodes, files))
}

fn import_report(success: bool, rolled_back: bool, message: &str, conversations: i64, nodes: i64, files: i64) -> Value {
    json!({
        "success": success,
        "rolledBack": rolled_back,
        "message": message,
        "importedConversations": conversations,
        "importedMessageNodes": nodes,
        "importedFiles": files,
    })
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
        || normalized == "rikka_hub-wal"
        || normalized == "rikka_hub-shm"
        || normalized == "settings.json"
        || (normalized.starts_with("accounts/") && normalized.ends_with("/settings.json"))
        || normalized.starts_with("upload/");
    Ok(allowed.then_some(path))
}

fn ensure_primary_account(account_id: &str) -> AppResult<()> {
    if account_id != DEFAULT_WEB_ACCOUNT_ID {
        return Err(AppError::bad_request("Migration import/export is only available to the primary account"));
    }
    Ok(())
}

trait IntoResponseExt {
    fn into_response(self) -> Response<Body>;
}

impl IntoResponseExt for (StatusCode, HeaderMap, Body) {
    fn into_response(self) -> Response<Body> {
        axum::response::IntoResponse::into_response(self)
    }
}
