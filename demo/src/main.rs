//! Orbit: a small, playable multi-window tray app built entirely from the
//! reusable crates.
//!
//! * `traykit` draws the icon and the popup, whose rows are a live task list of
//!   the open windows (each row is a toggle).
//! * `webmsg` serves one page over loopback and bridges the pages to this host.
//! * `websurface` shows that page in as many windows as the user opens.
//! * the host owns the state (theme, missions, focus timer); every window is a
//!   view of it, kept in sync by pushed `state` events.
//!
//! Run: `cargo run -p orbit-demo` (`ORBIT_ENGINE=browser|webview` to force an
//! engine, `ORBIT_DATA_DIR=...` to relocate profiles).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Value};

use websurface::{Engine, SurfaceConfig, SurfaceId, WebHost, WebHostHandle};

const ICON_ICO: &[u8] = include_bytes!("../assets/icon.ico");
const NAMESPACE: &str = "orbit";
const TITLE: &str = "Orbit";

fn main() {
    if let Err(e) = run() {
        note(&format!("Orbit demo stopped with an error: {e}"));
        std::process::exit(1);
    }
}

/// Prints when a console is attached (a no-op error otherwise); never panics,
/// which matters in a GUI-subsystem build where stderr may not exist.
fn note(msg: &str) {
    let _ = writeln!(std::io::stderr(), "{msg}");
}

// ------------------------------------------------------------------- missions

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Mission {
    Hub,
    Timer,
    Lab,
    Bubbles,
}

impl Mission {
    const ALL: [Mission; 4] = [Mission::Hub, Mission::Timer, Mission::Lab, Mission::Bubbles];

    fn slug(self) -> &'static str {
        match self {
            Mission::Hub => "hub",
            Mission::Timer => "timer",
            Mission::Lab => "lab",
            Mission::Bubbles => "bubbles",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mission::Hub => "控制台",
            Mission::Timer => "专注计时",
            Mission::Lab => "调色实验室",
            Mission::Bubbles => "星尘",
        }
    }

    fn title(self) -> String {
        format!("{TITLE} · {}", self.name())
    }

    fn size(self) -> (i32, i32) {
        match self {
            Mission::Hub => (940, 660),
            Mission::Timer => (520, 640),
            Mission::Lab => (780, 660),
            Mission::Bubbles => (760, 580),
        }
    }

    fn from_slug(slug: &str) -> Option<Mission> {
        Mission::ALL.into_iter().find(|m| m.slug() == slug)
    }
}

// ---------------------------------------------------------------------- state

#[derive(Clone, Copy, Serialize)]
struct Theme {
    h: u16,
    s: u8,
    l: u8,
}

#[derive(Clone, Copy)]
struct TimerState {
    running: bool,
    ends_at: u64,
    paused_remaining: u64,
    duration_ms: u64,
}

impl TimerState {
    fn idle(duration_ms: u64) -> Self {
        TimerState {
            running: false,
            ends_at: 0,
            paused_remaining: duration_ms,
            duration_ms,
        }
    }
}

#[derive(Clone, Serialize)]
struct LogEntry {
    t: u64,
    text: String,
}

struct Inner {
    theme: Theme,
    windows: HashMap<Mission, SurfaceId>,
    timer: TimerState,
    log: VecDeque<LogEntry>,
    started: Instant,
}

impl Inner {
    fn new() -> Self {
        Inner {
            theme: Theme { h: 265, s: 82, l: 64 },
            windows: HashMap::new(),
            timer: TimerState::idle(25 * 60 * 1000),
            log: VecDeque::new(),
            started: Instant::now(),
        }
    }
}

struct App {
    data_dir: PathBuf,
    engine: Engine,
    server: OnceLock<Arc<webmsg::Server>>,
    handle: OnceLock<WebHostHandle>,
    inner: Mutex<Inner>,
    /// Serializes window opens/closes without holding `inner` across a slow
    /// (browser-launching) call, so the tray menu never stalls.
    window_op: Mutex<()>,
    quit: AtomicBool,
}

