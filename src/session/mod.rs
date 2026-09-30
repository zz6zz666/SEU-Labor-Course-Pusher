//! Owns the HTTP client, the persistent cookie jar and the site endpoints. The
//! login is bound to a session cookie, so the whole jar (including non-expiring
//! cookies) must be kept.

pub mod jar;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use url::Url;

use jar::Jar;

/// The site rejects some requests unless a desktop Chrome UA is presented.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

pub const COURSE_PAGE: &str = "https://labor.seu.edu.cn/SJItemKaiKe/XuanKe/Index";
pub const CAS_LOGIN: &str = "https://auth.seu.edu.cn/dist/#/dist/main/login?service=https://labor.seu.edu.cn/UnifiedAuth/CASLogin";
pub const PUSHPLUS: &str = "https://www.pushplus.plus/send";

pub struct Response {
    pub status: u16,
    pub body: String,
    pub final_url: String,
}

pub struct Client {
    pub jar: Arc<Jar>,
    agent: ureq::Agent,
}

/// Builds a ureq agent with the system TLS backend (SChannel on Windows) and
/// manual redirects (we follow them ourselves to capture cookies and the final
/// URL).
pub fn build_agent(timeout: Duration) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new().timeout(timeout).redirects(0);
    if let Ok(connector) = ureq::native_tls::TlsConnector::new() {
        builder = builder.tls_connector(Arc::new(connector));
    }
    builder.build()
}

impl Client {
    pub fn new(jar_path: PathBuf, timeout: Duration) -> Result<Client> {
        let jar = Arc::new(Jar::load(jar_path)?);
        Ok(Client::with_jar(jar, timeout))
    }

    pub fn with_jar(jar: Arc<Jar>, timeout: Duration) -> Client {
        Client {
            jar,
            agent: build_agent(timeout),
        }
    }

    pub fn get(&self, url: &str) -> Result<Response> {
        self.execute(
            "GET",
            url,
            None,
            None,
            &[
                (
                    "Accept",
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                ),
                ("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8"),
            ],
        )
    }

    pub fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<Response> {
        let body = form
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        self.execute(
            "POST",
            url,
            Some(&body),
            Some("application/x-www-form-urlencoded"),
            &[("Accept", "application/json, text/plain, */*")],
        )
    }

    pub fn post_json(&self, url: &str, body: &str) -> Result<Response> {
        self.execute(
            "POST",
            url,
            Some(body),
            Some("application/json; charset=utf-8"),
            &[("Accept", "application/json, text/plain, */*")],
        )
    }

    fn execute(
        &self,
        method: &str,
        url: &str,
        body: Option<&str>,
        content_type: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<Response> {
        let mut current = url.to_string();
        let mut method = method.to_string();
        let mut body: Option<String> = body.map(|s| s.to_string());

        for _ in 0..11 {
            let mut req = self.agent.request(&method, &current);
            req = req.set("User-Agent", USER_AGENT);
            for (k, v) in headers {
                req = req.set(k, v);
            }
            if let Ok(u) = Url::parse(&current) {
                if let Some(cookie) = self.jar.cookie_header(&u) {
                    req = req.set("Cookie", &cookie);
                }
            }
            if let (Some(ct), Some(_)) = (content_type, body.as_deref()) {
                req = req.set("Content-Type", ct);
            }

            let result = match body.as_deref() {
                Some(b) => req.send_string(b),
                None => req.call(),
            };
            let resp = match result {
                Ok(r) => r,
                Err(ureq::Error::Status(_, r)) => r,
                Err(e) => return Err(e.into()),
            };

            let set_cookies: Vec<String> = resp
                .all("set-cookie")
                .iter()
                .map(|s| s.to_string())
                .collect();
            if let Ok(u) = Url::parse(&current) {
                self.jar.set_cookies(&u, &set_cookies);
            }

            let status = resp.status();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                if let Some(loc) = resp.header("location").map(|s| s.to_string()) {
                    let next = Url::parse(&current)
                        .ok()
                        .and_then(|u| u.join(&loc).ok())
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| loc.clone());
                    if status == 303 || ((status == 301 || status == 302) && method == "POST") {
                        method = "GET".to_string();
                        body = None;
                    }
                    current = next;
                    continue;
                }
            }

            let final_url = current.clone();
            let text = resp.into_string().unwrap_or_default();
            return Ok(Response {
                status,
                body: text,
                final_url,
            });
        }
        Err(anyhow!("重定向次数过多"))
    }
}

fn encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
