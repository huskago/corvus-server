use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    Json,
};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use crate::{
    error::AppError,
    models::{FileListEntry, FileMeta, InstanceManifest, UploadedFile},
    routes::AuthUser,
    state::AppState,
    storage,
};

pub async fn get_manifest(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<InstanceManifest>, AppError> {
    storage::read_manifest(&state.data_dir, &id)
        .await
        .map(Json)
        .map_err(AppError::Storage)
}

pub async fn put_manifest(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(manifest): Json<InstanceManifest>,
) -> Result<Json<InstanceManifest>, AppError> {
    storage::write_manifest(&state.data_dir, &id, &manifest)
        .await
        .map_err(AppError::Storage)?;
    Ok(Json(manifest))
}

pub async fn upload(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<Vec<UploadedFile>>, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    if !instances.iter().any(|i| i.game_dir_name == id) {
        return Err(AppError::NotFound(format!("instance '{id}' not found")));
    }

    let files_dir = state.files_dir(&id);
    tokio::fs::create_dir_all(&files_dir)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;

    let (max_bytes, public_url) = {
        let cfg = state.config.read().await;
        (
            cfg.server.max_upload_mb as usize * 1024 * 1024,
            cfg.server.public_url.trim_end_matches('/').to_string(),
        )
    };

    let mut uploaded: Vec<UploadedFile> = Vec::new();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let filename = field.file_name().unwrap_or("unknown").to_string();
        let safe = sanitize(&filename);

        if safe.is_empty() {
            return Err(AppError::BadRequest("invalid filename".to_string()));
        }
        if !safe.ends_with(".jar") && !safe.ends_with(".zip") {
            return Err(AppError::BadRequest(format!(
                "{safe}: only .jar and .zip files accepted"
            )));
        }

        let dest = files_dir.join(&safe);
        let mut file = tokio::fs::File::create(&dest)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        let mut hasher = Sha1::new();
        let mut written = 0usize;

        loop {
            let chunk = field
                .chunk()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            let Some(chunk) = chunk else { break };

            written += chunk.len();
            if written > max_bytes {
                drop(file);
                tokio::fs::remove_file(&dest).await.ok();
                return Err(AppError::BadRequest(format!(
                    "{safe}: exceeds {} MB limit",
                    max_bytes / (1024 * 1024)
                )));
            }

            hasher.update(&chunk);
            if let Err(e) = file.write_all(&chunk).await {
                drop(file);
                tokio::fs::remove_file(dest).await.ok();
                return Err(AppError::Storage(e.to_string()));
            }
        }

        file.flush()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
        drop(file);

        let sha1 = hex::encode(hasher.finalize());
        let size = written as u64;

        {
            let _guard = state.write_lock.lock().await;
            storage::upsert_file_meta(
                &state.data_dir,
                &id,
                &safe,
                FileMeta { sha1: sha1.clone(), size },
            )
            .await
            .map_err(AppError::Storage)?;
        }

        let download_url = format!("{public_url}/files/{id}/{safe}");
        uploaded.push(UploadedFile { name: safe, sha1, size, download_url });
    }

    Ok(Json(uploaded))
}

pub async fn delete_file(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((id, filename)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(AppError::BadRequest("invalid filename".to_string()));
    }

    let _guard = state.write_lock.lock().await;
    let path = state.files_dir(&id).join(&filename);
    tokio::fs::remove_file(&path)
        .await
        .map_err(|_| AppError::NotFound(format!("{filename} not found")))?;

    storage::remove_file_meta(&state.data_dir, &id, &filename)
        .await
        .map_err(AppError::Storage)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_files(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<FileListEntry>>, AppError> {
    let files_dir = state.files_dir(&id);

    if !files_dir.exists() {
        return Ok(Json(Vec::new()));
    }

    let public_url = state
        .config
        .read()
        .await
        .server
        .public_url
        .trim_end_matches('/')
        .to_string();

    let index = storage::read_files_index(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;

    let mut entries = tokio::fs::read_dir(&files_dir)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;

    let mut result = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let disk_size = entry
            .metadata()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?
            .len();

        let (sha1, size) = match index.get(&name) {
            Some(m) => (m.sha1.clone(), m.size),
            None => (String::new(), disk_size),
        };

        let url = format!("{public_url}/files/{id}/{name}");
        result.push(FileListEntry { name, sha1, size, url });
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(result))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}
