//! Command seu-labor is the resident SEU labor-course watcher (Rust port).
//!
//! Polling, auto-selection and auto-cancel use plain HTTP (the list is
//! server-rendered). The system Chromium browser is launched only for
//! interactive login and course viewing.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod autostart;
mod brand;
mod config;
mod fsutil;
mod logging;
mod login;
mod login_scripts;
mod notify;
mod osutil;
mod paths;
mod proc;
mod selection;
mod session;
mod singleinstance;
mod site;
mod state;
mod watcher;
mod web;
mod wizard;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;

use crate::logging::Logger;
use crate::notify::Dispatcher;
use crate::selection::Runner;
use crate::session::Client;
use crate::watcher::Watcher;

const VERSION: &str = match option_env!("SEU_LABOR_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

fn main() {
    let _ = winkit::set_app_user_model_id(osutil::APP_APP_USER_MODEL_ID);
    let args: Vec<String> = std::env::args().skip(1).collect();
    if has_flag(&args, "-version") {
        osutil::console_println(VERSION);
        return;
    }
    if has_flag(&args, "-wizard-ui") {
        let url = flag_value(&args, "-wiz-url").unwrap_or_default();
        let data = flag_value(&args, "-wiz-data").unwrap_or_default();
        let browser = flag_value(&args, "-wiz-browser").unwrap_or_default();
        if let Err(e) = run_wizard_ui(&url, &data, &browser) {
            osutil::console_println(&format!("错误: {}", e));
            std::process::exit(1);
        }
        return;
    }

    let once = has_flag(&args, "-once");
    let do_login = has_flag(&args, "-login");
    let show_wizard = has_flag(&args, "-wizard");

    if let Err(e) = run(once, do_login, show_wizard) {
        osutil::console_println(&format!("错误: {}", e));
        std::process::exit(1);
    }
}

fn has_flag(args: &[String], name: &str) -> bool {
    let alt = format!("-{}", name);
    args.iter().any(|a| a == name || a == &alt)
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    let alt = format!("-{}", name);
    for (i, a) in args.iter().enumerate() {
        if a == name || a == &alt {
            return args.get(i + 1).cloned();
        }
    }
    None
}

struct WizardState {
    srv: Option<Arc<webmsg::Server>>,
    proc: Option<proc::ChildProc>,
}

struct App {
    p: paths::Paths,
    store: Arc<config::Store>,
    st: Arc<state::Store>,
    client: Arc<Client>,
    w: Arc<Watcher>,
    dispatcher: Arc<Dispatcher>,
    log: Logger,
    login_mu: Mutex<()>,
    course_view: Mutex<Option<browserhost::Session>>,
    wizard: Mutex<WizardState>,
}

fn run(once: bool, do_login: bool, show_wizard: bool) -> Result<()> {
    let p = paths::Paths::resolve()?;
    let (store, warnings) = config::Store::open(p.config_path.clone())?;
    let cfg = store.get();
    logging::init(
        p.log_dir.clone(),
        logging::Level::parse(&cfg.logging.level),
        cfg.logging.retention_days as i64,
    );
    let log = logging::new("main");

    log.info("================ SEU 劳动教育课程监控 ================");
    log.info(format!("版本 {} · {}", VERSION, p.data_dir.display()));
    log.info(format!("运行配置: {}", cfg.safe_summary()));
    for w in &warnings {
        log.warn(w);
    }

    // Repair the autostart entry on every launch when it is enabled.
    if cfg.behavior.auto_launch_at_login {
        let actual = autostart::apply(true);
        if actual != cfg.behavior.auto_launch_at_login {
            let _ = store.update(|c| c.behavior.auto_launch_at_login = actual);
        }
    }

    let st = state::Store::open(p.state_path.clone())?;
    let client = Arc::new(Client::new(p.cookies_path.clone(), Duration::from_secs(20))?);

    if do_login {
        return run_login(&p, &store, &client, &st, &log);
    }
    if client.jar.is_empty() {
        if cfg.behavior.auto_login && cfg.has_credentials() {
            log.info("尚无登录态,将由后台尝试静默登录");
        } else {
            log.warn("尚未登录:请先运行 `seu-labor -login` 完成一次登录");
        }
    }

    let mut dispatcher = Dispatcher::new(logging::new("notify"));
    dispatcher.add(Box::new(notify::pushplus::PushPlus::new(
        store.clone(),
        logging::new("pushplus"),
    )));
    dispatcher.add(Box::new(notify::toast::Toast::new(store.clone())));
    let dispatcher = Arc::new(dispatcher);

    let selector = Arc::new(Runner {
        client: client.clone(),
    });
    let w = Watcher::new(
        store.clone(),
        st.clone(),
        client.clone(),
        dispatcher.clone(),
        logging::new("watcher"),
        Some(selector),
    );

    let app = Arc::new(App {
        p: p.clone(),
        store: store.clone(),
        st: st.clone(),
        client: client.clone(),
        w: w.clone(),
        dispatcher: dispatcher.clone(),
        log: log.clone(),
        login_mu: Mutex::new(()),
        course_view: Mutex::new(None),
        wizard: Mutex::new(WizardState {
            srv: None,
            proc: None,
        }),
    });

    if once {
        w.tick();
        let s = w.status();
        log.info(format!("单次抓取结果: {} - {}", s.last_verdict, s.last_message));
        let _ = client.jar.save();
        return Ok(());
    }

    let (inst, already) = singleinstance::acquire();
    if already {
        log.info("检测到已有实例在运行,请求其打开设置向导后退出");
        singleinstance::signal_existing();
        return Ok(());
    }

    let tray = match traykit::Tray::new(build_tray(&app)) {
        Ok(t) => Some(t),
        Err(e) => {
            log.warn(format!("创建系统托盘失败,将以无界面方式常驻: {}", e));
            None
        }
    };

    // Silent re-login on auth expiry, then manual fallback.
    {
        let app = app.clone();
        w.set_on_auth_expired(Arc::new(move |reason: String| {
            let app = app.clone();
            std::thread::spawn(move || {
                if try_silent_login(&app) {
                    app.w.trigger_now();
                    return;
                }
                app.dispatcher
                    .dispatch(&notify::event::build_auth_expired(&reason));
                if app.store.get().behavior.auto_open_on_auth_failure {
                    app.log.info("静默重登不可行,自动打开登录窗口");
                    open_login_window(&app);
                }
            });
        }));
    }

    // The polling loop.
    {
        let w = w.clone();
        std::thread::spawn(move || w.run());
    }

    // While expired, retry the unattended login periodically.
    {
        let app = app.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(300));
            if app.st.get().auth_state != state::AuthState::Expired {
                continue;
            }
            if try_silent_login(&app) {
                app.w.trigger_now();
            }
        });
    }

    // Config hot reload.
    {
        let app = app.clone();
        let tray = tray;
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            let (changed, warns) = app.store.reload();
            if !changed {
                continue;
            }
            let cfg = app.store.get();
            logging::configure(
                logging::Level::parse(&cfg.logging.level),
                cfg.logging.retention_days as i64,
            );
            app.log.info(format!("配置已热重载 {}", cfg.safe_summary()));
            for x in warns {
                app.log.warn(x);
            }
            if let Some(t) = &tray {
                t.update();
            }
        });
    }

    if let Some(tray) = tray {
        let t = tray;
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(3));
            t.update();
        });
    }

    if !st.get().first_run_completed {
        log.info("首次运行,打开设置向导");
        open_wizard(&app);
    } else if show_wizard {
        log.info("按启动参数打开设置向导");
        open_wizard(&app);
    }

    if let Some(inst) = inst {
        let app = app.clone();
        std::thread::spawn(move || loop {
            inst.wait();
            app.log.info("收到另一实例的请求,打开设置向导");
            open_wizard(&app);
        });
    }

    if let Some(tray) = tray {
        tray.run();
        Ok(())
    } else {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
}

