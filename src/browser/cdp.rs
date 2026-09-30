//! Drives the system Chromium browser through CDP. It never bundles an engine:
//! the executable is Edge/Chrome that is already installed.

use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use std::net::TcpStream;

use super::{discovery, Browser};
use crate::session::jar::StoredCookie;

pub struct Chrome {
    child: Child,
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: i64,
}

/// How the browser window should be presented.
#[derive(Clone, Copy)]
pub enum WindowMode {
    /// Chromeless app window (`--app`), used for the interactive login.
    App,
    /// Ordinary window with tabs and an address bar, used for course viewing.
    Browser,
}

impl Chrome {
    pub fn launch(
        visible: bool,
        profile_dir: &str,
        url: Option<&str>,
        mode: WindowMode,
        exec_path: Option<&str>,
    ) -> Result<Chrome> {
        let candidates = discovery::candidates(exec_path);
        if candidates.is_empty() {
            return Err(discovery::no_browser_error());
        }
        let mut last: Option<anyhow::Error> = None;
        for exe in &candidates {
            match Self::launch_one(exe, visible, profile_dir, url, mode) {
                Ok(c) => return Ok(c),
                Err(e) => last = Some(anyhow!("{} ({})", e, exe)),
            }
        }
        Err(last.unwrap_or_else(discovery::no_browser_error))
    }

    fn launch_one(
        exe: &str,
        visible: bool,
        profile_dir: &str,
        url: Option<&str>,
        mode: WindowMode,
    ) -> Result<Chrome> {
        // Free our profile from any lingering instance (which would otherwise
        // make Chromium hand the command line off and exit without a port), then
        // clean it in place so no "restore pages" bubble appears.
        if !profile_dir.is_empty() {
            super::winproc::kill_browsers_for_profile(profile_dir);
            prepare_profile(profile_dir)?;
        }

        let mut cmd = crate::osutil::command(exe);
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
        if !visible {
            cmd.arg("--headless=new");
        }
        if !profile_dir.is_empty() {
            cmd.arg(format!("--user-data-dir={}", profile_dir));
        }
        if let Some(url) = url {
            cmd.arg("--start-maximized");
            match mode {
                WindowMode::App => {
                    cmd.arg(format!("--app={}", url));
                }
                WindowMode::Browser => {
                    cmd.arg(url);
                }
            }
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("无法启动浏览器: {}", exe))?;

        let port = match wait_for_devtools_port(&mut child, profile_dir) {
            Ok(p) => p,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let ws_url = match wait_for_page_target(port, url) {
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
                return Err(anyhow!("无法连接浏览器调试端口: {}", e));
            }
        };
        apply_timeouts(&ws);
        Ok(Chrome {
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

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "id": id, "method": method, "params": params });
        self.ws
            .send(Message::text(msg.to_string()))
            .context("CDP 发送失败")?;
        loop {
            let raw = self.ws.read().context("CDP 读取失败")?;
            let text = match raw {
                Message::Text(t) => t.as_str().to_string(),
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Ping(p) => {
                    let _ = self.ws.send(Message::Pong(p));
                    continue;
                }
                Message::Close(_) => return Err(anyhow!("CDP 连接已关闭")),
                _ => continue,
            };
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id").and_then(|x| x.as_i64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(anyhow!("CDP {} 失败: {}", method, err));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }
}

impl Browser for Chrome {
    fn eval(&mut self, script: &str) -> Result<Value> {
        let r = self.call(
            "Runtime.evaluate",
            json!({
                "expression": script,
                "awaitPromise": true,
                "returnByValue": true,
            }),
        )?;
        if let Some(exc) = r.get("exceptionDetails") {
            return Err(anyhow!("脚本执行异常: {}", exc));
        }
        Ok(r.get("result")
            .and_then(|x| x.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn all_cookies(&mut self) -> Result<Vec<StoredCookie>> {
        let r = self.call("Storage.getCookies", json!({}))?;
        let arr = r
            .get("cookies")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::with_capacity(arr.len());
        for c in arr {
            let session = c.get("session").and_then(|x| x.as_bool()).unwrap_or(false);
            let expires_raw = c.get("expires").and_then(|x| x.as_f64()).unwrap_or(-1.0);
            out.push(StoredCookie {
                name: str_field(&c, "name"),
                value: str_field(&c, "value"),
                domain: str_field(&c, "domain"),
                path: str_field(&c, "path"),
                expires: if session || expires_raw <= 0.0 {
                    0
                } else {
                    expires_raw as i64
                },
                secure: c.get("secure").and_then(|x| x.as_bool()).unwrap_or(false),
                http_only: c.get("httpOnly").and_then(|x| x.as_bool()).unwrap_or(false),
            });
        }
        Ok(out)
    }

    fn set_cookies(&mut self, cookies: &[StoredCookie]) -> Result<()> {
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

    fn close(&mut self) {
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
                "浏览器进程提前退出({}),可能有同配置的浏览器实例在运行",
                status
            ));
        }
        if Instant::now() > deadline {
            return Err(anyhow!("等待浏览器调试端口超时"));
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
            return Err(anyhow!("等待浏览器页面就绪超时"));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Ensures the user-data directory exists and carries a display name, so the
/// browser does not label our profile as "unSpecified".
fn prepare_profile(dir: &str) -> Result<()> {
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
    if !has_name {
        def["name"] = json!("SEU劳动教育助手");
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
