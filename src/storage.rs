use std::{collections::HashMap, path::Path};
use crate::models::{FileMeta, InstanceInfo, InstanceManifest, LauncherRelease, NewsItem};

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

async fn read_json<T>(path: &Path) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("parse {}: {e}", path.display()))
}

async fn write_json<T: serde::Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, &content)
        .await
        .map_err(|e| format!("write tmp: {e}"))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| format!("rename to {}: {e}", path.display()))
}

pub async fn read_instances(data_dir: &Path) -> Result<Vec<InstanceInfo>, String> {
    read_json(&data_dir.join("instances.json")).await
}

pub async fn write_instances(data_dir: &Path, items: &[InstanceInfo]) -> Result<(), String> {
    write_json(&data_dir.join("instances.json"), items).await
}

pub async fn read_news(data_dir: &Path) -> Result<Vec<NewsItem>, String> {
    read_json(&data_dir.join("news.json")).await
}

pub async fn write_news(data_dir: &Path, items: &[NewsItem]) -> Result<(), String> {
    write_json(&data_dir.join("news.json"), items).await
}

pub async fn read_manifest(data_dir: &Path, game_dir: &str) -> Result<InstanceManifest, String> {
    if !valid_id(game_dir) {
        return Err(format!("invalid instance id: {game_dir}"));
    }
    read_json(&data_dir.join("instances").join(game_dir).join("manifest.json")).await
}

pub async fn write_manifest(
    data_dir: &Path,
    game_dir: &str,
    manifest: &InstanceManifest,
) -> Result<(), String> {
    if !valid_id(game_dir) {
        return Err(format!("invalid instance id: {game_dir}"));
    }
    write_json(
        &data_dir.join("instances").join(game_dir).join("manifest.json"),
        manifest,
    )
    .await
}

fn files_index_path(data_dir: &Path, game_dir: &str) -> std::path::PathBuf {
    data_dir.join("instances").join(game_dir).join("files.json")
}

pub async fn read_files_index(
    data_dir: &Path,
    game_dir: &str,
) -> Result<HashMap<String, FileMeta>, String> {
    read_json(&files_index_path(data_dir, game_dir)).await
}

pub async fn write_files_index(
    data_dir: &Path,
    game_dir: &str,
    index: &HashMap<String, FileMeta>,
) -> Result<(), String> {
    write_json(&files_index_path(data_dir, game_dir), index).await
}

pub async fn read_launcher_release(data_dir: &Path) -> Result<LauncherRelease, String> {
    read_json(&data_dir.join("launcher-release.json")).await
}

pub async fn write_launcher_release(data_dir: &Path, release: &LauncherRelease) -> Result<(), String> {
    write_json(&data_dir.join("launcher-release.json"), release).await
}

pub async fn upsert_file_meta(
    data_dir: &Path,
    game_dir: &str,
    name: &str,
    meta: FileMeta,
) -> Result<(), String> {
    let mut index = read_files_index(data_dir, game_dir).await?;
    index.insert(name.to_string(), meta);
    write_files_index(data_dir, game_dir, &index).await
}

pub async fn remove_file_meta(
    data_dir: &Path,
    game_dir: &str,
    name: &str,
) -> Result<(), String> {
    let mut index = read_files_index(data_dir, game_dir).await?;
    index.remove(name);
    write_files_index(data_dir, game_dir, &index).await
}