impl App {
    fn new() -> App {
        let data_dir = std::env::var_os("ORBIT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir)
                    .join("OrbitDemo")
            });
        App {
            data_dir,
            engine: engine_from_env(),
            server: OnceLock::new(),
            handle: OnceLock::new(),
            inner: Mutex::new(Inner::new()),
            window_op: Mutex::new(()),
            quit: AtomicBool::new(false),
        }
    }
}

fn engine_from_env() -> Engine {
    match std::env::var("ORBIT_ENGINE").ok().as_deref().map(str::trim) {
        Some("browser") | Some("borrowed") => Engine::Borrowed,
        Some("webview") | Some("webview2") => Engine::WebView2,
        _ => Engine::Auto,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn push_log(inner: &mut Inner, text: impl Into<String>) {
    inner.log.push_back(LogEntry {
        t: now_ms(),
        text: text.into(),
    });
    while inner.log.len() > 60 {
        inner.log.pop_front();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TimerView {
    running: bool,
    ends_at: Option<u64>,
    paused_remaining: Option<u64>,
    duration_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    now: u64,
    uptime_ms: u64,
    theme: Theme,
    windows: Vec<&'static str>,
    peer_count: usize,
    timer: TimerView,
    log: Vec<LogEntry>,
}

fn snapshot(app: &App) -> Value {
    let now = now_ms();
    let inner = app.inner.lock().unwrap();
    let timer = TimerView {
        running: inner.timer.running,
        ends_at: inner.timer.running.then_some(inner.timer.ends_at),
        paused_remaining: (!inner.timer.running).then_some(inner.timer.paused_remaining),
        duration_ms: inner.timer.duration_ms,
    };
    let windows = Mission::ALL
        .iter()
        .filter(|m| inner.windows.contains_key(m))
        .map(|m| m.slug())
        .collect();
    let snap = Snapshot {
        now,
        uptime_ms: inner.started.elapsed().as_millis() as u64,
        theme: inner.theme,
        windows,
        peer_count: inner.windows.len(),
        timer,
        log: inner.log.iter().cloned().collect(),
    };
    serde_json::to_value(snap).unwrap_or(Value::Null)
}

fn broadcast(app: &App) {
    if let Some(handle) = app.handle.get() {
        handle.broadcast("state", snapshot(app));
    }
}

// -------------------------------------------------------------- window routing

fn mission_config(app: &App, mission: Mission) -> Result<SurfaceConfig> {
    let server = app
        .server
        .get()
        .ok_or_else(|| anyhow!("the bridge is not ready"))?;
    let (w, h) = mission.size();
    Ok(SurfaceConfig {
        url: format!("{}#{}", server.url(), mission.slug()),
        title: mission.title(),
        logical_width: w,
        logical_height: h,
        min_width: (w * 3 / 4).max(360),
        min_height: (h * 3 / 4).max(320),
        icon_ico: ICON_ICO,
        data_dir: app.data_dir.join(mission.slug()),
        browser_override: None,
        profile_name: mission.title(),
        chromeless: true,
        zoom: 1.0,
        zoomable: false,
    })
}

fn start_mission(app: &App, mission: Mission) -> Result<()> {
    let _op = app.window_op.lock().unwrap();
    if app.inner.lock().unwrap().windows.contains_key(&mission) {
        return Ok(());
    }
    let cfg = mission_config(app, mission)?;
    let handle = app
        .handle
        .get()
        .ok_or_else(|| anyhow!("the host is not running"))?
        .clone();
    let id = handle.open(cfg, app.engine)?;
    {
        let mut inner = app.inner.lock().unwrap();
        inner.windows.insert(mission, id);
        push_log(&mut inner, format!("{} 已打开", mission.name()));
    }
    broadcast(app);
    Ok(())
}

fn stop_mission(app: &App, mission: Mission) {
    let id = app.inner.lock().unwrap().windows.remove(&mission);
    let Some(id) = id else { return };
    if let Some(handle) = app.handle.get() {
        handle.close(id);
    }
    {
        let mut inner = app.inner.lock().unwrap();
        push_log(&mut inner, format!("{} 已关闭", mission.name()));
    }
    broadcast(app);
}

fn open_hub(app: &App) {
    let id = app.inner.lock().unwrap().windows.get(&Mission::Hub).copied();
    match id {
        Some(id) => {
            if let Some(handle) = app.handle.get() {
                handle.bring_to_front(id);
            }
        }
        None => {
            let _ = start_mission(app, Mission::Hub);
        }
    }
}

fn stop_all_windows(app: &App) {
    let missions: Vec<Mission> = app.inner.lock().unwrap().windows.keys().copied().collect();
    for mission in missions {
        stop_mission(app, mission);
    }
}

fn random_theme(app: &App) {
    let mut seed = now_ms() ^ ((std::process::id() as u64) << 17) | 1;
    let h = (rand_u64(&mut seed) % 360) as u16;
    let s = (60 + rand_u64(&mut seed) % 34) as u8;
    let l = (56 + rand_u64(&mut seed) % 16) as u8;
    {
        let mut inner = app.inner.lock().unwrap();
        inner.theme = Theme { h, s, l };
        push_log(&mut inner, format!("主题切换到色相 {h}°"));
    }
    broadcast(app);
}

fn rand_u64(seed: &mut u64) -> u64 {
    let mut x = *seed;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *seed = x;
    x
}

// ----------------------------------------------------------------- focus timer

fn timer_start(app: &App, data: &Value) {
    let minutes = data
        .get("minutes")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 180);
    let duration = minutes * 60 * 1000;
    let mut inner = app.inner.lock().unwrap();
    let resume = !inner.timer.running
        && inner.timer.paused_remaining > 0
        && inner.timer.paused_remaining < inner.timer.duration_ms;
    if !resume {
        inner.timer.duration_ms = duration;
    }
    let remaining = if resume {
        inner.timer.paused_remaining
    } else {
        duration
    };
    inner.timer.running = true;
    inner.timer.ends_at = now_ms() + remaining;
    push_log(
        &mut inner,
        format!("专注开始（{} 分钟）", (remaining / 60000).max(1)),
    );
    drop(inner);
    broadcast(app);
}

fn timer_pause(app: &App) {
    {
        let mut inner = app.inner.lock().unwrap();
        if inner.timer.running {
            let remaining = inner.timer.ends_at.saturating_sub(now_ms());
            inner.timer.running = false;
            inner.timer.paused_remaining = remaining.max(1000);
            push_log(&mut inner, "专注已暂停");
        }
    }
    broadcast(app);
}

fn timer_reset(app: &App) {
    {
        let mut inner = app.inner.lock().unwrap();
        let duration = inner.timer.duration_ms;
        inner.timer = TimerState::idle(duration);
        push_log(&mut inner, "专注已重置");
    }
    broadcast(app);
}

// ------------------------------------------------------------------- heartbeat

fn spawn_heartbeat(app: &Arc<App>) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        reconcile_windows(&app);
        {
            let mut inner = app.inner.lock().unwrap();
            if inner.timer.running && now_ms() >= inner.timer.ends_at {
                inner.timer.running = false;
                inner.timer.paused_remaining = inner.timer.duration_ms;
                let minutes = inner.timer.duration_ms / 60000;
                push_log(&mut inner, format!("专注完成（{} 分钟）", minutes));
            }
        }
        broadcast(&app);
    });
}

