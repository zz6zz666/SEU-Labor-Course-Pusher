//! Leveled, daily-rotated log files.
//!
//! Every message passes through `redact()` so credentials and tokens can never
//! reach the log even if a caller forgets.

use std::fmt::Display;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Local;
use regex::Regex;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn parse(s: &str) -> Level {
        match s {
            "debug" => Level::Debug,
            "warn" => Level::Warn,
            "error" => Level::Error,
            _ => Level::Info,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

struct LoggerState {
    min_level: Level,
    log_dir: PathBuf,
    retention_days: i64,
    current_date: String,
    current_path: PathBuf,
}

static STATE: OnceLock<Mutex<LoggerState>> = OnceLock::new();

fn state() -> &'static Mutex<LoggerState> {
    STATE.get_or_init(|| {
        Mutex::new(LoggerState {
            min_level: Level::Info,
            log_dir: PathBuf::new(),
            retention_days: 7,
            current_date: String::new(),
            current_path: PathBuf::new(),
        })
    })
}

pub fn init(dir: PathBuf, level: Level, retention: i64) {
    let mut s = state().lock().unwrap();
    s.log_dir = dir;
    s.min_level = level;
    s.retention_days = retention;
    cleanup_locked(&mut s);
}

pub fn configure(level: Level, retention: i64) {
    let mut s = state().lock().unwrap();
    s.min_level = level;
    s.retention_days = retention;
    cleanup_locked(&mut s);
}

#[derive(Clone)]
pub struct Logger {
    tag: String,
}

pub fn new(tag: impl Into<String>) -> Logger {
    Logger { tag: tag.into() }
}

impl Logger {
    pub fn debug(&self, msg: impl Display) {
        write(Level::Debug, &self.tag, &msg.to_string());
    }
    pub fn info(&self, msg: impl Display) {
        write(Level::Info, &self.tag, &msg.to_string());
    }
    pub fn warn(&self, msg: impl Display) {
        write(Level::Warn, &self.tag, &msg.to_string());
    }
    pub fn error(&self, msg: impl Display) {
        write(Level::Error, &self.tag, &msg.to_string());
    }
}

fn redact_kv() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)("?(?:password|pwd|token)"?\s*[:=]\s*")[^"]*(")"#).unwrap())
}

fn redact_var() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(Password|Token|__RequestVerificationToken)([=:]\s*)[^\s&,"}]+"#)
            .unwrap()
    })
}

fn redact(s: &str) -> String {
    let s = redact_kv().replace_all(s, "$1***$2");
    redact_var().replace_all(&s, "$1$2***").into_owned()
}

fn write(level: Level, tag: &str, msg: &str) {
    let mut s = state().lock().unwrap();
    if level < s.min_level {
        return;
    }
    let msg = redact(msg);
    let line = format!(
        "[{}] [{}] [{}] {}\n",
        Local::now().format("%Y-%m-%dT%H:%M:%S%:z"),
        level.as_str(),
        tag,
        msg
    );

    ensure_file_locked(&mut s);
    if !s.current_path.as_os_str().is_empty() {
        if let Ok(mut f) = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&s.current_path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
    let _ = std::io::stderr().write_all(line.as_bytes());
}

fn ensure_file_locked(s: &mut LoggerState) {
    let date = Local::now().format("%Y-%m-%d").to_string();
    if date == s.current_date && !s.current_path.as_os_str().is_empty() {
        return;
    }
    s.current_date = date.clone();
    s.current_path = s.log_dir.join(format!("daemon-{}.log", date));
    cleanup_locked(s);
}

fn cleanup_locked(s: &mut LoggerState) {
    let Ok(entries) = std::fs::read_dir(&s.log_dir) else {
        return;
    };
    let cutoff = Local::now() - chrono::Duration::days(s.retention_days);
    let cutoff = cutoff.naive_local();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("daemon-") || !name.ends_with(".log") {
            continue;
        }
        if let Ok(info) = e.metadata() {
            if let Ok(modified) = info.modified() {
                let dt: chrono::DateTime<Local> = modified.into();
                if dt.naive_local() < cutoff {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}