// ------------------------------------------------------------------ app glue

fn status_lines(app: &Arc<App>) -> Vec<String> {
    let s = app.w.status();
    let login = match app.st.get().auth_state {
        state::AuthState::Valid => "登录正常",
        state::AuthState::Expired => "登录失效",
        state::AuthState::Unknown => "尚未登录",
    };
    let first = format!(
        "{} · 符合条件 {} 门 · 今日新推送 {} 门",
        login, s.current_valid, s.pushed_today
    );
    let mut second = if s.last_message.is_empty() {
        "等待首次抓取".to_string()
    } else {
        s.last_message.clone()
    };
    if let Some(t) = s.next_run {
        second.push_str(&format!("（下次 {}）", t.format("%H:%M:%S")));
    }
    vec![first, second]
}

fn try_silent_login(app: &Arc<App>) -> bool {
    let Ok(_guard) = app.login_mu.try_lock() else {
        return false;
    };
    let cfg = app.store.get();
    if !cfg.behavior.auto_login || !cfg.has_credentials() {
        return false;
    }
    let res = login::auto_run(
        &cfg.credentials.username,
        &cfg.credentials.password,
        &logging::new("login"),
    );
    if res.outcome != login::AutoOutcome::Success {
        app.log
            .warn(format!("静默重登未成功: {:?} {}", res.outcome, res.detail));
        return false;
    }
    app.client.jar.set_stored(&res.cookies);
    if let Err(e) = app.client.jar.save() {
        app.log.warn(format!("保存登录态失败: {}", e));
    }
    let _ = app.st.update(|s| {
        s.first_run_completed = true;
        s.auth_state = state::AuthState::Valid;
    });
    app.log.info("静默重登成功");
    true
}