/// A user closing a window with the native title-bar button is invisible to the
/// host API, so periodically drop missions whose surface is gone.
fn reconcile_windows(app: &Arc<App>) {
    let Some(handle) = app.handle.get() else { return };
    let live = handle.live_ids();
    let mut inner = app.inner.lock().unwrap();
    let closed: Vec<Mission> = inner
        .windows
        .iter()
        .filter(|(_, id)| !live.contains(id))
        .map(|(m, _)| *m)
        .collect();
    for mission in closed {
        inner.windows.remove(&mission);
        push_log(&mut inner, format!("{} 已关闭", mission.name()));
    }
}

// -------------------------------------------------------------- bridge handler

fn handle_request(app: Arc<App>, name: &str, data: Value) -> Result<Value> {
    match name {
        "state" => Ok(snapshot(&app)),
        "start" => {
            start_mission(&app, mission_from(&data)?)?;
            Ok(snapshot(&app))
        }
        "stop" => {
            stop_mission(&app, mission_from(&data)?);
            Ok(snapshot(&app))
        }
        "stopAll" => {
            stop_all_windows(&app);
            Ok(snapshot(&app))
        }
        "openHub" => {
            open_hub(&app);
            Ok(snapshot(&app))
        }
        "setTheme" => {
            let theme = theme_from(&data);
            app.inner.lock().unwrap().theme = theme;
            broadcast(&app);
            Ok(snapshot(&app))
        }
        "randomTheme" => {
            random_theme(&app);
            Ok(snapshot(&app))
        }
        "say" => {
            let text = data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if !text.is_empty() {
                push_log(&mut app.inner.lock().unwrap(), text);
                broadcast(&app);
            }
            Ok(snapshot(&app))
        }
        "timerStart" => {
            timer_start(&app, &data);
            Ok(snapshot(&app))
        }
        "timerPause" => {
            timer_pause(&app);
            Ok(snapshot(&app))
        }
        "timerReset" => {
            timer_reset(&app);
            Ok(snapshot(&app))
        }
        "quit" => {
            request_quit(&app);
            Ok(json!({ "ok": true }))
        }
        other => Err(anyhow!("unknown method: {other}")),
    }
}

