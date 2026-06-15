use axum::{
    body::Body,
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::{error::AppError, state::AppState, storage};

const ADMIN_HTML: &str = include_str!("../../admin/index.html");

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "uptime_secs": state.started_at.elapsed().as_secs()
    }))
}

pub async fn serve_admin() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        ADMIN_HTML,
    )
}

pub async fn get_instances(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    Ok(Json(instances))
}

pub async fn get_news(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let news = storage::read_news(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    Ok(Json(news))
}

pub async fn get_manifest(
    State(state): State<AppState>,
    Path(game_dir): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let manifest = storage::read_manifest(&state.data_dir, &game_dir)
        .await
        .map_err(AppError::Storage)?;
    Ok(Json(manifest))
}

pub async fn get_file(
    State(state): State<AppState>,
    Path((game_dir, filename)): Path<(String, String)>,
) -> Result<Response, AppError> {
    if game_dir.contains("..") || filename.contains("..") {
        return Err(AppError::BadRequest("invalid path".to_string()));
    }

    let path = state.files_dir(&game_dir).join(&filename);
    let file = File::open(&path)
        .await
        .map_err(|_| AppError::NotFound(format!("{filename} not found")))?;

    let file_size = file
        .metadata()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
        .len();

    let content_type = if filename.ends_with(".jar") {
        "application/java-archive"
    } else {
        "application/zip"
    };

    let stream = ReaderStream::new(file);
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, file_size)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from_stream(stream))
        .unwrap())
}