fn open_login_window(app: &Arc<App>) {
    let cfg = app.store.get();
    let exe = cfg.browser_path().map(str::to_string);
    let cred = cfg.credentials;
    let profile = app.p.data_dir.join("browser-profile");
    let profile = profile.to_string_lossy().into_owned();
    let log = logging::new("login");
    let opts = login::LoginOptions {
        url: session::CAS_LOGIN,
        profile_dir: &profile,
        timeout: Duration::from_secs(300),
        poll_interval: Duration::from_secs(3),
        username: &cred.username,
        password: &cred.password,
        exec_path: exe.as_deref(),
        log: &log,
    };
    match login::run(&opts) {
        Ok(cookies) => {
            app.client.jar.set_stored(&cookies);
            let _ = app.client.jar.save();
            let _ = app.st.update(|s| {
                s.first_run_completed = true;
                s.auth_state = state::AuthState::Valid;
            });
            app.log.info("登录态已保存,后台现在可以直接轮询");
            app.w.trigger_now();
        }
        Err(e) => app.log.warn(format!("登录失败: {}", browser_error(&e))),
    }
}

fn open_course_view(app: &Arc<App>) {
    let mut guard = app.course_view.lock().unwrap();
    if let Some(view) = guard.as_mut() {
        // Only reuse a live window: navigating a closed browser would block.
        if view.is_alive() && view.navigate(session::COURSE_PAGE).is_ok() {
            return;
        }
        view.close();
        *guard = None;
    }

    let profile = app
        .p
        .data_dir
        .join("browser-profile-view")
        .to_string_lossy()
        .into_owned();
    let exe = app.store.get().browser_path().map(str::to_string);
    match browserhost::Session::launch(browserhost::SessionConfig {
        visible: true,
        profile_dir: profile,
        url: Some(session::COURSE_PAGE.to_string()),
        mode: browserhost::WindowMode::Browser,
        exec_path: exe,
        profile_name: brand::TITLE.to_string(),
        position: None,
    }) {
        Ok(mut win) => {
            let cookies: Vec<browserhost::Cookie> = app
                .client
                .jar
                .all()
                .iter()
                .map(|c| c.to_browser_cookie())
                .collect();
            if let Err(e) = win.set_cookies(&cookies) {
                app.log.warn(format!("注入登录态失败: {}", e));
            }
            if let Err(e) = win.navigate(session::COURSE_PAGE) {
                app.log.warn(format!("加载选课页失败: {}", e));
            }
            *guard = Some(win);
        }
        Err(e) => app.log.warn(format!("打开选课页失败: {}", browser_error(&e))),
    }
}