fn mission_from(data: &Value) -> Result<Mission> {
    let slug = data.get("id").and_then(Value::as_str).unwrap_or("");
    Mission::from_slug(slug).ok_or_else(|| anyhow!("unknown mission: {slug}"))
}

fn theme_from(data: &Value) -> Theme {
    let h = (data.get("h").and_then(Value::as_u64).unwrap_or(265) % 360) as u16;
    let s = data.get("s").and_then(Value::as_u64).unwrap_or(82).clamp(30, 100) as u8;
    let l = data.get("l").and_then(Value::as_u64).unwrap_or(64).clamp(40, 78) as u8;
    Theme { h, s, l }
}

fn request_quit(app: &App) {
    app.quit.store(true, Ordering::SeqCst);
    if let Some(handle) = app.handle.get() {
        handle.wake();
    }
}

// ------------------------------------------------------------------------ tray

mod cmd {
    pub const OPEN_HUB: u32 = 1;
    pub const RANDOM_THEME: u32 = 2;
    pub const STOP_ALL: u32 = 3;
    pub const QUIT: u32 = 10;
    pub const TOGGLE_BASE: u32 = 100;
}

fn build_tray(app: &Arc<App>) -> traykit::TrayConfig {
    let a = app.clone();
    let tooltip: traykit::StringFn = Arc::new(move || {
        let inner = a.inner.lock().unwrap();
        format!(
            "{TITLE} · {} 个窗口 · 色相 {}°",
            inner.windows.len(),
            inner.theme.h
        )
    });
    let a = app.clone();
    let items: traykit::ItemsFn = Arc::new(move || tray_items(&a));
    let a = app.clone();
    let on_command: traykit::CommandFn = Arc::new(move |id| tray_command(&a, id));
    let a = app.clone();
    let on_left_click: traykit::Callback = Arc::new(move || open_hub(&a));
    traykit::TrayConfig {
        icon_ico: ICON_ICO,
        tooltip,
        items,
        on_command,
        on_left_click,
    }
}

