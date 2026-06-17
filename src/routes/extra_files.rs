use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use sha1::{Digest, Sha1};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use std::collections::HashSet;

use crate::{
    error::AppError,
    models::{
        ExtraFile, ExtraFileEntry, FileMeta, IntegrateRequest, ManifestFile, MkdirRequest,
        PhantomExtraFile, PhantomFile, RehashResult, ScanResult, TreeResponse,
    },
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

pub(crate) async fn hash_file(path: &std::path::Path) -> Result<(String, u64), AppError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| AppError::Storage(format!("open {}: {e}", path.display())))?;
    let size = file
        .metadata()
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
        .len();
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok((hex::encode(hasher.finalize()), size))
}

async fn scan_extra_files(
    base: &std::path::Path,
    known: &HashSet<String>,
) -> Result<Vec<PhantomExtraFile>, AppError> {
    let mut result = Vec::new();
    let mut dirs_to_visit = vec![base.to_path_buf()];
    while let Some(dir) = dirs_to_visit.pop() {
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
        while let Some(e) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?
        {
            let ft = e
                .file_type()
                .await
                .map_err(|e| AppError::Storage(e.to_string()))?;
            let path = e.path();
            if ft.is_dir() {
                dirs_to_visit.push(path);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                // Skip paths that wouldn't survive validate_extra_path (unsanitizable segments)
                match validate_extra_path(&rel) {
                    Ok(safe) if safe == rel => {}
                    _ => continue,
                }
                if !known.contains(&rel) {
                    let size = e
                        .metadata()
                        .await
                        .map_err(|e| AppError::Storage(e.to_string()))?
                        .len();
                    result.push(PhantomExtraFile { path: rel, size });
                }
            }
        }
    }
    Ok(result)
}

