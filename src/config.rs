use std::{io, path::Path};

use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub github: GitHubConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitHubConfig {
    pub pat: String,
    pub repo: String,
    #[serde(default = "default_workflow")]
    pub workflow: String,
    #[serde(default = "default_branch")]
    pub branch: String,
}

fn default_workflow() -> String { "release.yml".into() }
fn default_branch() -> String { "main".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub data_dir: String,
    pub public_url: String,
    pub max_upload_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub username: String,
    pub password_hash: String,
    pub jwt_secret: String,
    pub jwt_expiry_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            github: GitHubConfig::default(),
            server: ServerConfig {
                port: 8080,
                data_dir: "./data".to_string(),
                public_url: "http://localhost:8080".to_string(),
                max_upload_mb: 512,
            },
            auth: AuthConfig {
                username: "admin".to_string(),
                password_hash: String::new(),
                jwt_secret: String::new(),
                jwt_expiry_secs: 86400,
            },
        }
    }
}

impl Config {
    pub fn load_from(path: &Path) -> Self {
        Figment::new()
            .merge(Toml::file(path))
            .merge(Env::prefixed("CORVUS_").split("__"))
            .extract()
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        let content = toml::to_string_pretty(self).expect("serialize config");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let c = Config::default();
        assert_eq!(c.server.port, 8080);
        assert_eq!(c.auth.username, "admin");
        assert!(c.auth.password_hash.is_empty());
    }

    #[test]
    fn load_from_nonexistent_returns_default() {
        let c = Config::load_from(Path::new("/nonexistent/config.toml"));
        assert_eq!(c.server.port, 8080);
    }

    #[test]
    fn save_to_and_reload() {
        let dir = std::env::temp_dir().join("corvus_test_config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let mut cfg = Config::default();
        cfg.server.port = 9999;
        cfg.save_to(&path).unwrap();

        let loaded = Config::load_from(&path);
        assert_eq!(loaded.server.port, 9999);

        std::fs::remove_file(&path).ok();
    }
}
