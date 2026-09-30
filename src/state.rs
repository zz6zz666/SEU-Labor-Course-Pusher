//! Persisted long-lived runtime state (pushed ids, auth state, counters).
//! Writes are atomic via a temp file + rename; the JSON layout matches the Go
//! implementation.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthState {
    Unknown,
    Valid,
    Expired,
}

impl AuthState {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthState::Unknown => "unknown",
            AuthState::Valid => "valid",
            AuthState::Expired => "expired",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    #[serde(rename = "pushedUniqueIds")]
    pub pushed_unique_ids: Vec<String>,
    #[serde(rename = "autoHandledIds")]
    pub auto_handled_ids: Vec<String>,
    #[serde(rename = "authState")]
    pub auth_state: AuthState,
    #[serde(rename = "firstRunCompleted")]
    pub first_run_completed: bool,
    #[serde(rename = "lastRunAt")]
    pub last_run_at: String,
    #[serde(rename = "lastSuccessAt")]
    pub last_success_at: String,
    #[serde(rename = "lastAuthFailureAt")]
    pub last_auth_failure_at: String,
    #[serde(rename = "consecutiveFailures")]
    pub consecutive_failures: i64,
    #[serde(rename = "lastSummaryDate")]
    pub last_summary_date: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            pushed_unique_ids: Vec::new(),
            auto_handled_ids: Vec::new(),
            auth_state: AuthState::Unknown,
            first_run_completed: false,
            last_run_at: String::new(),
            last_success_at: String::new(),
            last_auth_failure_at: String::new(),
            consecutive_failures: 0,
            last_summary_date: String::new(),
        }
    }
}

pub struct Store {
    mu: Mutex<State>,
    path: PathBuf,
}

impl Store {
    pub fn open(path: PathBuf) -> anyhow::Result<Arc<Store>> {
        let mut s = State::default();
        if let Ok(raw) = std::fs::read(&path) {
            let body = raw.strip_prefix(UTF8_BOM).unwrap_or(&raw);
            s = serde_json::from_slice(body)?;
        }
        let st = Arc::new(Store {
            mu: Mutex::new(s),
            path,
        });
        st.save()?;
        Ok(st)
    }

    pub fn get(&self) -> State {
        self.mu.lock().unwrap().clone()
    }

    pub fn update<F: FnOnce(&mut State)>(&self, f: F) -> anyhow::Result<()> {
        let mut guard = self.mu.lock().unwrap();
        f(&mut guard);
        self.save_locked(&guard)
    }

    pub fn add_pushed(&self, ids: &[String]) -> anyhow::Result<()> {
        self.update(|s| s.pushed_unique_ids = union(&s.pushed_unique_ids, ids))
    }

    pub fn set_pushed(&self, ids: &[String]) -> anyhow::Result<()> {
        self.update(|s| {
            if s.pushed_unique_ids != ids {
                s.pushed_unique_ids = ids.to_vec();
            }
        })
    }

    pub fn add_auto_handled(&self, ids: &[String]) -> anyhow::Result<()> {
        self.update(|s| s.auto_handled_ids = union(&s.auto_handled_ids, ids))
    }

    pub fn set_auto_handled(&self, ids: &[String]) -> anyhow::Result<()> {
        self.update(|s| {
            if s.auto_handled_ids != ids {
                s.auto_handled_ids = ids.to_vec();
            }
        })
    }

    pub fn set_auth_state(&self, a: AuthState) -> anyhow::Result<()> {
        self.update(|s| {
            s.auth_state = a;
            if a == AuthState::Expired {
                s.last_auth_failure_at = now();
            }
        })
    }

    fn save(&self) -> anyhow::Result<()> {
        let guard = self.mu.lock().unwrap();
        self.save_locked(&guard)
    }

    fn save_locked(&self, s: &State) -> anyhow::Result<()> {
        let mut raw = serde_json::to_string_pretty(s)?;
        raw.push('\n');
        let tmp = PathBuf::from(format!("{}.tmp", self.path.display()));
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn union(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(a.len() + b.len());
    for v in a.iter().chain(b.iter()) {
        if !out.iter().any(|x| x == v) {
            out.push(v.clone());
        }
    }
    out
}
