//! Drives the system Chromium browser through CDP. It never bundles an engine:
//! the executable is Edge/Chrome (or any Chromium fork) that is already
//! installed.

use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use crate::cookie::Cookie;
use crate::discovery;

/// A live browser process bound to a dedicated profile, with a CDP socket to a
/// page target.
pub struct Session {
    child: Child,
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: i64,
}

/// How the browser window should be presented.
#[derive(Clone, Copy)]
pub enum WindowMode {
    /// Chromeless app window (`--app`), maximized, for the interactive login.
    App,
    /// Ordinary window with tabs and an address bar, maximized, for course viewing.
    Browser,
    /// Chromeless app window at a fixed logical size, for the settings screen.
    AppFramed { width: i32, height: i32 },
}

/// Everything needed to launch one session.
pub struct SessionConfig {
    /// Show a window; when false the browser runs headless.
    pub visible: bool,
    /// Dedicated user-data directory. Empty uses the browser's default profile.
    pub profile_dir: String,
    /// Page to open; `None` opens the browser's own startup surface.
    pub url: Option<String>,
    /// How the window should be presented.
    pub mode: WindowMode,
    /// Explicit browser executable; `None` auto-discovers one.
    pub exec_path: Option<String>,
    /// Display name for the profile in the browser's profile picker. Empty
    /// leaves any existing name untouched.
    pub profile_name: String,
}

impl Session {
    /// Locates a browser and launches it with `cfg`.
    pub fn launch(cfg: SessionConfig) -> Result<Session> {
        let candidates = discovery::candidates(cfg.exec_path.as_deref());
        if candidates.is_empty() {
            return Err(discovery::no_browser_error());
        }
        let mut last: Option<anyhow::Error> = None;
        for exe in &candidates {
            match Self::launch_one(exe, &cfg) {
                Ok(c) => return Ok(c),
                Err(e) => last = Some(anyhow!("{} ({})", e, exe)),
            }
        }
        Err(last.unwrap_or_else(discovery::no_browser_error))
    }