/// Maps `browserhost`'s "no browser found" error to actionable, localized
/// guidance; every other error is passed through unchanged.
fn browser_error(e: &anyhow::Error) -> String {
    if browserhost::is_no_browser(e) {
        "未找到可用的 Chromium 内核浏览器(Edge/Chrome 等),请在 config.json 设置 \
         browser.path 或安装 Edge/Chrome 后重试"
            .to_string()
    } else {
        e.to_string()
    }
}

fn open_wizard(app: &Arc<App>) {
    // Tear down any existing wizard server + window process. The profile also
    // covers a window hosted by a borrowed browser (the no-WebView2 case).
    let browser_profile = app
        .p
        .data_dir
        .join("wizard-webview")
        .join("browser")
        .to_string_lossy()
        .into_owned();
    {
        let mut wz = app.wizard.lock().unwrap();
        if let Some(p) = wz.proc.take() {
            p.kill();
        }
        if let Some(srv) = wz.srv.take() {
            srv.set_on_close(Box::new(|| {}));
            srv.close();
        }
    }
    browserhost::kill_for_profile(&browser_profile);

    let srv = match wizard::open(build_wizard_actions(app)) {
        Ok(s) => s,
        Err(e) => {
            app.log.warn(format!("启动设置向导失败: {}", e));
            return;
        }
    };

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            app.log
                .warn(format!("定位程序路径失败,改用默认浏览器: {}", e));
            let _ = osutil::open_url(&srv.url());
            return;
        }
    };
    let data = app
        .p
        .data_dir
        .join("wizard-webview")
        .to_string_lossy()
        .into_owned();
    let mut args = vec![
        "-wizard-ui".to_string(),
        "-wiz-url".to_string(),
        srv.url(),
        "-wiz-data".to_string(),
        data,
    ];
    // Let the settings window use the same explicit browser override as the
    // course/login windows, when one is configured.
    if let Some(bp) = app.store.get().browser_path() {
        args.push("-wiz-browser".to_string());
        args.push(bp.to_string());
    }

    match proc::spawn(&exe, &args) {
        Ok(cp) => {
            // The page's「关闭窗口」button closes the API server; also stop the
            // child and any borrowed browser hosting the window.
            {
                let app2 = app.clone();
                let bp = browser_profile.clone();
                srv.set_on_close(Box::new(move || {
                    let app = app2.clone();
                    let bp = bp.clone();
                    std::thread::spawn(move || {
                        let mut wz = app.wizard.lock().unwrap();
                        if let Some(p) = wz.proc.take() {
                            p.kill();
                        }
                        drop(wz);
                        browserhost::kill_for_profile(&bp);
                    });
                }));
            }
            // The child exiting tears the API server down.
            {
                let app2 = app.clone();
                let srv2 = srv.clone();
                let pid = cp.pid();
                let cp_thread = cp.clone();
                std::thread::spawn(move || {
                    cp_thread.wait();
                    let mut wz = app2.wizard.lock().unwrap();
                    let same = wz.proc.as_ref().map(|p| p.pid() == pid).unwrap_or(false);
                    if same {
                        wz.proc = None;
                    }
                    let srv = if same { wz.srv.take() } else { None };
                    drop(wz);
                    if let Some(srv) = srv {
                        srv.set_on_close(Box::new(|| {}));
                        srv.close();
                    }
                    let _ = srv2;
                });
            }
            let mut wz = app.wizard.lock().unwrap();
            wz.srv = Some(srv);
            wz.proc = Some(cp);
            app.log.info("设置向导已启动(独立窗口进程)");
        }
        Err(e) => {
            app.log
                .warn(format!("打开设置向导窗口失败,改用默认浏览器: {}", e));
            let _ = osutil::open_url(&srv.url());
            let mut wz = app.wizard.lock().unwrap();
            wz.srv = Some(srv);
        }
    }
}

