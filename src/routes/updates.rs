use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::{error::AppError, models::LauncherPlatformEntry, routes::AuthUser, state::AppState, storage};

fn is_valid_platform(p: &str) -> bool {
    matches!(p, "windows-x86_64" | "linux-x86_64" | "darwin-aarch64" | "darwin-x86_64")
}

pub async fn latest_json(State(state): State<AppState>) -> Result<Response, AppError> {
    let release = storage::read_launcher_release(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    if release.version.is_empty() || release.platforms.is_empty() {
        return Err(AppError::NotFound("no launcher release configured".into()));
    }

    let platforms: serde_json::Map<String, serde_json::Value> = release
        .platforms
        .iter()
        .map(|(k, v)| (k.clone(), json!({"url": v.url, "signature": v.signature})))
        .collect();

    let payload = json!({
        "version": release.version,
        "notes": release.notes,
        "pub_date": release.pub_date,
        "platforms": platforms,
    });

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap())
}

pub async fn get_update_file(
    State(state): State<AppState>,
    Path((platform, filename)): Path<(String, String)>,
) -> Result<Response, AppError> {
    if !is_valid_platform(&platform) {
        return Err(AppError::NotFound("invalid platform".into()));
    }
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(AppError::BadRequest("invalid filename".into()));
    }

    let path = state.updates_dir(&platform).join(&filename);
    let file = File::open(&path)
        .await
        .map_err(|_| AppError::NotFound(format!("{filename} not found")))?;

    let file_size = file
        .metadata()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
        .len();

    let stream = ReaderStream::new(file);
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, file_size)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from_stream(stream))
        .unwrap())
}

pub async fn get_release(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<crate::models::LauncherRelease>, AppError> {
    storage::read_launcher_release(&state.data_dir)
        .await
        .map(Json)
        .map_err(AppError::Storage)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseMeta {
    pub version: String,
    pub notes: String,
    pub pub_date: String,
}

pub async fn put_release_meta(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(body): Json<ReleaseMeta>,
) -> Result<StatusCode, AppError> {
    let _guard = state.write_lock.lock().await;
    let mut release = storage::read_launcher_release(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    release.version = body.version;
    release.notes = body.notes;
    release.pub_date = body.pub_date;
    storage::write_launcher_release(&state.data_dir, &release)
        .await
        .map_err(AppError::Storage)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn upload_platform(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(platform): Path<String>,
    mut multipart: Multipart,
) -> Result<StatusCode, AppError> {
    if !is_valid_platform(&platform) {
        return Err(AppError::BadRequest(format!("invalid platform: {platform}")));
    }

    let updates_dir = state.updates_dir(&platform);
    tokio::fs::create_dir_all(&updates_dir)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;

    let mut filename = String::new();
    let mut signature = String::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                filename = field
                    .file_name()
                    .unwrap_or("binary")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || matches!(*c, '.' | '_' | '-'))
                    .collect();
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                // clear existing files for this platform before writing
                if let Ok(mut rd) = tokio::fs::read_dir(&updates_dir).await {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        tokio::fs::remove_file(entry.path()).await.ok();
                    }
                }
                tokio::fs::write(updates_dir.join(&filename), data)
                    .await
                    .map_err(|e| AppError::Storage(e.to_string()))?;
            }
            "signature" => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                signature = String::from_utf8(data.to_vec())
                    .map_err(|_| AppError::BadRequest("invalid signature encoding".into()))?;
            }
            _ => {}
        }
    }

    if filename.is_empty() {
        return Err(AppError::BadRequest("no file uploaded".into()));
    }

    let public_url = state.config.read().await.server.public_url.clone();
    let url = format!("{public_url}/updates/{platform}/{filename}");

    let _guard = state.write_lock.lock().await;
    let mut release = storage::read_launcher_release(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    release.platforms.insert(
        platform,
        LauncherPlatformEntry {
            url,
            signature: signature.trim().to_string(),
        },
    );
    storage::write_launcher_release(&state.data_dir, &release)
        .await
        .map_err(AppError::Storage)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_platform(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(platform): Path<String>,
) -> Result<StatusCode, AppError> {
    if !is_valid_platform(&platform) {
        return Err(AppError::BadRequest(format!("invalid platform: {platform}")));
    }
    let _guard = state.write_lock.lock().await;
    let mut release = storage::read_launcher_release(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    release.platforms.remove(&platform);
    storage::write_launcher_release(&state.data_dir, &release)
        .await
        .map_err(AppError::Storage)?;

    tokio::fs::remove_dir_all(state.updates_dir(&platform))
        .await
        .ok();

    Ok(StatusCode::NO_CONTENT)
}
