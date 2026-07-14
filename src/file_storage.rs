use std::io;
use std::path::{Component, Path, PathBuf};

use bytes::Bytes;

use crate::db::{self, ManagedFileRecord};
use crate::error::{AppError, AppResult};
use crate::imghippo;
use crate::imgpile;
use crate::AppState;

pub struct StoredFile {
    pub record: ManagedFileRecord,
    pub url: String,
}

pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    fn non_zero(width: u32, height: u32) -> Option<(u32, u32)> {
        (width > 0 && height > 0).then_some((width, height))
    }

    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return non_zero(
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        );
    }

    if bytes.len() >= 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return non_zero(
            u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
            u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
        );
    }

    if bytes.len() >= 26 && bytes.starts_with(b"BM") {
        let width = i32::from_le_bytes(bytes[18..22].try_into().ok()?).unsigned_abs();
        let height = i32::from_le_bytes(bytes[22..26].try_into().ok()?).unsigned_abs();
        return non_zero(width, height);
    }

    if bytes.len() >= 30 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        let chunk = &bytes[12..16];
        if chunk == b"VP8X" {
            let width = 1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]);
            let height = 1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]);
            return non_zero(width, height);
        }
        if chunk == b"VP8 "
            && bytes.len() >= 30
            && bytes[23] == 0x9d
            && bytes[24] == 0x01
            && bytes[25] == 0x2a
        {
            let width = u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff;
            let height = u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff;
            return non_zero(width as u32, height as u32);
        }
        if chunk == b"VP8L" && bytes.len() >= 25 && bytes[20] == 0x2f {
            let b1 = bytes[21] as u32;
            let b2 = bytes[22] as u32;
            let b3 = bytes[23] as u32;
            let b4 = bytes[24] as u32;
            let width = 1 + b1 + ((b2 & 0x3f) << 8);
            let height = 1 + (b2 >> 6) + (b3 << 2) + ((b4 & 0x0f) << 10);
            return non_zero(width, height);
        }
    }

    if bytes.len() >= 4 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        let mut index = 2usize;
        while index + 3 < bytes.len() {
            if bytes[index] != 0xff {
                index += 1;
                continue;
            }
            while index < bytes.len() && bytes[index] == 0xff {
                index += 1;
            }
            if index >= bytes.len() {
                break;
            }
            let marker = bytes[index];
            index += 1;
            if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
                continue;
            }
            if index + 2 > bytes.len() {
                break;
            }
            let segment_length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;
            if segment_length < 2 || index + segment_length > bytes.len() {
                break;
            }
            let is_start_of_frame = matches!(
                marker,
                0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
            );
            if is_start_of_frame && segment_length >= 7 {
                let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as u32;
                let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
                return non_zero(width, height);
            }
            index += segment_length;
        }
    }

    None
}

pub fn uses_imgpile_storage(storage: &str) -> bool {
    storage.trim().eq_ignore_ascii_case("imgpile")
}

pub fn uses_imghippo_storage(storage: &str) -> bool {
    storage.trim().eq_ignore_ascii_case("imghippo")
}

pub async fn store_bytes(
    state: &AppState,
    account_id: String,
    display_name: String,
    mime_type: String,
    bytes: Bytes,
) -> AppResult<StoredFile> {
    let is_image = mime_type.trim().to_ascii_lowercase().starts_with("image/");
    if uses_imghippo_storage(&state.config.file_storage) && is_image {
        let api_key = state
            .next_imghippo_key()
            .ok_or_else(|| AppError::bad_request("Imghippo storage is not configured"))?;
        let upload = imghippo::upload_bytes(
            state.http.clone(),
            api_key,
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
            "imghippo".to_string(),
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

    if uses_imgpile_storage(&state.config.file_storage) && is_image {
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