mod tray_cmd {
    pub const OPEN_COURSE: u32 = 1;
    pub const LOGIN: u32 = 2;
    pub const FETCH_NOW: u32 = 3;
    pub const OPEN_LOGS: u32 = 4;
    pub const OPEN_CONFIG: u32 = 5;
    pub const OPEN_WIZARD: u32 = 6;
    pub const TOGGLE_AUTOSTART: u32 = 7;
    pub const TOGGLE_AUTOSELECT: u32 = 8;
    pub const QUIT: u32 = 9;
}

fn build_tray(app: &Arc<App>) -> traykit::TrayConfig {
    let a = app.clone();
    let tooltip: traykit::StringFn =
        Arc::new(move || status_lines(&a).into_iter().next().unwrap_or_default());
    let a = app.clone();
    let items: traykit::ItemsFn = Arc::new(move || tray_items(&a));
    let a = app.clone();
    let on_command: traykit::CommandFn = Arc::new(move |id| tray_command(&a, id));
    let a = app.clone();
    let on_left_click: traykit::Callback = Arc::new(move || {
        let a = a.clone();
        std::thread::spawn(move || open_course_view(&a));
    });
    traykit::TrayConfig {
        icon_ico: assets::ICON_ICO,
        tooltip,
        items,
        on_command,
        on_left_click,
    }
}

fn tray_items(app: &Arc<App>) -> Vec<traykit::MenuItem> {
    use traykit::MenuItem;
    let mut v = Vec::new();
    for line in status_lines(app) {
        if !line.is_empty() {
            v.push(MenuItem::info(line));
        }
    }
    v.push(MenuItem::separator());
    v.push(MenuItem::command(tray_cmd::OPEN_COURSE, "打开选课页", 0xE774));
    v.push(MenuItem::command(tray_cmd::LOGIN, "重新登录", 0xE72E));
    v.push(MenuItem::separator());
    v.push(MenuItem::command(tray_cmd::FETCH_NOW, "立即抓取一次", 0xE72C));
    v.push(MenuItem::command(tray_cmd::OPEN_LOGS, "打开日志目录", 0xE8B7));
    v.push(MenuItem::command(
        tray_cmd::OPEN_CONFIG,
        "打开 config.json",
        0xE8A5,
    ));
    v.push(MenuItem::command(tray_cmd::OPEN_WIZARD, "设置向导", 0xE713));
    v.push(MenuItem::separator());
    v.push(MenuItem::toggle(
        tray_cmd::TOGGLE_AUTOSTART,
        "开机自启",
        Arc::new(autostart::is_enabled),
    ));
    let a = app.clone();
    v.push(MenuItem::toggle(
        tray_cmd::TOGGLE_AUTOSELECT,
        "自动选课",
        Arc::new(move || a.store.get().behavior.auto_select),
    ));
    v.push(MenuItem::separator());
    if !VERSION.is_empty() {
        v.push(MenuItem::info(format!("版本 {}", VERSION)));
    }
    v.push(MenuItem::command(tray_cmd::QUIT, "退出", 0xE8BB));
    v
}

fn tray_command(app: &Arc<App>, id: u32) {
    match id {
        tray_cmd::OPEN_COURSE => open_course_view(app),
        tray_cmd::LOGIN => open_login_window(app),
        tray_cmd::FETCH_NOW => app.w.trigger_now(),
        tray_cmd::OPEN_LOGS => {
            let _ = osutil::open_folder(&app.p.log_dir.to_string_lossy());
        }
        tray_cmd::OPEN_CONFIG => {
            let _ = osutil::open_target(&app.p.config_path.to_string_lossy());
        }
        tray_cmd::OPEN_WIZARD => open_wizard(app),
        tray_cmd::TOGGLE_AUTOSTART => {
            let actual = autostart::apply(!autostart::is_enabled());
            let _ = app.store.update(|c| c.behavior.auto_launch_at_login = actual);
        }
        tray_cmd::TOGGLE_AUTOSELECT => {
            let _ = app
                .store
                .update(|c| c.behavior.auto_select = !c.behavior.auto_select);
        }
        tray_cmd::QUIT => {
            let _ = app.client.jar.save();
            std::process::exit(0);
        }
        _ => {}
    }
}

