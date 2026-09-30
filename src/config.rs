//! On-disk configuration schema and a store that loads, validates and persists
//! it. The JSON layout mirrors the previous Go implementation (field order,
//! 2-space indent, camelCase keys) so existing installs keep working. The
//! `filters` keys were renamed to spell out which list is a whitelist and which
//! is a blacklist; the old names are still accepted on read.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Filters. `location_whitelist` keeps a course whose 开课地点 contains any
/// keyword (substring whitelist); `category_blacklist` drops a course whose
/// 项目类别 exactly matches any keyword (blacklist). An empty list means no
/// filtering for that dimension.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Filters {
    #[serde(rename = "locationWhitelist", alias = "locations")]
    pub location_whitelist: Vec<String>,
    #[serde(rename = "categoryBlacklist", alias = "categories")]
    pub category_blacklist: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    #[serde(rename = "refreshIntervalMs")]
    pub refresh_interval_ms: i64,
    #[serde(rename = "jitterRatio")]
    pub jitter_ratio: f64,
    #[serde(rename = "dailySummaryHour")]
    pub daily_summary_hour: Option<i32>,
    #[serde(rename = "failureAlertThreshold")]
    pub failure_alert_threshold: i32,
    #[serde(rename = "maxBackoffMs")]
    pub max_backoff_ms: i64,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            refresh_interval_ms: 180_000,
            jitter_ratio: 0.1,
            daily_summary_hour: Some(21),
            failure_alert_threshold: 3,
            max_backoff_ms: 30 * 60_000,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PushPlus {
    pub enabled: bool,
    pub token: String,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowsNotify {
    pub enabled: bool,
    #[serde(rename = "openBrowserOnClick")]
    pub open_browser_on_click: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Push {
    pub pushplus: PushPlus,
    pub windows: WindowsNotify,
}

/// Optional override for the browser used by the login / course-view windows.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    /// Explicit path to a Chromium-family browser executable. When empty, one
    /// is discovered automatically (Edge, Chrome, or another Chromium build).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Behavior {
    #[serde(rename = "autoLogin")]
    pub auto_login: bool,
    #[serde(rename = "autoOpenOnAuthFailure")]
    pub auto_open_on_auth_failure: bool,
    #[serde(rename = "autoLaunchAtLogin")]
    pub auto_launch_at_login: bool,
    #[serde(rename = "autoSelect")]
    pub auto_select: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Logging {
    pub level: String,
    #[serde(rename = "retentionDays")]
    pub retention_days: i32,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub credentials: Credentials,
    pub filters: Filters,
    pub schedule: Schedule,
    pub push: Push,
    pub behavior: Behavior,
    pub logging: Logging,
    #[serde(skip_serializing_if = "BrowserConfig::is_empty")]
    pub browser: BrowserConfig,
}

impl BrowserConfig {
    fn is_empty(&self) -> bool {
        self.path.trim().is_empty()
    }
}

impl Default for PushPlus {
    fn default() -> Self {
        PushPlus {
            enabled: true,
            token: String::new(),
            title: "劳动教育课程推送".to_string(),
        }
    }
}

impl Default for WindowsNotify {
    fn default() -> Self {
        WindowsNotify {
            enabled: true,
            open_browser_on_click: true,
        }
    }
}

impl Default for Logging {
    fn default() -> Self {
        Logging {
            level: "info".to_string(),
            retention_days: 7,
        }
    }
}

impl Default for Behavior {
    fn default() -> Self {
        Behavior {
            auto_login: true,
            auto_open_on_auth_failure: true,
            auto_launch_at_login: false,
            auto_select: false,
        }
    }
}

impl Config {
    pub fn normalize(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        let valid = matches!(self.logging.level.as_str(), "debug" | "info" | "warn" | "error");
        if !valid {
            warnings.push(format!(
                "logging.level 非法({:?}),已重置为 info",
                self.logging.level
            ));
            self.logging.level = "info".to_string();
        }
        if self.logging.retention_days < 1 {
            warnings.push("logging.retentionDays 非法,已重置为 7".to_string());
            self.logging.retention_days = 7;
        }
        if self.schedule.refresh_interval_ms < 30_000 {
            warnings
                .push("schedule.refreshIntervalMs 不得小于 30000,已重置为 180000".to_string());
            self.schedule.refresh_interval_ms = 180_000;
        }
        if self.schedule.jitter_ratio < 0.0 || self.schedule.jitter_ratio > 0.5 {
            warnings.push("schedule.jitterRatio 应在 0~0.5 之间,已重置为 0.1".to_string());
            self.schedule.jitter_ratio = 0.1;
        }
        if let Some(h) = self.schedule.daily_summary_hour {
            if !(0..=23).contains(&h) {
                warnings
                    .push("schedule.dailySummaryHour 应为 0~23 或 null,已重置为 21".to_string());
                self.schedule.daily_summary_hour = Some(21);
            }
        }
        warnings
    }

    pub fn has_credentials(&self) -> bool {
        !self.credentials.username.is_empty() && !self.credentials.password.is_empty()
    }

    /// The configured browser override, or `None` when auto-discovery should be
    /// used.
    pub fn browser_path(&self) -> Option<&str> {
        let p = self.browser.path.trim();
        if p.is_empty() {
            None
        } else {
            Some(p)
        }
    }

    pub fn safe_summary(&self) -> serde_json::Value {
        serde_json::json!({
            "usernameConfigured": !self.credentials.username.is_empty(),
            "locationWhitelist": self.filters.location_whitelist,
            "categoryBlacklist": self.filters.category_blacklist,
            "refreshIntervalMs": self.schedule.refresh_interval_ms,
            "dailySummaryHour": self.schedule.daily_summary_hour,
            "pushplusEnabled": self.push.pushplus.enabled,
            "pushplusTokenConfigured": !self.push.pushplus.token.is_empty(),
            "windowsNotifyEnabled": self.push.windows.enabled,
            "autoLogin": self.behavior.auto_login,
            "autoSelect": self.behavior.auto_select,
            "logLevel": self.logging.level,
        })
    }
}

pub struct Store {
    mu: Mutex<Config>,
    path: PathBuf,
}

fn write_file(path: &Path, cfg: &Config) -> std::io::Result<()> {
    let mut raw = serde_json::to_string_pretty(cfg).unwrap_or_default();
    raw.push('\n');
    crate::fsutil::atomic_write(path, raw.as_bytes())
}

impl Store {
    pub fn open(path: PathBuf) -> anyhow::Result<(Arc<Store>, Vec<String>)> {
        if !path.exists() {
            let cfg = Config::default();
            write_file(&path, &cfg)?;
            return Ok((
                Arc::new(Store {
                    mu: Mutex::new(cfg),
                    path,
                }),
                vec!["未找到 config.json,已写入默认配置".to_string()],
            ));
        }
        let raw = std::fs::read(&path)?;
        let body = raw.strip_prefix(UTF8_BOM).unwrap_or(&raw);
        let mut cfg: Config = serde_json::from_slice(body)
            .map_err(|e| anyhow::anyhow!("config.json 不是合法 JSON: {}", e))?;
        let warnings = cfg.normalize();
        Ok((
            Arc::new(Store {
                mu: Mutex::new(cfg),
                path,
            }),
            warnings,
        ))
    }

    pub fn get(&self) -> Config {
        self.mu.lock().unwrap().clone()
    }

    pub fn update<F: FnOnce(&mut Config)>(&self, f: F) -> anyhow::Result<()> {
        let mut guard = self.mu.lock().unwrap();
        let mut next = guard.clone();
        f(&mut next);
        next.normalize();
        write_file(&self.path, &next)?;
        *guard = next;
        Ok(())
    }

    pub fn reload(&self) -> (bool, Vec<String>) {
        let Ok(raw) = std::fs::read(&self.path) else {
            return (false, Vec::new());
        };
        let body = raw.strip_prefix(UTF8_BOM).unwrap_or(&raw);
        let Ok(mut next) = serde_json::from_slice::<Config>(body) else {
            return (false, Vec::new());
        };
        let warnings = next.normalize();

        let mut guard = self.mu.lock().unwrap();
        let before = serde_json::to_vec(&*guard).unwrap_or_default();
        let after = serde_json::to_vec(&next).unwrap_or_default();
        if before == after {
            return (false, Vec::new());
        }
        *guard = std::mem::take(&mut next);
        (true, warnings)
    }
}
