//! Persistent cookie jar that retains session cookies (Expires zero) as well,
//! which a standard jar drops. Persisting them is required: the site binds the
//! login to a non-expiring session cookie. The JSON layout matches the Go
//! implementation.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use url::Url;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: i64,
    pub secure: bool,
    #[serde(rename = "httpOnly")]
    pub http_only: bool,
}

impl StoredCookie {
    /// Borrow as the browser engine's cookie shape, for CDP injection.
    pub fn to_browser_cookie(&self) -> browserhost::Cookie {
        browserhost::Cookie {
            name: self.name.clone(),
            value: self.value.clone(),
            domain: self.domain.clone(),
            path: self.path.clone(),
            expires: self.expires,
            secure: self.secure,
            http_only: self.http_only,
        }
    }
}

impl From<browserhost::Cookie> for StoredCookie {
    fn from(c: browserhost::Cookie) -> Self {
        StoredCookie {
            name: c.name,
            value: c.value,
            domain: c.domain,
            path: c.path,
            expires: c.expires,
            secure: c.secure,
            http_only: c.http_only,
        }
    }
}

pub struct Jar {
    mu: Mutex<Vec<StoredCookie>>,
    path: Option<PathBuf>,
}

impl Jar {
    /// A non-persisting jar (save writes nowhere).
    pub fn empty() -> Jar {
        Jar {
            mu: Mutex::new(Vec::new()),
            path: None,
        }
    }

    pub fn load(path: PathBuf) -> Result<Jar> {
        let mut cookies: Vec<StoredCookie> = Vec::new();
        if let Ok(raw) = std::fs::read(&path) {
            let body = raw.strip_prefix(UTF8_BOM).unwrap_or(&raw);
            cookies = serde_json::from_slice(body)?;
        }
        let now = chrono::Utc::now().timestamp();
        cookies.retain(|c| c.expires == 0 || c.expires >= now);
        Ok(Jar {
            mu: Mutex::new(cookies),
            path: Some(path),
        })
    }

    pub fn set_cookies(&self, url: &Url, headers: &[String]) {
        let now = chrono::Utc::now().timestamp();
        let host = url.host_str().unwrap_or("").to_string();
        let mut guard = self.mu.lock().unwrap();
        for h in headers {
            let Ok(c) = cookie::Cookie::parse(h.clone()) else {
                continue;
            };
            let domain = c
                .domain()
                .map(|d| d.trim_start_matches('.').to_string())
                .unwrap_or_else(|| host.clone());
            let path = c.path().map(|p| p.to_string()).unwrap_or_else(|| "/".to_string());

            let mut expires = 0i64;
            if let Some(ma) = c.max_age() {
                if ma.whole_seconds() < 0 {
                    remove(&mut guard, c.name(), &domain, &path);
                    continue;
                }
                expires = now + ma.whole_seconds();
            } else if let Some(cookie::Expiration::DateTime(dt)) = c.expires() {
                expires = dt.unix_timestamp();
            }

            let sc = StoredCookie {
                name: c.name().to_string(),
                value: c.value().to_string(),
                domain,
                path,
                expires,
                secure: c.secure().unwrap_or(false),
                http_only: c.http_only().unwrap_or(false),
            };
            upsert(&mut guard, sc);
        }
    }

    /// Injects already-parsed cookies (e.g. harvested from the browser).
    pub fn set_stored(&self, cookies: &[StoredCookie]) {
        let mut guard = self.mu.lock().unwrap();
        for c in cookies {
            upsert(&mut guard, c.clone());
        }
    }

    /// Builds the `Cookie` request header value for `url`, or `None`.
    pub fn cookie_header(&self, url: &Url) -> Option<String> {
        let now = chrono::Utc::now().timestamp();
        let host = url.host_str().unwrap_or("");
        let path = url.path();
        let guard = self.mu.lock().unwrap();
        let parts: Vec<String> = guard
            .iter()
            .filter(|c| c.expires == 0 || c.expires >= now)
            .filter(|c| domain_match(host, &c.domain) && path_match(path, &c.path))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("; "))
        }
    }

    pub fn all(&self) -> Vec<StoredCookie> {
        self.mu.lock().unwrap().clone()
    }

    pub fn is_empty(&self) -> bool {
        self.mu.lock().unwrap().is_empty()
    }

    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut guard = self.mu.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        guard.retain(|c| c.expires == 0 || c.expires >= now);
        guard.sort_by(|a, b| {
            a.domain
                .cmp(&b.domain)
                .then_with(|| a.name.cmp(&b.name))
        });
        crate::fsutil::atomic_write_json(path, &*guard)
    }
}

fn upsert(cookies: &mut Vec<StoredCookie>, c: StoredCookie) {
    for slot in cookies.iter_mut() {
        if slot.name == c.name && slot.domain == c.domain && slot.path == c.path {
            *slot = c;
            return;
        }
    }
    cookies.push(c);
}

fn remove(cookies: &mut Vec<StoredCookie>, name: &str, domain: &str, path: &str) {
    cookies.retain(|c| !(c.name == name && c.domain == domain && c.path == path));
}

fn domain_match(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{}", domain))
}

fn path_match(req_path: &str, cookie_path: &str) -> bool {
    if cookie_path.is_empty() || cookie_path == "/" {
        return true;
    }
    req_path.starts_with(cookie_path)
}
