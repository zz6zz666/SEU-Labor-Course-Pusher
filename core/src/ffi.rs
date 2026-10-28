//! JNI surface for the Android shell. Every call is JSON in / JSON out so the
//! Kotlin side stays a thin renderer and the core remains testable on desktop.
//!
//! The Kotlin class is `com.zz6zz666.seulabor.CoreBridge`; keep its package and
//! name stable or the exported symbols stop resolving.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jni::objects::{JObject, JString};
use jni::sys::{jboolean, jstring};
use jni::JNIEnv;
use serde_json::json;

use crate::cas;
use crate::config;
use crate::js;
use crate::logging;
use crate::notify::collect::CollectingChannel;
use crate::notify::event;
use crate::notify::pushplus::PushPlus;
use crate::notify::{Channel, Dispatcher};
use crate::selection;
use crate::session;
use crate::site;
use crate::state;
use crate::state::AuthState;
use crate::watcher;

struct App {
    store: Arc<config::Store>,
    state: Arc<state::Store>,
    client: Arc<session::Client>,
    watcher: Arc<watcher::Watcher>,
    events: CollectingChannel,
    tick_mu: Mutex<()>,
}

static APP: OnceLock<Mutex<Option<Arc<App>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<App>>> {
    APP.get_or_init(|| Mutex::new(None))
}

fn current() -> Option<Arc<App>> {
    slot().lock().unwrap().clone()
}

fn init_app(dir: &str) -> anyhow::Result<Arc<App>> {
    let data_dir = PathBuf::from(dir);
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(data_dir.join("logs"))?;

    let (store, warnings) = config::Store::open(data_dir.join("config.json"))?;
    let cfg = store.get();
    logging::init(
        data_dir.join("logs"),
        logging::Level::parse(&cfg.logging.level),
        cfg.logging.retention_days as i64,
    );
    let log = logging::new("android");
    for w in warnings {
        log.warn(&w);
    }

    let st = state::Store::open(data_dir.join("state.json"))?;
    let client = Arc::new(session::Client::new(
        data_dir.join("cookies.json"),
        Duration::from_secs(20),
    )?);

    let events = CollectingChannel::new();
    let mut dispatcher = Dispatcher::new(logging::new("notify"));
    dispatcher.add(Box::new(PushPlus::new(
        store.clone(),
        logging::new("pushplus"),
    )));
    dispatcher.add(Box::new(events.clone()));

    let selector = Arc::new(selection::Runner {
        client: client.clone(),
    });
    let w = watcher::Watcher::new(
        store.clone(),
        st.clone(),
        client.clone(),
        Arc::new(dispatcher),
        logging::new("watcher"),
        Some(selector),
    );
    let collector = events.clone();
    w.set_on_auth_expired(Arc::new(move |reason: String| {
        let _ = collector.send(&event::build_auth_expired(&reason));
    }));

    Ok(Arc::new(App {
        store,
        state: st,
        client,
        watcher: w,
        events,
        tick_mu: Mutex::new(()),
    }))
}

fn utf8(env: &mut JNIEnv<'_>, s: &JString<'_>) -> Option<String> {
    env.get_string(s).ok().map(|v| v.into())
}