    fn launch_one(exe: &str, cfg: &SessionConfig) -> Result<Session> {
        let profile_dir = cfg.profile_dir.as_str();
        let url = cfg.url.as_deref();
        // Free our profile from any lingering instance (which would otherwise
        // make Chromium hand the command line off and exit without a port), then
        // clean it in place so no "restore pages" bubble appears.
        if !profile_dir.is_empty() {
            crate::winproc::kill_for_profile(profile_dir);
            prepare_profile(profile_dir, &cfg.profile_name)?;
        }

        let mut cmd = command(exe);
        cmd.arg("--remote-debugging-port=0")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--noerrdialogs")
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-background-timer-throttling")
            .arg("--disable-renderer-backgrounding")
            .arg("--disable-backgrounding-occluded-windows")
            .arg("--disable-popup-blocking")
            .arg("--disable-prompt-on-repost")
            // Keep the browser from lingering in the background or handing a
            // new launch off to a stale instance (leaving us without a port).
            .arg("--disable-background-networking")
            .arg("--disable-background-mode")
            .arg("--disable-default-apps")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("--disable-component-update")
            .arg("--disable-client-side-phishing-detection")
            .arg("--disable-breakpad")
            .arg("--no-service-autorun")
            .arg("--password-store=basic")
            .arg("--use-mock-keychain")
            // A forced kill would otherwise make the next launch show a
            // "restore pages" bubble instead of our window.
            .arg("--hide-crash-restore-bubble")
            .arg("--disable-session-crashed-bubble")
            .arg("--disable-features=Translate,AutofillServerCommunication,InfiniteSessionRestore,CalculateNativeWinOcclusion");
        if !cfg.visible {
            cmd.arg("--headless=new");
        }
        if !profile_dir.is_empty() {
            cmd.arg(format!("--user-data-dir={}", profile_dir));
        }
        if let Some(url) = url {
            match cfg.mode {
                WindowMode::App => {
                    cmd.arg("--start-maximized");
                    cmd.arg(format!("--app={}", url));
                }
                WindowMode::Browser => {
                    cmd.arg("--start-maximized");
                    cmd.arg(url);
                }
                WindowMode::AppFramed { width, height } => {
                    cmd.arg(format!("--window-size={},{}", width, height));
                    cmd.arg(format!("--app={}", url));
                }
            }
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to launch browser: {}", exe))?;

        let port = match wait_for_devtools_port(&mut child, profile_dir) {
            Ok(p) => p,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let ws_url = match wait_for_page_target(port, cfg.url.as_deref()) {
            Ok(u) => u,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let (ws, _resp) = match connect(&ws_url) {
            Ok(x) => x,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!("failed to connect to the browser debugging port: {}", e));
            }
        };
        apply_timeouts(&ws);
        Ok(Session {
            child,
            ws,
            next_id: 1,
        })
    }

    /// Whether the browser process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Starts loading `url` without waiting for the document, for windows the
    /// caller only needs to display.
    pub fn navigate(&mut self, url: &str) -> Result<()> {
        self.call("Page.navigate", json!({ "url": url }))?;
        Ok(())
    }

    /// Runs script in the page context and decodes its JSON result.
    pub fn eval(&mut self, script: &str) -> Result<Value> {
        let r = self.call(
            "Runtime.evaluate",
            json!({
                "expression": script,
                "awaitPromise": true,
                "returnByValue": true,
            }),
        )?;
        if let Some(exc) = r.get("exceptionDetails") {
            return Err(anyhow!("script threw: {}", exc));
        }
        Ok(r.get("result")
            .and_then(|x| x.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// Returns every cookie in the profile, including session cookies.
    pub fn cookies(&mut self) -> Result<Vec<Cookie>> {
        let r = self.call("Storage.getCookies", json!({}))?;
        Ok(cookies_from_cdp(&r))
    }

    /// Injects cookies into the profile. Required before navigating: session
    /// cookies are not retained in the on-disk profile.
    pub fn set_cookies(&mut self, cookies: &[Cookie]) -> Result<()> {
        let arr: Vec<Value> = cookies
            .iter()
            .map(|c| {
                let mut o = json!({
                    "name": c.name,
                    "value": c.value,
                    "domain": c.domain,
                    "path": c.path,
                    "secure": c.secure,
                    "httpOnly": c.http_only,
                });
                if c.expires > 0 {
                    o["expires"] = json!(c.expires as f64);
                }
                o
            })
            .collect();
        self.call("Network.setCookies", json!({ "cookies": arr }))?;
        Ok(())
    }

    pub fn close(&mut self) {
        // Ask the browser to shut down cleanly so the profile is not flagged as
        // crashed. Native only: no taskkill, no console window.
        let _ = self.call("Browser.close", json!({}));
        for _ in 0..30 {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "id": id, "method": method, "params": params });
        self.ws
            .send(Message::text(msg.to_string()))
            .context("CDP send failed")?;
        loop {
            let raw = self.ws.read().context("CDP receive failed")?;
            let text = match raw {
                Message::Text(t) => t.as_str().to_string(),
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Ping(p) => {
                    let _ = self.ws.send(Message::Pong(p));
                    continue;
                }
                Message::Close(_) => return Err(anyhow!("CDP connection closed")),
                _ => continue,
            };
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id").and_then(|x| x.as_i64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(anyhow!("CDP {} failed: {}", method, err));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }
}

/// Builds a `Command` that never flashes a console window.
fn command(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Bounds the debug socket so a dead peer surfaces as an error instead of
/// blocking forever.
fn apply_timeouts(ws: &WebSocket<MaybeTlsStream<TcpStream>>) {
    let t = Some(Duration::from_secs(20));
    match ws.get_ref() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(t);
            let _ = s.set_write_timeout(t);
        }
        MaybeTlsStream::NativeTls(s) => {
            let _ = s.get_ref().set_read_timeout(t);
            let _ = s.get_ref().set_write_timeout(t);
        }
        _ => {}
    }
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// Maps a `Storage.getCookies` result into [`Cookie`]s. Session cookies
/// (expiry `-1`/`0`) become `expires == 0`.
fn cookies_from_cdp(result: &Value) -> Vec<Cookie> {
    match result.get("cookies").and_then(|x| x.as_array()) {
        Some(arr) => arr.iter().map(cookie_from_cdp).collect(),
        None => Vec::new(),
    }
}

fn cookie_from_cdp(c: &Value) -> Cookie {
    let session = c.get("session").and_then(|x| x.as_bool()).unwrap_or(false);
    let expires_raw = c.get("expires").and_then(|x| x.as_f64()).unwrap_or(-1.0);
    Cookie {
        name: str_field(c, "name"),
        value: str_field(c, "value"),
        domain: str_field(c, "domain"),
        path: str_field(c, "path"),
        expires: if session || expires_raw <= 0.0 {
            0
        } else {
            expires_raw as i64
        },
        secure: c.get("secure").and_then(|x| x.as_bool()).unwrap_or(false),
        http_only: c.get("httpOnly").and_then(|x| x.as_bool()).unwrap_or(false),
    }
}

fn wait_for_devtools_port(child: &mut Child, profile_dir: &str) -> Result<u16> {
    let path = PathBuf::from(profile_dir).join("DevToolsActivePort");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(txt) = std::fs::read_to_string(&path) {
            if let Some(line) = txt.lines().next() {
                if let Ok(p) = line.trim().parse::<u16>() {
                    if p > 0 {
                        return Ok(p);
                    }
                }
            }
        }
        // If our process exited, it handed the command off to an existing
        // instance: fail fast instead of waiting out the timeout.
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow!(
                "browser process exited early ({}); another instance may be using the same profile",
                status
            ));
        }
        if Instant::now() > deadline {
            return Err(anyhow!("timed out waiting for the browser debugging port"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_page_target(port: u16, app_url: Option<&str>) -> Result<String> {
    let url = format!("http://127.0.0.1:{}/json", port);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(resp) = ureq::get(&url).call() {
            if let Ok(text) = resp.into_string() {
                if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&text) {
                    let pages: Vec<&Value> = arr
                        .iter()
                        .filter(|t| t.get("type").and_then(|x| x.as_str()) == Some("page"))
                        .collect();
                    let url_of = |t: &&Value| {
                        t.get("url")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string()
                    };
                    // Prefer the app window we asked for, then any real
                    // (non-newtab) page, so a restored session cannot hijack it.
                    let pick = match app_url {
                        Some(a) if !a.is_empty() => pages
                            .iter()
                            .find(|t| url_of(t) == a)
                            .or_else(|| {
                                pages.iter().find(|t| {
                                    let u = url_of(t);
                                    u.starts_with("http") || u == "about:blank"
                                })
                            })
                            .or_else(|| pages.first()),
                        _ => pages.first(),
                    };
                    if let Some(t) = pick {
                        if let Some(ws) = t.get("webSocketDebuggerUrl").and_then(|x| x.as_str()) {
                            return Ok(ws.to_string());
                        }
                    }
                }
            }
        }
        if Instant::now() > deadline {
            return Err(anyhow!("timed out waiting for the browser page to become ready"));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Ensures the user-data directory exists and, when `display_name` is set,
/// carries it as the profile's name so the browser does not label our profile
/// as "unSpecified".
fn prepare_profile(dir: &str, display_name: &str) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    // Drop stale state so a fresh launch is not misled: singleton locks from a
    // killed background instance, and a `DevToolsActivePort` left by a previous
    // run (which would otherwise be read before the new browser rewrites it,
    // sending us to a dead port).
    for name in [
        "SingletonLock",
        "SingletonCookie",
        "SingletonSocket",
        "lockfile",
        "DevToolsActivePort",
    ] {
        let _ = std::fs::remove_file(PathBuf::from(dir).join(name));
    }
    sanitize_preferences(dir);

    let path = PathBuf::from(dir).join("Local State");
    let mut root: Value = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({}));
    if !root.is_object() {
        root = json!({});
    }

    let profile = ensure_obj(&mut root, "profile");
    let info = ensure_obj(profile, "info_cache");
    let def = ensure_obj(info, "Default");
    let has_name = def
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_name && !display_name.is_empty() {
        def["name"] = json!(display_name);
    }
    if profile.get("last_used").is_none() {
        profile["last_used"] = json!("Default");
    }

    let out = serde_json::to_vec(&root)?;
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn ensure_obj<'a>(v: &'a mut Value, key: &str) -> &'a mut Value {
    if !v.get(key).map(|x| x.is_object()).unwrap_or(false) {
        v[key] = json!({});
    }
    v.get_mut(key).unwrap()
}

/// Marks the profile as cleanly exited so Chromium does not offer to restore a
/// previous session (which would override `--app`).
fn sanitize_preferences(dir: &str) {
    let path = PathBuf::from(dir).join("Default").join("Preferences");
    let Ok(raw) = std::fs::read(&path) else {
        return;
    };
    let Ok(mut root) = serde_json::from_slice::<Value>(&raw) else {
        return;
    };
    if !root.is_object() {
        return;
    }
    match root.get_mut("profile").and_then(|p| p.as_object_mut()) {
        Some(profile) => {
            profile.insert("exit_type".to_string(), json!("Normal"));
            profile.insert("exited_cleanly".to_string(), json!(true));
        }
        None => {
            root["profile"] = json!({ "exit_type": "Normal", "exited_cleanly": true });
        }
    }
    if let Ok(out) = serde_json::to_vec(&root) {
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_cdp_cookies_including_session_and_expiring() {
        let result = json!({
            "cookies": [
                {
                    "name": "SID", "value": "abc", "domain": ".seu.edu.cn",
                    "path": "/", "session": true, "expires": -1.0,
                    "secure": true, "httpOnly": true
                },
                {
                    "name": "T", "value": "xyz", "domain": "labor.seu.edu.cn",
                    "path": "/app", "expires": 1_900_000_000.0_f64,
                    "secure": false, "httpOnly": false
                }
            ]
        });
        let cookies = cookies_from_cdp(&result);
        assert_eq!(cookies.len(), 2);

        assert_eq!(cookies[0].name, "SID");
        assert_eq!(cookies[0].expires, 0, "session cookie becomes 0");
        assert!(cookies[0].secure && cookies[0].http_only);

        assert_eq!(cookies[1].expires, 1_900_000_000);
        assert_eq!(cookies[1].path, "/app");
    }

    #[test]
    fn missing_or_malformed_cookies_yield_empty() {
        assert!(cookies_from_cdp(&json!({})).is_empty());
        assert!(cookies_from_cdp(&json!({ "cookies": "no" })).is_empty());
    }
}
