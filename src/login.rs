//! Interactive login: opens a visible Chromium window so the user can clear a
//! captcha/SMS, then harvests the cookie jar. The unattended HTTP replay lives
//! in `seu_labor_core::cas`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};

use browserhost::{Session, WindowMode};

use seu_labor_core::js;
use seu_labor_core::logging::Logger;
use seu_labor_core::session;
use seu_labor_core::session::jar::{Jar, StoredCookie};
use seu_labor_core::site;

use crate::browser_cookie;

pub struct LoginOptions<'a> {
    pub url: &'a str,
    pub profile_dir: &'a str,
    pub timeout: Duration,
    pub poll_interval: Duration,
    /// Pre-fill the login form (the captcha is left to the user).
    pub username: &'a str,
    pub password: &'a str,
    /// Optional explicit browser executable; `None` auto-discovers one.
    pub exec_path: Option<&'a str>,
    pub log: &'a Logger,
}

/// Blocks until the session authenticates or the deadline passes. The returned
/// cookies include session cookies; the caller must persist them.
pub fn run(opts: &LoginOptions<'_>) -> Result<Vec<StoredCookie>> {
    let poll = if opts.poll_interval.is_zero() {
        Duration::from_secs(3)
    } else {
        opts.poll_interval
    };

    let mut b = Session::launch(browserhost::SessionConfig {
        visible: true,
        profile_dir: opts.profile_dir.to_string(),
        url: Some(opts.url.to_string()),
        mode: WindowMode::App,
        exec_path: opts.exec_path.map(str::to_string),
        profile_name: crate::brand::TITLE.to_string(),
        position: None,
    })?;

    opts.log
        .info("已打开登录窗口,请在窗口内完成登录(可能需要验证码/短信)");

    let deadline = Instant::now() + opts.timeout;
    let mut filled = opts.username.is_empty() || opts.password.is_empty();

    loop {
        std::thread::sleep(poll);
        if Instant::now() > deadline {
            b.close();
            return Err(anyhow!("等待登录超时({:?})", opts.timeout));
        }
        if !filled {
            filled = try_fill_credentials(&mut b, opts);
        }
        let Ok(cookies) = b.cookies() else {
            continue;
        };
        if authenticated(&cookies) {
            opts.log
                .info(format!("检测到登录成功,已获取 {} 个 Cookie", cookies.len()));
            b.close();
            return Ok(cookies.into_iter().map(browser_cookie::to_stored).collect());
        }
    }
}

fn try_fill_credentials(b: &mut Session, opts: &LoginOptions<'_>) -> bool {
    let Ok(probe) = b.eval(&js::ready_script()) else {
        return false;
    };
    if !probe.get("ready").and_then(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }
    let Ok(filled) = b.eval(&js::fill_credentials_script(
        opts.username,
        opts.password,
    )) else {
        return false;
    };
    if filled.as_bool().unwrap_or(false) {
        opts.log
            .info("已自动预填账号密码,请填写验证码后点击登录");
        true
    } else {
        false
    }
}

fn authenticated(cookies: &[browserhost::Cookie]) -> bool {
    let stored: Vec<StoredCookie> = cookies
        .iter()
        .cloned()
        .map(browser_cookie::to_stored)
        .collect();
    let jar = Arc::new(Jar::empty());
    jar.set_stored(&stored);
    let client = session::Client::with_jar(jar, Duration::from_secs(15));
    matches!(site::check(&client), Ok(p) if p.verdict == site::Verdict::Courses)
}