pub async fn scan(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ScanResult>, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    if !instances.iter().any(|i| i.game_dir_name == id) {
        return Err(AppError::NotFound(format!("instance '{id}' not found")));
    }

    let _guard = state.write_lock.lock().await;

    let manifest = storage::read_manifest(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;

    let known_names: HashSet<String> = manifest
        .mods
        .iter()
        .chain(manifest.resource_packs.iter())
        .chain(manifest.shaders.iter())
        .map(|f| f.name.clone())
        .collect();

    let files_dir = state.files_dir(&id);
    let mut phantom_files: Vec<PhantomFile> = Vec::new();
    if files_dir.exists() {
        let mut entries = tokio::fs::read_dir(&files_dir)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
        while let Some(e) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?
        {
            let ft = e
                .file_type()
                .await
                .map_err(|e| AppError::Storage(e.to_string()))?;
            if !ft.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if sanitize_segment(&name) != name {
                continue; // name contains chars that can't be integrated, skip
            }
            if !known_names.contains(&name) {
                let size = e
                    .metadata()
                    .await
                    .map_err(|e| AppError::Storage(e.to_string()))?
                    .len();
                phantom_files.push(PhantomFile { name, size });
            }
        }
        phantom_files.sort_by(|a, b| a.name.cmp(&b.name));
    }

    let extra_index = storage::read_extra_files_index(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;
    let known_extra: HashSet<String> = extra_index.keys().cloned().collect();

    let extra_dir = state.extra_files_dir(&id);
    let mut phantom_extra: Vec<PhantomExtraFile> = Vec::new();
    if extra_dir.exists() {
        phantom_extra = scan_extra_files(&extra_dir, &known_extra).await?;
        phantom_extra.sort_by(|a, b| a.path.cmp(&b.path));
    }

    Ok(Json(ScanResult {
        files: phantom_files,
        extra_files: phantom_extra,
    }))
}

pub async fn get_extra_file(
    State(state): State<AppState>,
    Path((id, file_path)): Path<(String, String)>,
) -> Result<Response, AppError> {
    if id.contains("..") {
        return Err(AppError::BadRequest("invalid instance id".into()));
    }
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

async fn cleanup_empty_parents(base: &std::path::Path, rel_path: &std::path::Path) {
    let mut current = match rel_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => base.join(p),
        _ => return,
    };

    loop {
        if current == base {
            break;
        }
        let mut entries = match tokio::fs::read_dir(&current).await {
            Ok(e) => e,
            Err(_) => break,
        };
        let has_entries = entries.next_entry().await.ok().flatten().is_some();
        if has_entries {
            break;
        }
        if tokio::fs::remove_dir(&current).await.is_err() {
            break;
        }
        current = match current.parent() {
            Some(p) => p.to_path_buf(),
            None => break,
        };
    }
}

#[derive(serde::Deserialize)]
pub struct UploadQuery {
    #[serde(default)]
    pub dir: String,
}

pub async fn upload(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<UploadQuery>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<Vec<ExtraFile>>, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    if !instances.iter().any(|i| i.game_dir_name == id) {
        return Err(AppError::NotFound(format!("instance '{id}' not found")));
    }

    let safe_dir = validate_extra_path(&q.dir)?;
    let base = state.extra_files_dir(&id);
    let target_dir = if safe_dir.is_empty() {
        base.clone()
    } else {
        base.join(&safe_dir)
    };
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?;

    let (max_bytes, public_url) = {
        let cfg = state.config.read().await;
        (
            cfg.server.max_upload_mb as usize * 1024 * 1024,
            cfg.server.public_url.trim_end_matches('/').to_string(),
        )
    };

    let mut uploaded: Vec<ExtraFile> = Vec::new();
    let mut expected_sha1: Option<String> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        if field.file_name().is_none() {
            if field.name() == Some("sha1") {
                let val = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if val.len() == 40 && val.chars().all(|c| c.is_ascii_hexdigit()) {
                    expected_sha1 = Some(val);
                }
            }
            continue;
        }

        let filename = field.file_name().unwrap_or("unknown").to_string();
        let safe_name = sanitize_segment(&filename);
        if safe_name.is_empty() {
            return Err(AppError::BadRequest("invalid filename".into()));
        }
        if safe_name.len() > 255 {
            return Err(AppError::BadRequest(format!(
                "{safe_name}: filename too long"
            )));
        }

        let rel_path = if safe_dir.is_empty() {
            safe_name.clone()
        } else {
            format!("{safe_dir}/{safe_name}")
        };

        let dest = target_dir.join(&safe_name);
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
                    "{safe_name}: exceeds {} MB limit",
                    max_bytes / (1024 * 1024)
                )));
            }

            hasher.update(&chunk);
            if let Err(e) = file.write_all(&chunk).await {
                drop(file);
                tokio::fs::remove_file(&dest).await.ok();
                return Err(AppError::Storage(e.to_string()));
            }
        }

        file.flush()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
        drop(file);

        let sha1 = hex::encode(hasher.finalize());
        let size = written as u64;

        if let Some(expected) = expected_sha1.take() {
            if sha1 != expected {
                tokio::fs::remove_file(&dest).await.ok();
                return Err(AppError::BadRequest(format!(
                    "{safe_name}: integrity check failed (SHA1 mismatch)"
                )));
            }
        }

        let download_url = format!("{public_url}/extra/{id}/{rel_path}");

        {
            let _guard = state.write_lock.lock().await;
            storage::upsert_extra_file_meta(
                &state.data_dir,
                &id,
                &rel_path,
                FileMeta { sha1: sha1.clone(), size },
            )
            .await
            .map_err(AppError::Storage)?;

            let mut manifest = storage::read_manifest(&state.data_dir, &id)
                .await
                .map_err(AppError::Storage)?;
            manifest.extra_files.retain(|f| f.path != rel_path);
            manifest.extra_files.push(ExtraFile {
                path: rel_path.clone(),
                download_url: download_url.clone(),
                sha1: sha1.clone(),
                size,
            });
            storage::write_manifest(&state.data_dir, &id, &manifest)
                .await
                .map_err(AppError::Storage)?;
        }

        uploaded.push(ExtraFile { path: rel_path, download_url, sha1, size });
    }

    Ok(Json(uploaded))
}