fn build_wizard_actions(app: &Arc<App>) -> wizard::Actions {
    let p = app.p.clone();

    let a = app.clone();
    let status: Box<dyn Fn() -> wizard::Status + Send + Sync> = Box::new(move || {
        let cfg = a.store.get();
        let s = a.w.status();
        let st = a.st.get();
        wizard::Status {
            first_run_completed: st.first_run_completed,
            credentials_configured: cfg.has_credentials(),
            pushplus_configured: !cfg.push.pushplus.token.is_empty(),
            windows_notify_enabled: cfg.push.windows.enabled,
            auth_state: st.auth_state.as_str().to_string(),
            watcher_message: s.last_message,
            last_success_at: s.last_success_at,
            auto_start_enabled: autostart::is_enabled(),
            auto_select_enabled: cfg.behavior.auto_select,
            filters_configured: !cfg.filters.location_whitelist.is_empty()
                || !cfg.filters.category_blacklist.is_empty(),
            config_path: p.config_path.to_string_lossy().into_owned(),
            data_dir: p.data_dir.to_string_lossy().into_owned(),
            version: VERSION.to_string(),
        }
    });

    let a = app.clone();
    let cfg_view: Box<dyn Fn() -> wizard::ConfigView + Send + Sync> = Box::new(move || {
        let cfg = a.store.get();
        wizard::ConfigView {
            username: cfg.credentials.username.clone(),
            password: cfg.credentials.password.clone(),
            pushplus_token: cfg.push.pushplus.token.clone(),
            pushplus_enabled: cfg.push.pushplus.enabled,
            windows_notify_enabled: cfg.push.windows.enabled,
            auto_launch_at_login: cfg.behavior.auto_launch_at_login,
            auto_select: cfg.behavior.auto_select,
            location_whitelist: cfg.filters.location_whitelist.clone(),
            category_blacklist: cfg.filters.category_blacklist.clone(),
        }
    });

    let a = app.clone();
    let verify: Box<dyn Fn() -> (String, String) + Send + Sync> = Box::new(move || {
        match site::check(&a.client) {
            Ok(probe) => match probe.verdict {
                site::Verdict::Courses => ("valid".to_string(), probe.reason),
                site::Verdict::Login => ("expired".to_string(), probe.reason),
                site::Verdict::Unknown => ("unknown".to_string(), probe.reason),
            },
            Err(e) => ("unknown".to_string(), format!("请求失败: {}", e)),
        }
    });

    let a = app.clone();
    let save_account: Box<dyn Fn(&str, &str) -> Result<()> + Send + Sync> =
        Box::new(move |username: &str, password: &str| {
            a.store.update(|c| {
                c.credentials.username = username.to_string();
                c.credentials.password = password.to_string();
            })
        });

    let a = app.clone();
    let save_filters: Box<dyn Fn(&[String], &[String]) -> Result<()> + Send + Sync> =
        Box::new(move |location_whitelist: &[String], category_blacklist: &[String]| {
            a.store.update(|c| {
                c.filters.location_whitelist = location_whitelist.to_vec();
                c.filters.category_blacklist = category_blacklist.to_vec();
            })
        });

    let a = app.clone();
    let save_notify: Box<dyn Fn(&str, bool, bool) -> Result<()> + Send + Sync> =
        Box::new(move |token: &str, pp: bool, wn: bool| {
            a.store.update(|c| {
                c.push.pushplus.token = token.to_string();
                c.push.pushplus.enabled = pp && !token.is_empty();
                c.push.windows.enabled = wn;
            })
        });

    let a = app.clone();
    let set_behavior: Box<dyn Fn(Option<bool>, Option<bool>) -> Result<()> + Send + Sync> =
        Box::new(move |auto_start: Option<bool>, auto_select: Option<bool>| {
            if let Some(v) = auto_start {
                let actual = autostart::apply(v);
                a.store
                    .update(|c| c.behavior.auto_launch_at_login = actual)?;
            }
            if let Some(v) = auto_select {
                a.store.update(|c| c.behavior.auto_select = v)?;
            }
            Ok(())
        });

    let a = app.clone();
    let open_login: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        let a = a.clone();
        std::thread::spawn(move || open_login_window(&a));
    });
    let a = app.clone();
    let open_course: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        let a = a.clone();
        std::thread::spawn(move || open_course_view(&a));
    });
    let a = app.clone();
    let open_config: Box<dyn Fn() + Send + Sync> =
        Box::new(move || {
            let _ = osutil::open_target(&a.p.config_path.to_string_lossy());
        });
    let a = app.clone();
    let open_logs: Box<dyn Fn() + Send + Sync> =
        Box::new(move || {
            let _ = osutil::open_folder(&a.p.log_dir.to_string_lossy());
        });
    let open_external: Box<dyn Fn(&str) -> Result<()> + Send + Sync> =
        Box::new(|url: &str| osutil::open_url(url));

    wizard::Actions {
        status,
        config: cfg_view,
        verify,
        save_account,
        save_filters,
        save_notify,
        set_behavior,
        open_login,
        open_course,
        open_config,
        open_logs,
        open_external,
    }
}