fn tray_items(app: &Arc<App>) -> Vec<traykit::MenuItem> {
    use traykit::MenuItem;
    let mut v = Vec::new();
    {
        let inner = app.inner.lock().unwrap();
        let up = inner.started.elapsed().as_secs();
        v.push(MenuItem::info(format!(
            "运行中 {} 个窗口 · 已运行 {}:{:02}",
            inner.windows.len(),
            up / 60,
            up % 60
        )));
        v.push(MenuItem::info(format!("主题色相 {}°（所有窗口实时同步）", inner.theme.h)));
    }
    v.push(MenuItem::separator());
    for (i, mission) in Mission::ALL.iter().enumerate() {
        let m = *mission;
        let a = app.clone();
        let checked: traykit::BoolFn =
            Arc::new(move || a.inner.lock().unwrap().windows.contains_key(&m));
        v.push(MenuItem::toggle(
            cmd::TOGGLE_BASE + i as u32,
            m.name(),
            checked,
        ));
    }
    v.push(MenuItem::separator());
    v.push(MenuItem::command(cmd::OPEN_HUB, "控制台置顶", 0xE71D));
    v.push(MenuItem::command(cmd::RANDOM_THEME, "随机主题", 0xE790));
    v.push(MenuItem::command(cmd::STOP_ALL, "关闭所有窗口", 0xE8BB));
    v.push(MenuItem::separator());
    v.push(MenuItem::info(format!("{TITLE} 多窗口演示")));
    v.push(MenuItem::command(cmd::QUIT, "退出", 0xE7E8));
    v
}

fn tray_command(app: &Arc<App>, id: u32) {
    if id >= cmd::TOGGLE_BASE {
        if let Some(mission) = Mission::ALL.get((id - cmd::TOGGLE_BASE) as usize).copied() {
            if app.inner.lock().unwrap().windows.contains_key(&mission) {
                stop_mission(app, mission);
            } else {
                let _ = start_mission(app, mission);
            }
        }
        return;
    }
    match id {
        cmd::OPEN_HUB => open_hub(app),
        cmd::RANDOM_THEME => random_theme(app),
        cmd::STOP_ALL => stop_all_windows(app),
        cmd::QUIT => request_quit(app),
        _ => {}
    }
}

// ------------------------------------------------------------------------ main

fn run() -> Result<()> {
    let app = Arc::new(App::new());

    let handler_app = app.clone();
    let server = webmsg::serve(
        webmsg::Config {
            namespace: NAMESPACE.to_string(),
            title: TITLE.to_string(),
            index_html: include_str!("../assets/index.html").to_string(),
            icon_ico: ICON_ICO,
            extra_csp: "",
        },
        Arc::new(move |name, data| handle_request(handler_app.clone(), name, data)),
    )?;
    let _ = app.server.set(server);

    let mut host = WebHost::new()?;
    let _ = app.handle.set(host.handle());

    // The control room is opened synchronously: the loop is not running yet, so
    // `WebHostHandle::open` (which needs the loop) must not be used here.
    {
        let cfg = mission_config(&app, Mission::Hub)?;
        let id = host.open(cfg, app.engine)?;
        let mut inner = app.inner.lock().unwrap();
        inner.windows.insert(Mission::Hub, id);
        push_log(&mut inner, "控制台已就绪");
    }

    // Pop a couple more windows right after launch (via the handle, once the
    // loop is running) so the multi-window nature is obvious immediately.
    {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            for mission in [Mission::Timer, Mission::Lab] {
                if let Err(e) = start_mission(&app, mission) {
                    note(&format!("could not open {}: {e}", mission.name()));
                }
            }
        });
    }

    let tray = match traykit::Tray::new(build_tray(&app)) {
        Ok(t) => Some(t),
        Err(e) => {
            note(&format!("tray icon unavailable, running window-only: {e}"));
            None
        }
    };

    spawn_heartbeat(&app);
    broadcast(&app);

    let quit_app = app.clone();
    if tray.is_some() {
        host.run_until(move || quit_app.quit.load(Ordering::SeqCst))?;
        tray.as_ref().unwrap().stop();
    } else {
        host.run()?;
    }

    // Borrowed-browser windows own their profile directories; reap them so no
    // browser lingers after the demo exits.
    browserhost::kill_for_profile(&app.data_dir.to_string_lossy());
    Ok(())
}
