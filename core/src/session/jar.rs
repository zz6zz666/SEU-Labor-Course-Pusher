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
                if ma.whole_seconds() <= 0 {
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

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;

    #[test]
    fn keeps_session_cookies_and_honours_domain_and_path() {
        let jar = Jar::empty();
        let set_url = Url::parse("https://labor.seu.edu.cn/UnifiedAuth/CASLogin").unwrap();
        jar.set_cookies(
            &set_url,
            &[
                "sid=abc; Path=/; HttpOnly".to_string(),
                "scoped=1; Domain=.seu.edu.cn; Path=/SJItemKaiKe".to_string(),
            ],
        );

        let in_scope = Url::parse("https://labor.seu.edu.cn/SJItemKaiKe/XuanKe/Index").unwrap();
        let header = jar.cookie_header(&in_scope).unwrap();
        assert!(header.contains("sid=abc"));
        assert!(header.contains("scoped=1"));

        let out_of_path = Url::parse("https://labor.seu.edu.cn/other").unwrap();
        let header = jar.cookie_header(&out_of_path).unwrap();
        assert!(header.contains("sid=abc"));
        assert!(!header.contains("scoped=1"));
    }

    #[test]
    fn max_age_zero_removes_the_cookie() {
        let jar = Jar::empty();
        let url = Url::parse("https://labor.seu.edu.cn/").unwrap();
        jar.set_cookies(&url, &["sid=abc; Path=/".to_string()]);
        assert!(jar.cookie_header(&url).is_some());
        jar.set_cookies(&url, &["sid=; Path=/; Max-Age=0".to_string()]);
        assert!(jar.cookie_header(&url).is_none());
    }
}
