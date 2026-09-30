//! Runtime locations for data, config, logs and snapshots.
//!
//! Defaults target the per-user config directory; `SEU_DAEMON_DATA_DIR` and
//! `SEU_DAEMON_CONFIG` override them (useful for development).

use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "SEU劳动教育课程推送助手";

#[derive(Clone, Debug)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
    pub log_dir: PathBuf,
    pub snapshot_dir: PathBuf,
    pub state_path: PathBuf,
    pub cookies_path: PathBuf,
}

fn user_config_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("APPDATA") {
        return PathBuf::from(v);
    }
    PathBuf::from(".")
}

impl Paths {
    pub fn resolve() -> std::io::Result<Paths> {
        let data_dir = match std::env::var_os("SEU_DAEMON_DATA_DIR") {
            Some(v) => PathBuf::from(v),
            None => user_config_dir().join(APP_DIR_NAME),
        };
        let config_path = match std::env::var_os("SEU_DAEMON_CONFIG") {
            Some(v) => PathBuf::from(v),
            None => data_dir.join("config.json"),
        };

        let p = Paths {
            config_path,
            log_dir: data_dir.join("logs"),
            snapshot_dir: data_dir.join("snapshots"),
            state_path: data_dir.join("state.json"),
            cookies_path: data_dir.join("cookies.json"),
            data_dir,
        };
        for dir in [&p.data_dir, &p.log_dir, &p.snapshot_dir] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(p)
    }
}
