//! The settings model and the bridge wiring for the hosted settings page.
//!
//! The page itself is served and driven by `webmsg`; this module only declares
//! the host's action table and maps the page's messages onto it, so no
//! transport or HTTP details live here.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::Serialize;

use crate::assets;
use crate::brand;
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

pub struct Actions {
    pub status: Box<dyn Fn() -> Status + Send + Sync>,
    pub config: Box<dyn Fn() -> ConfigView + Send + Sync>,
    pub verify: Box<dyn Fn() -> (String, String) + Send + Sync>,
    pub save_account: Box<dyn Fn(&str, &str) -> Result<()> + Send + Sync>,
    pub save_filters: Box<dyn Fn(&[String], &[String]) -> Result<()> + Send + Sync>,
    pub save_notify: Box<dyn Fn(&str, bool, bool) -> Result<()> + Send + Sync>,
    pub set_behavior: Box<dyn Fn(Option<bool>, Option<bool>) -> Result<()> + Send + Sync>,
    pub open_login: Box<dyn Fn() + Send + Sync>,
    pub open_course: Box<dyn Fn() + Send + Sync>,
    pub open_config: Box<dyn Fn() + Send + Sync>,
    pub open_logs: Box<dyn Fn() + Send + Sync>,
    pub open_external: Box<dyn Fn(&str) -> Result<()> + Send + Sync>,
}

/// Starts the loopback settings bridge. It opens no window itself; a surface
/// (WebView2 or a borrowed browser) loads the returned URL.
pub fn open(actions: Actions) -> Result<Arc<webmsg::Server>> {
    let a = Arc::new(actions);
    let handler: webmsg::Handler = Arc::new(move |name, data| dispatch(&a, name, data));
    webmsg::serve(
        webmsg::Config {
            // Keep the page's original `window.seuWizard` namespace.
            namespace: "seuWizard".to_string(),
            title: brand::TITLE.to_string(),
            index_html: web::WIZARD_HTML.to_string(),
            icon_ico: assets::ICON_ICO,
            extra_csp: "connect-src 'self'; img-src 'self' data:;",
        },
        handler,
    )
}

fn dispatch(a: &Arc<Actions>, name: &str, data: serde_json::Value) -> Result<serde_json::Value> {
    match name {
        "getStatus" => Ok(serde_json::to_value((a.status)())?),
        "getConfig" => Ok(serde_json::to_value((a.config)())?),
        "verify" => {
            let (state, reason) = (a.verify)();
            Ok(serde_json::json!({ "state": state, "reason": reason }))
        }
        "saveAccount" => Ok(result_json((a.save_account)(
            &str_of(&data, "username"),
            &str_of(&data, "password"),
        ))),
        "saveFilters" => Ok(result_json((a.save_filters)(
            &arr_of(&data, "locationWhitelist"),
            &arr_of(&data, "categoryBlacklist"),
        ))),
        "saveNotify" => {
            let token = str_of(&data, "pushplusToken");
            let pp = data
                .get("pushplusEnabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let wn = data
                .get("windowsNotifyEnabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            Ok(result_json((a.save_notify)(&token, pp, wn)))
        }
        "setBehavior" => {
            let auto_start = data.get("autoStart").and_then(|x| x.as_bool());
            let auto_select = data.get("autoSelect").and_then(|x| x.as_bool());
            Ok(result_json((a.set_behavior)(auto_start, auto_select)))
        }
        "openLogin" => {
            (a.open_login)();
            Ok(serde_json::json!({ "ok": true }))
        }
        "openCourse" => {
            (a.open_course)();
            Ok(serde_json::json!({ "ok": true }))
        }
        "openConfig" => {
            (a.open_config)();
            Ok(serde_json::json!({ "ok": true }))
        }
        "openLogs" => {
            (a.open_logs)();
            Ok(serde_json::json!({ "ok": true }))
        }
        "openExternal" => {
            let url = data.as_str().unwrap_or("");
            Ok(result_json((a.open_external)(url)))
        }
        other => Err(anyhow!("未知的设置请求: {}", other)),
    }
}

/// Business errors travel as data (`{ok:false,error}`) so the page's
/// `if (!res.ok)` check sees them instead of the call throwing.
fn result_json(r: Result<()>) -> serde_json::Value {
    match r {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }),
    }
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
