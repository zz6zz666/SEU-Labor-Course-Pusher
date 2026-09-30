//! Serves the settings page over loopback HTTP. A native WebView2 window (see
//! `appwindow`) renders the page, and its `window.seuWizard` API is backed by
//! `/api/*` endpoints, so the original wizard.html is reused unchanged.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

use crate::logging::Logger;
use crate::web;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub first_run_completed: bool,
    pub credentials_configured: bool,
    pub pushplus_configured: bool,
    pub windows_notify_enabled: bool,
    pub auth_state: String,
    pub watcher_message: String,
    pub last_success_at: String,
    pub auto_start_enabled: bool,
    pub auto_select_enabled: bool,
    pub filters_configured: bool,
    pub config_path: String,
    pub data_dir: String,
    pub version: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub username: String,
    pub password: String,
    pub pushplus_token: String,
    pub pushplus_enabled: bool,
    pub windows_notify_enabled: bool,
    pub auto_launch_at_login: bool,
    pub auto_select: bool,
    pub location_whitelist: Vec<String>,
    pub category_blacklist: Vec<String>,
}

type Callback = Box<dyn Fn() + Send + Sync>;

pub struct Actions {
    pub status: Box<dyn Fn() -> Status + Send + Sync>,
    pub config: Box<dyn Fn() -> ConfigView + Send + Sync>,
    pub verify: Box<dyn Fn() -> (String, String) + Send + Sync>,
    pub save_account: Box<dyn Fn(&str, &str) -> Result<()> + Send + Sync>,
    pub save_filters: Box<dyn Fn(&[String], &[String]) -> Result<()> + Send + Sync>,
    pub save_notify: Box<dyn Fn(&str, bool, bool) -> Result<()> + Send + Sync>,
    pub set_behavior: Box<dyn Fn(Option<bool>, Option<bool>) -> Result<()> + Send + Sync>,
    pub open_login: Callback,
    pub open_course: Callback,
    pub open_config: Callback,
    pub open_logs: Callback,
    pub open_external: Box<dyn Fn(&str) -> Result<()> + Send + Sync>,
}

pub struct Server {
    addr: SocketAddr,
    actions: Actions,
    log: Logger,
    stop: AtomicBool,
    on_close: Mutex<Option<Callback>>,
}

/// Starts the loopback settings API. It does not open any window itself: the
/// native WebView2 window consumes these endpoints.
pub fn open(actions: Actions, log: Logger) -> Result<Arc<Server>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = Arc::new(Server {
        addr,
        actions,
        log,
        stop: AtomicBool::new(false),
        on_close: Mutex::new(None),
    });

    let accept_server = server.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if accept_server.stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let conn_server = accept_server.clone();
            std::thread::spawn(move || {
                let _ = handle_conn(&conn_server, stream);
            });
        }
    });

    Ok(server)
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    /// Registers a callback fired exactly once when the server closes.
    pub fn set_on_close(&self, f: Callback) {
        *self.on_close.lock().unwrap() = Some(f);
    }

    pub fn close(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        // Unblock the accept loop.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        let cb = self.on_close.lock().unwrap().take();
        if let Some(cb) = cb {
            cb();
        }
    }
}