fn run_login(
    p: &paths::Paths,
    store: &Arc<config::Store>,
    client: &Arc<Client>,
    st: &Arc<state::Store>,
    log: &Logger,
) -> Result<()> {
    let cfg = store.get();
    let exe = cfg.browser_path().map(str::to_string);
    let cred = cfg.credentials;
    let profile = p
        .data_dir
        .join("browser-profile")
        .to_string_lossy()
        .into_owned();
    let cookies = login::run(&login::LoginOptions {
        url: session::CAS_LOGIN,
        profile_dir: &profile,
        timeout: Duration::from_secs(300),
        poll_interval: Duration::from_secs(3),
        username: &cred.username,
        password: &cred.password,
        exec_path: exe.as_deref(),
        log,
    })
    .map_err(|e| anyhow::anyhow!("{}", browser_error(&e)))?;

    client.jar.set_stored(&cookies);
    client.jar.save()?;
    let _ = st.update(|s| {
        s.first_run_completed = true;
        s.auth_state = state::AuthState::Valid;
    });
    log.info("登录态已保存,后台现在可以直接轮询");
    Ok(())
}

fn run_wizard_ui(url: &str, data_path: &str, browser: &str) -> Result<()> {
    let data = if data_path.is_empty() {
        std::env::temp_dir().join("seu-labor-wizard-webview")
    } else {
        std::path::PathBuf::from(data_path)
    };
    let url = if url.is_empty() { "about:blank" } else { url };

    let cfg = websurface::SurfaceConfig {
        url: url.to_string(),
        title: brand::TITLE.to_string(),
        logical_width: brand::DESIGN_WIDTH,
        logical_height: brand::DESIGN_HEIGHT,
        min_width: brand::MIN_WIDTH,
        min_height: brand::MIN_HEIGHT,
        position: None,
        icon_ico: assets::ICON_ICO,
        data_dir: data,
        browser_override: (!browser.trim().is_empty()).then(|| browser.to_string()),
        profile_name: brand::TITLE.to_string(),
        chromeless: true,
        zoom: 1.0,
        zoomable: false,
    };
    let mut host = websurface::WebHost::new()?;
    host.open(cfg, wizard_engine())?;
    host.run()?;
    Ok(())
}

/// `SEU_WIZARD_ENGINE=browser|webview` forces one engine for troubleshooting;
/// otherwise WebView2 when present, else a borrowed Chromium.
fn wizard_engine() -> websurface::Engine {
    match std::env::var("SEU_WIZARD_ENGINE")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("browser") => websurface::Engine::Borrowed,
        Some("webview") => websurface::Engine::WebView2,
        _ => websurface::Engine::Auto,
    }
}