pub async fn integrate(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<IntegrateRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    if !instances.iter().any(|i| i.game_dir_name == id) {
        return Err(AppError::NotFound(format!("instance '{id}' not found")));
    }

    let public_url = state
        .config
        .read()
        .await
        .server
        .public_url
        .trim_end_matches('/')
        .to_string();
    let files_dir = state.files_dir(&id);
    let extra_dir = state.extra_files_dir(&id);

    let mut file_hashes: Vec<(String, u64)> = Vec::new();
    for entry in &body.files {
        if entry.name.contains("..") || entry.name.contains('/') || entry.name.contains('\\') {
            return Err(AppError::BadRequest(format!("invalid filename: {}", entry.name)));
        }
        let sanitized = sanitize_segment(&entry.name);
        if sanitized != entry.name || sanitized.is_empty() {
            return Err(AppError::BadRequest(format!("invalid filename: {}", entry.name)));
        }
        let abs = files_dir.join(&entry.name);
        let (sha1, size) = hash_file(&abs).await?;
        file_hashes.push((sha1, size));
    }

    let mut extra_hashes: Vec<(String, String, u64)> = Vec::new();
    for extra in &body.extra_files {
        let safe_path = validate_extra_path(&extra.path)?;
        if safe_path.is_empty() {
            return Err(AppError::BadRequest("extra file path is empty".into()));
        }
        let abs = extra_dir.join(&safe_path);
        let (sha1, size) = hash_file(&abs).await?;
        extra_hashes.push((safe_path, sha1, size));
    }

    let _guard = state.write_lock.lock().await;

    let mut manifest = storage::read_manifest(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;

    let mut files_index = storage::read_files_index(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;

    for (entry, (sha1, size)) in body.files.iter().zip(file_hashes.iter()) {
        let download_url = format!("{public_url}/files/{id}/{}", entry.name);

        files_index.insert(entry.name.clone(), FileMeta { sha1: sha1.clone(), size: *size });

        let section = match entry.section.as_str() {
            "mods" => &mut manifest.mods,
            "resourcePacks" => &mut manifest.resource_packs,
            "shaders" => &mut manifest.shaders,
            _ => {
                return Err(AppError::BadRequest(format!(
                    "invalid section: {}",
                    entry.section
                )))
            }
        };
        section.retain(|f| f.name != entry.name);
        section.push(ManifestFile {
            name: entry.name.clone(),
            download_url,
            sha1: sha1.clone(),
            size: *size,
            status: entry.status.clone(),
        });
    }

    storage::write_files_index(&state.data_dir, &id, &files_index)
        .await
        .map_err(AppError::Storage)?;

    let mut extra_index = storage::read_extra_files_index(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;

    for (safe_path, sha1, size) in &extra_hashes {
        let download_url = format!("{public_url}/extra/{id}/{safe_path}");

        extra_index.insert(safe_path.clone(), FileMeta { sha1: sha1.clone(), size: *size });

        manifest.extra_files.retain(|f| f.path != *safe_path);
        manifest.extra_files.push(ExtraFile {
            path: safe_path.clone(),
            download_url,
            sha1: sha1.clone(),
            size: *size,
        });
    }

    storage::write_extra_files_index(&state.data_dir, &id, &extra_index)
        .await
        .map_err(AppError::Storage)?;

    storage::write_manifest(&state.data_dir, &id, &manifest)
        .await
        .map_err(AppError::Storage)?;

    Ok(axum::http::StatusCode::OK)
}

pub async fn rehash(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RehashResult>, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    if !instances.iter().any(|i| i.game_dir_name == id) {
        return Err(AppError::NotFound(format!("instance '{id}' not found")));
    }

    let files_dir = state.files_dir(&id);
    let extra_dir = state.extra_files_dir(&id);

    let _guard = state.write_lock.lock().await;
    let mut manifest = storage::read_manifest(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;
    let mut files_index = storage::read_files_index(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;
    let mut extra_index = storage::read_extra_files_index(&state.data_dir, &id)
        .await
        .map_err(AppError::Storage)?;
    let mut updated = 0u32;

    for section in [
        &mut manifest.mods,
        &mut manifest.resource_packs,
        &mut manifest.shaders,
    ] {
        for f in section.iter_mut() {
            let abs = files_dir.join(&f.name);
            if !abs.exists() {
                continue;
            }
            let (sha1, size) = hash_file(&abs).await?;
            if sha1 != f.sha1 || size != f.size {
                f.sha1 = sha1.clone();
                f.size = size;
                files_index.insert(f.name.clone(), FileMeta { sha1, size });
                updated += 1;
            }
        }
    }

    for f in manifest.extra_files.iter_mut() {
        let abs = extra_dir.join(&f.path);
        if !abs.exists() {
            continue;
        }
        let (sha1, size) = hash_file(&abs).await?;
        if sha1 != f.sha1 || size != f.size {
            f.sha1 = sha1.clone();
            f.size = size;
            extra_index.insert(f.path.clone(), FileMeta { sha1, size });
            updated += 1;
        }
    }

    storage::write_files_index(&state.data_dir, &id, &files_index)
        .await
        .map_err(AppError::Storage)?;
    storage::write_extra_files_index(&state.data_dir, &id, &extra_index)
        .await
        .map_err(AppError::Storage)?;
    storage::write_manifest(&state.data_dir, &id, &manifest)
        .await
        .map_err(AppError::Storage)?;

    Ok(Json(RehashResult { updated }))
}

pub async fn delete_extra_file(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((id, file_path)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let safe_path = validate_extra_path(&file_path)?;
    if safe_path.is_empty() {
        return Err(AppError::BadRequest("file path is required".into()));
    }

    let base = state.extra_files_dir(&id);
    let abs_path = base.join(&safe_path);

    {
        let _guard = state.write_lock.lock().await;

        tokio::fs::remove_file(&abs_path)
            .await
            .map_err(|_| AppError::NotFound(format!("{safe_path} not found")))?;

        storage::remove_extra_file_meta(&state.data_dir, &id, &safe_path)
            .await
            .map_err(AppError::Storage)?;

        let mut manifest = storage::read_manifest(&state.data_dir, &id)
            .await
            .map_err(AppError::Storage)?;
        manifest.extra_files.retain(|f| f.path != safe_path);
        storage::write_manifest(&state.data_dir, &id, &manifest)
            .await
            .map_err(AppError::Storage)?;
    }

    cleanup_empty_parents(&base, std::path::Path::new(&safe_path)).await;

    Ok(StatusCode::NO_CONTENT)
}