fn handle_conn(server: &Arc<Server>, mut stream: TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if buf.len() > 1 << 20 {
            return Ok(());
        }
    }

    let header_text = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    let response = route(server, &method, &path, &body);
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn route(server: &Arc<Server>, method: &str, path: &str, body: &str) -> String {
    let a = &server.actions;
    match (method, path) {
        ("GET", "/") => html_response(&page()),
        ("GET", "/api/status") => json_ok(&(a.status)()),
        ("GET", "/api/config") => json_ok(&(a.config)()),
        ("POST", "/api/verify") => {
            let (state, reason) = (a.verify)();
            json_ok(&serde_json::json!({ "state": state, "reason": reason }))
        }
        ("POST", "/api/save-account") => {
            let v = parse_body(body);
            result_response((a.save_account)(&str_of(&v, "username"), &str_of(&v, "password")))
        }
        ("POST", "/api/save-filters") => {
            let v = parse_body(body);
            result_response((a.save_filters)(
                &arr_of(&v, "locationWhitelist"),
                &arr_of(&v, "categoryBlacklist"),
            ))
        }
        ("POST", "/api/save-notify") => {
            let v = parse_body(body);
            let token = str_of(&v, "pushplusToken");
            let pp = v
                .get("pushplusEnabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let wn = v
                .get("windowsNotifyEnabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            result_response((a.save_notify)(&token, pp, wn))
        }
        ("POST", "/api/set-behavior") => {
            let v = parse_body(body);
            let auto_start = v.get("autoStart").and_then(|x| x.as_bool());
            let auto_select = v.get("autoSelect").and_then(|x| x.as_bool());
            result_response((a.set_behavior)(auto_start, auto_select))
        }
        ("POST", "/api/open-login") => {
            (a.open_login)();
            json_ok(&serde_json::json!({ "ok": true }))
        }
        ("POST", "/api/open-course") => {
            (a.open_course)();
            json_ok(&serde_json::json!({ "ok": true }))
        }
        ("POST", "/api/open-config") => {
            (a.open_config)();
            json_ok(&serde_json::json!({ "ok": true }))
        }
        ("POST", "/api/open-logs") => {
            (a.open_logs)();
            json_ok(&serde_json::json!({ "ok": true }))
        }
        ("POST", "/api/open-external") => {
            let v = parse_body(body);
            result_response((a.open_external)(&str_of(&v, "url")))
        }
        ("POST", "/api/close") => {
            let srv = server.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                srv.close();
            });
            json_ok(&serde_json::json!({ "ok": true }))
        }
        _ => {
            let _ = &server.log;
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
        }
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_body(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

fn str_of(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn arr_of(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn json_ok<T: Serialize>(v: &T) -> String {
    let body = serde_json::to_string(v).unwrap_or_else(|_| "null".to_string());
    json_response(&body)
}

fn json_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn html_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn result_response(res: Result<()>) -> String {
    match res {
        Ok(()) => json_ok(&serde_json::json!({ "ok": true })),
        Err(e) => {
            let body = serde_json::json!({ "ok": false, "error": e.to_string() }).to_string();
            format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
        }
    }
}

fn page() -> String {
    let page = web::WIZARD_HTML.replace(CSP_ORIGINAL, CSP_PATCHED);
    page.replacen("<head>", &format!("<head>\n{}", SHIM), 1)
}

const CSP_ORIGINAL: &str =
    "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline';";
const CSP_PATCHED: &str = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:;";

const SHIM: &str = r#"<script>
(function () {
  async function call(path, body) {
    const opt = { method: body === undefined ? 'GET' : 'POST', headers: {} };
    if (body !== undefined) { opt.headers['Content-Type'] = 'application/json'; opt.body = JSON.stringify(body); }
    const res = await fetch(path, opt);
    const text = await res.text();
    let data = null;
    try { data = text ? JSON.parse(text) : null; } catch (e) { data = null; }
    if (!res.ok) { throw new Error((data && data.error) || ('HTTP ' + res.status)); }
    return data;
  }
  window.seuWizard = {
    verify: () => call('/api/verify', {}),
    getStatus: () => call('/api/status'),
    getConfig: () => call('/api/config'),
    saveAccount: (d) => call('/api/save-account', d),
    saveFilters: (d) => call('/api/save-filters', d),
    saveNotify: (d) => call('/api/save-notify', d),
    setBehavior: (d) => call('/api/set-behavior', d),
    openLogin: () => call('/api/open-login', {}),
    openCourse: () => call('/api/open-course', {}),
    openConfig: () => call('/api/open-config', {}),
    openLogs: () => call('/api/open-logs', {}),
    openExternal: (url) => call('/api/open-external', { url: url }),
    close: () => call('/api/close', {})
  };
})();
</script>
"#;
