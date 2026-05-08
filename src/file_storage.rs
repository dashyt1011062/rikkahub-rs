use std::io;
use std::path::{Component, Path, PathBuf};

use bytes::Bytes;

use crate::db::{self, ManagedFileRecord};
use crate::error::{AppError, AppResult};
use crate::imgpile;
use crate::AppState;

pub struct StoredFile {
    pub record: ManagedFileRecord,
    pub url: String,
}

pub fn uses_imgpile_storage(storage: &str) -> bool {
    storage.trim().eq_ignore_ascii_case("imgpile")
}

pub async fn store_bytes(
    state: &AppState,
    account_id: String,
    display_name: String,
    mime_type: String,
    bytes: Bytes,
) -> AppResult<StoredFile> {
    if uses_imgpile_storage(&state.config.file_storage) && mime_type.trim().to_ascii_lowercase().starts_with("image/") {
        let upload = imgpile::upload_bytes(
            state.http.clone(),
            state.config.imgpile_key.clone(),
            bytes,
            display_name.clone(),
            mime_type.clone(),
        )
        .await?;
        let record = db::insert_remote_file(
            state.config.db_path.clone(),
            account_id,
            display_name,
            mime_type,
            upload.size_bytes,
            "imgpile".to_string(),
            upload.original_url.clone(),
            upload.page_url,
            upload.delete_url,
            upload.thumbnail_url,
        )
        .await?;
        return Ok(StoredFile {
            record,
            url: upload.original_url,
        });
    }

    let size_bytes = i64::try_from(bytes.len())
        .map_err(|_| AppError::bad_request("File too large"))?;
    let record = db::insert_local_file(
        state.config.db_path.clone(),
        account_id.clone(),
        display_name,
        mime_type,
        size_bytes,
        "local".to_string(),
    )
    .await?;

    let path = local_file_path(&state.config.data_dir, &record)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| AppError::internal(format!("failed to create upload directory: {error}")))?;
    }
    if let Err(error) = tokio::fs::write(&path, &bytes).await {
        let _ = db::delete_file_record(state.config.db_path.clone(), account_id, record.id).await;
        return Err(AppError::internal(format!("failed to write local file: {error}")));
    }

    Ok(StoredFile {
        url: record.relative_path.clone(),
        record,
    })
}

pub async fn delete_local_file_if_present(data_dir: &Path, file: &ManagedFileRecord) -> AppResult<()> {
    if file.remote_url.as_deref().is_some_and(|value| !value.trim().is_empty()) {
        return Ok(());
    }

    let path = local_file_path(data_dir, file)?;
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::internal(format!("failed to delete local file: {error}"))),
    }
}

pub async fn read_local_file_bytes(data_dir: &Path, file: &ManagedFileRecord) -> AppResult<Option<(Bytes, String)>> {
    if file.remote_url.as_deref().is_some_and(|value| !value.trim().is_empty()) {
        return Ok(None);
    }

    let path = local_file_path(data_dir, file)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => AppError::not_found("File not found on disk"),
            _ => AppError::internal(format!("failed to read local file: {error}")),
        })?;
    let mime = if file.mime_type.trim().is_empty() {
        mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string()
    } else {
        file.mime_type.clone()
    };
    Ok(Some((Bytes::from(bytes), mime)))
}

fn local_file_path(data_dir: &Path, file: &ManagedFileRecord) -> AppResult<PathBuf> {
    let relative_path = sanitize_relative_path(&file.relative_path)?;
    let path = data_dir.join(relative_path);
    ensure_safe_child(data_dir, &path)?;
    Ok(path)
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
