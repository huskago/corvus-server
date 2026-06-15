use std::{
    collections::HashMap,
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use tokio::sync::{Mutex as AsyncMutex, RwLock};

use crate::{config::Config, error::AppError};

pub struct LoginAttempts {
    pub count: u32,
    pub window_start: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: Arc<PathBuf>,
    pub data_dir: PathBuf,
    pub started_at: Arc<Instant>,
    pub write_lock: Arc<AsyncMutex<()>>,
    pub login_attempts: Arc<Mutex<HashMap<IpAddr, LoginAttempts>>>,
}

impl AppState {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let data_dir = PathBuf::from(&config.server.data_dir);
        Self {
            data_dir,
            config_path: Arc::new(config_path),
            config: Arc::new(RwLock::new(config)),
            started_at: Arc::new(Instant::now()),
            write_lock: Arc::new(AsyncMutex::new(())),
            login_attempts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn instance_dir(&self, game_dir_name: &str) -> PathBuf {
        self.data_dir.join("instances").join(game_dir_name)
    }

    pub fn files_dir(&self, game_dir_name: &str) -> PathBuf {
        self.instance_dir(game_dir_name).join("files")
    }

    pub fn updates_dir(&self, platform: &str) -> PathBuf {
        self.data_dir.join("launcher-updates").join(platform)
    }

    pub fn check_login_rate_limit(&self, ip: IpAddr) -> Result<(), AppError> {
        const MAX_ATTEMPTS: u32 = 10;
        const WINDOW_SECS: u64 = 60;

        let mut map = self.login_attempts.lock().unwrap();
        let now = Instant::now();

        let entry = map.entry(ip).or_insert_with(|| LoginAttempts {
            count: 0,
            window_start: now,
        });

        if entry.window_start.elapsed().as_secs() >= WINDOW_SECS {
            entry.count = 0;
            entry.window_start = now;
        }

        entry.count += 1;
        if entry.count > MAX_ATTEMPTS {
            Err(AppError::TooManyRequests)
        } else {
            Ok(())
        }
    }

    pub fn reset_login_attempts(&self, ip: IpAddr) {
        if let Ok(mut map) = self.login_attempts.lock() {
            map.remove(&ip);
        }
    }
}
