use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    pub name: String,
    pub game_dir_name: String,
    pub version: String,
    pub mc_version: String,
    pub loader: ModLoader,
    pub loader_version: String,
    pub icon_url: String,
    pub bg_url: Option<String>,
    pub update_url: String,
    pub server_ip: Option<String>,
    pub maintenance: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModLoader {
    Vanilla,
    Fabric,
    Forge,
    #[serde(rename = "NEOFORGE")]
    NeoForge,
    Quilt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceManifest {
    pub mods: Vec<ManifestFile>,
    pub resource_packs: Vec<ManifestFile>,
    pub shaders: Vec<ManifestFile>,
    pub extra_files: Vec<ExtraFile>,
}

impl Default for InstanceManifest {
    fn default() -> Self {
        Self {
            mods: vec![],
            resource_packs: vec![],
            shaders: vec![],
            extra_files: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub name: String,
    #[serde(rename = "downloadURL")]
    pub download_url: String,
    pub sha1: String,
    pub size: u64,
    pub status: ModStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModStatus {
    Required,
    OptionalOn,
    OptionalOff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtraFile {
    pub path: String,
    #[serde(rename = "downloadURL")]
    pub download_url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub content: String,
    #[serde(rename = "type")]
    pub news_type: NewsType,
    pub date: String,
    pub image_url: Option<String>,
    pub action_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NewsType {
    Update,
    Event,
    Maintenance,
    Info,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub instance_count: usize,
    pub news_count: usize,
    pub total_files: usize,
    pub total_size_bytes: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileMeta {
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedFile {
    pub name: String,
    pub sha1: String,
    pub size: u64,
    pub download_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileListEntry {
    pub name: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}