fn out(env: &mut JNIEnv<'_>, s: String) -> jstring {
    env.new_string(s)
        .map(|v| v.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn status_json(app: &App) -> serde_json::Value {
    json!({
        "status": app.watcher.status().to_json(),
        "authState": app.state.get().auth_state.as_str(),
        "firstRunCompleted": app.state.get().first_run_completed,
        "config": app.store.get().safe_summary(),
        "cookieCount": app.client.jar.all().len(),
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeInit(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    dir: JString<'_>,
) -> jboolean {
    let Some(dir) = utf8(&mut env, &dir) else {
        return 0;
    };
    if current().is_some() {
        return 1;
    }
    match init_app(&dir) {
        Ok(app) => {
            *slot().lock().unwrap() = Some(app);
            1
        }
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeTick(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    let Some(app) = current() else {
        return out(&mut env, json!({ "error": "core not initialized" }).to_string());
    };
    let json = catch_unwind(AssertUnwindSafe(|| {
        let _guard = app.tick_mu.lock().unwrap_or_else(|e| e.into_inner());
        app.watcher.tick();
        let delay = app.watcher.next_delay();
        app.watcher.mark_next_run(delay);
        let events = app.events.drain();
        json!({
            "status": app.watcher.status().to_json(),
            "events": events,
            "authState": app.state.get().auth_state.as_str(),
            "nextDelayMs": delay.as_millis() as i64,
        })
    }))
    .unwrap_or_else(|_| json!({ "error": "tick panicked" }));
    out(&mut env, json.to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeStatus(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    let Some(app) = current() else {
        return out(&mut env, json!({ "error": "core not initialized" }).to_string());
    };
    out(&mut env, status_json(&app).to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeProbe(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    let Some(app) = current() else {
        return out(&mut env, json!({ "error": "core not initialized" }).to_string());
    };
    let json = match site::check(&app.client) {
        Ok(p) => {
            let verdict = match p.verdict {
                site::Verdict::Courses => {
                    let _ = app.state.update(|s| {
                        s.first_run_completed = true;
                        s.auth_state = AuthState::Valid;
                    });
                    "courses"
                }
                site::Verdict::Login => {
                    let _ = app.state.set_auth_state(AuthState::Expired);
                    "login"
                }
                site::Verdict::Unknown => "unknown",
            };
            json!({ "verdict": verdict, "reason": p.reason })
        }
        Err(e) => json!({ "verdict": "error", "reason": e.to_string() }),
    };
    out(&mut env, json.to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeGetConfig(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    let Some(app) = current() else {
        return out(&mut env, "{}".to_string());
    };
    let raw = serde_json::to_string(&app.store.get()).unwrap_or_else(|_| "{}".to_string());
    out(&mut env, raw)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeSetConfig(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    raw: JString<'_>,
) -> jstring {
    let Some(app) = current() else {
        return out(&mut env, json!({ "ok": false, "error": "core not initialized" }).to_string());
    };
    let Some(raw) = utf8(&mut env, &raw) else {
        return out(&mut env, json!({ "ok": false, "error": "bad string" }).to_string());
    };
    let result = serde_json::from_str::<config::Config>(&raw)
        .map_err(|e| anyhow::anyhow!("配置不是合法 JSON: {}", e))
        .and_then(|cfg| app.store.replace(cfg));
    let json = match result {
        Ok(warnings) => json!({ "ok": true, "warnings": warnings }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };
    out(&mut env, json.to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeImportCookies(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    raw: JString<'_>,
) -> jboolean {
    let Some(app) = current() else {
        return 0;
    };
    let Some(raw) = utf8(&mut env, &raw) else {
        return 0;
    };
    let Ok(cookies) = serde_json::from_str::<Vec<session::jar::StoredCookie>>(&raw) else {
        return 0;
    };
    let _guard = app.tick_mu.lock().unwrap_or_else(|e| e.into_inner());
    app.client.jar.set_stored(&cookies);
    if app.client.jar.save().is_ok() {
        1
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeExportCookies(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    let Some(app) = current() else {
        return out(&mut env, "[]".to_string());
    };
    let raw = serde_json::to_string(&app.client.jar.all()).unwrap_or_else(|_| "[]".to_string());
    out(&mut env, raw)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeAutoLogin(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    user: JString<'_>,
    pass: JString<'_>,
) -> jstring {
    let Some(app) = current() else {
        return out(&mut env, json!({ "outcome": "error", "detail": "core not initialized" }).to_string());
    };
    let (Some(user), Some(pass)) = (utf8(&mut env, &user), utf8(&mut env, &pass)) else {
        return out(&mut env, json!({ "outcome": "error", "detail": "bad string" }).to_string());
    };
    let json = catch_unwind(AssertUnwindSafe(|| {
        let _guard = app.tick_mu.lock().unwrap_or_else(|e| e.into_inner());
        let res = cas::auto_run(&user, &pass, &logging::new("login"));
        if res.outcome == cas::AutoOutcome::Success {
            app.client.jar.set_stored(&res.cookies);
            let _ = app.client.jar.save();
            let _ = app.state.update(|s| {
                s.first_run_completed = true;
                s.auth_state = AuthState::Valid;
            });
        }
        json!({
            "outcome": res.outcome.as_str(),
            "detail": res.detail,
        })
    }))
    .unwrap_or_else(|_| json!({ "outcome": "error", "detail": "login panicked" }));
    out(&mut env, json.to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeReadyScript(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    out(&mut env, js::ready_script())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zz6zz666_seulabor_CoreBridge_nativeFillScript(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    user: JString<'_>,
    pass: JString<'_>,
) -> jstring {
    let (Some(user), Some(pass)) = (utf8(&mut env, &user), utf8(&mut env, &pass)) else {
        return out(&mut env, String::new());
    };
    out(&mut env, js::fill_credentials_script(&user, &pass))
}
