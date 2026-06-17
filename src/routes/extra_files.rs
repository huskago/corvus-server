use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use tokio_util::io::ReaderStream;

use crate::{
    error::AppError,
    models::{ExtraFileEntry, MkdirRequest, TreeResponse},
    routes::AuthUser,
    state::AppState,
    storage,
};

pub(crate) fn validate_extra_path(path: &str) -> Result<String, AppError> {
    if path.is_empty() {
        return Ok(String::new());
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > 8 {
        return Err(AppError::BadRequest(
            "path too deep (max 8 segments)".into(),
        ));
    }
    let mut safe = Vec::new();
    for seg in &segments {
        if seg.is_empty() || *seg == "." || *seg == ".." {
            return Err(AppError::BadRequest(format!(
                "invalid path segment: '{seg}'"
            )));
        }
        let s = sanitize_segment(seg);
        if s.is_empty() {
            return Err(AppError::BadRequest(format!(
                "path segment '{seg}' contains no valid characters"
            )));
        }
        safe.push(s);
    }
    Ok(safe.join("/"))
}

pub(crate) fn content_type_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => "application/json",
        Some("toml") | Some("properties") | Some("cfg") | Some("txt") | Some("md") => {
            "text/plain; charset=utf-8"
        }
        Some("js") | Some("mjs") => "text/javascript",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("zip") => "application/zip",
        Some("jar") => "application/java-archive",
        _ => "application/octet-stream",
    }
}

fn sanitize_segment(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}

pub async fn get_extra_file(
    State(state): State<AppState>,
    Path((id, file_path)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let safe_path = validate_extra_path(&file_path)?;
    let abs_path = state.extra_files_dir(&id).join(&safe_path);

    let file = tokio::fs::File::open(&abs_path)
        .await
        .map_err(|_| AppError::NotFound(format!("{file_path} not found")))?;

    let file_size = file
        .metadata()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
        .len();

    let content_type = content_type_for(&abs_path);
    let filename = abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

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

#[derive(serde::Deserialize)]
pub struct TreeQuery {
    #[serde(default)]
    pub dir: String,
}

pub async fn tree(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<TreeQuery>,
) -> Result<Json<TreeResponse>, AppError> {
    let safe_dir = validate_extra_path(&q.dir)?;
    let base = state.extra_files_dir(&id);
    let target = if safe_dir.is_empty() {
        base.clone()
    } else {
        base.join(&safe_dir)
    };

    if !target.exists() {
        return Ok(Json(TreeResponse { dirs: vec![], files: vec![] }));
    }

    let index = storage::read_extra_files_index(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;

    let public_url = state
        .config
        .read()
        .await
        .server
        .public_url
        .trim_end_matches('/')
        .to_string();

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let mut entries = tokio::fs::read_dir(&target)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = entry
            .file_type()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        if ft.is_dir() {
            dirs.push(name);
        } else {
            let rel_path = if safe_dir.is_empty() {
                name.clone()
            } else {
                format!("{safe_dir}/{name}")
            };
            let (sha1, size) = match index.get(&rel_path) {
                Some(m) => (m.sha1.clone(), m.size),
                None => {
                    let disk_size = entry
                        .metadata()
                        .await
                        .map_err(|e| AppError::Storage(e.to_string()))?
                        .len();
                    (String::new(), disk_size)
                }
            };
            let url = format!("{public_url}/extra/{id}/{rel_path}");
            files.push(ExtraFileEntry { path: rel_path, name, sha1, size, url });
        }
    }

    dirs.sort();
    files.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(TreeResponse { dirs, files }))
}

pub async fn mkdir(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<MkdirRequest>,
) -> Result<StatusCode, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    if !instances.iter().any(|i| i.game_dir_name == id) {
        return Err(AppError::NotFound(format!("instance '{id}' not found")));
    }

    let safe_path = validate_extra_path(&body.path)?;
    if safe_path.is_empty() {
        return Err(AppError::BadRequest("path is required".into()));
    }

    let dir = state.extra_files_dir(&id).join(&safe_path);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;

    Ok(StatusCode::CREATED)
}
