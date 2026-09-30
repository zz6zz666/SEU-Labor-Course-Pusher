//! The two ways to establish a session.
//!
//! - `auto_run`: unattended and browser-free. It replays the CAS login flow
//!   over plain HTTP (public key → RSA-encrypt password → casLogin → redeem
//!   ticket) and only asks a human when the server actually demands a
//!   captcha/SMS.
//! - `run`: interactive, visible. It opens the CAS page in a browser window so
//!   the user can clear a captcha/SMS, then harvests the cookie jar.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use base64::Engine;
use rsa::pkcs8::DecodePublicKey;
use rsa::rand_core::OsRng;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};

use browserhost::{Session, WindowMode};

use crate::logging::Logger;
use crate::session;
use crate::session::jar::{Jar, StoredCookie};
use crate::site;

const CAS_SERVICE: &str = "https://labor.seu.edu.cn/UnifiedAuth/CASLogin";
const CAS_NEED_CAPTCHA: &str = "https://auth.seu.edu.cn/auth/casback/needCaptcha";
const CAS_CHIPER_KEY: &str = "https://auth.seu.edu.cn/auth/casback/getChiperKey";
const CAS_LOGIN_URL: &str = "https://auth.seu.edu.cn/auth/casback/casLogin";

// ---------------------------------------------------------------- unattended

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoOutcome {
    Success,
    NoCredentials,
    NeedManual,
    Error,
}

pub struct AutoResult {
    pub outcome: AutoOutcome,
    pub detail: String,
    pub cookies: Vec<StoredCookie>,
}

/// Replays the CAS login over HTTP and returns the harvested cookies. It probes
/// for a captcha/SMS first and returns `NeedManual` (making no login attempt)
/// when a human would be required.
pub fn auto_run(username: &str, password: &str, log: &Logger) -> AutoResult {
    if username.is_empty() || password.is_empty() {
        return AutoResult {
            outcome: AutoOutcome::NoCredentials,
            detail: "未配置账号密码".to_string(),
            cookies: Vec::new(),
        };
    }

    let jar = Arc::new(Jar::empty());
    let client = session::Client::with_jar(jar.clone(), Duration::from_secs(25));

    // 0) Ask the server whether a captcha / SMS would be demanded.
    let cap_res = match client.get(CAS_NEED_CAPTCHA) {
        Ok(r) => r,
        Err(e) => {
            return AutoResult {
                outcome: AutoOutcome::Error,
                detail: format!("探测验证码失败: {}", e),
                cookies: Vec::new(),
            }
        }
    };
    let cap: serde_json::Value = serde_json::from_str(&cap_res.body).unwrap_or_default();
    let need_stage2 = cap
        .get("needStage2Validation")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if need_stage2 {
        return AutoResult {
            outcome: AutoOutcome::NeedManual,
            detail: "服务端要求短信二次验证".to_string(),
            cookies: Vec::new(),
        };
    }
    let cap_success = cap.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    let cap_info = cap.get("info").and_then(|v| v.as_str()).unwrap_or("");
    if !cap_success || (cap_info.contains("验证码") && !cap_info.contains("不需要")) {
        return AutoResult {
            outcome: AutoOutcome::NeedManual,
            detail: format!("登录需要验证码: {}", cap_info),
            cookies: Vec::new(),
        };
    }

    // 1) Fetch the RSA public key.
    let key_res = match client.post_json(CAS_CHIPER_KEY, "{}") {
        Ok(r) => r,
        Err(e) => {
            return AutoResult {
                outcome: AutoOutcome::Error,
                detail: format!("获取公钥失败: {}", e),
                cookies: Vec::new(),
            }
        }
    };
    let key: serde_json::Value = serde_json::from_str(&key_res.body).unwrap_or_default();
    let ok = key.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    let public_key = key.get("publicKey").and_then(|v| v.as_str()).unwrap_or("");
    if !ok || public_key.is_empty() {
        return AutoResult {
            outcome: AutoOutcome::Error,
            detail: "获取公钥失败".to_string(),
            cookies: Vec::new(),
        };
    }
    let enc_pwd = match rsa_encrypt_base64(public_key, password) {
        Ok(v) => v,
        Err(e) => {
            return AutoResult {
                outcome: AutoOutcome::Error,
                detail: format!("加密密码失败: {}", e),
                cookies: Vec::new(),
            }
        }
    };

    // 2) Submit the credentials.
    let payload = serde_json::json!({
        "service": CAS_SERVICE,
        "username": username,
        "password": enc_pwd,
        "captcha": "",
        "rememberMe": false,
        "loginType": "account",
        "wxBinded": false,
        "mobilePhoneNum": "",
        "fingerPrint": format!("stable_{}", device_fingerprint()),
    })
    .to_string();
    let login_res = match client.post_json(CAS_LOGIN_URL, &payload) {
        Ok(r) => r,
        Err(e) => {
            return AutoResult {
                outcome: AutoOutcome::Error,
                detail: format!("提交登录失败: {}", e),
                cookies: Vec::new(),
            }
        }
    };
    let info: serde_json::Value = serde_json::from_str(&login_res.body).unwrap_or_default();
    let code = info.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
    let redirect_url = info
        .get("redirectUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if code != 200 || redirect_url.is_empty() {
        let msg = info.get("info").and_then(|v| v.as_str()).unwrap_or("");
        return AutoResult {
            outcome: AutoOutcome::Error,
            detail: format!("CAS 登录失败(code={}): {}", code, msg),
            cookies: Vec::new(),
        };
    }

    // 3) Redeem the ticket at the service so the labor session cookies are set.
    // The CAS response returns redirectUrl percent-encoded.
    let service_url = percent_encoding::percent_decode_str(redirect_url)
        .decode_utf8_lossy()
        .into_owned();
    if !service_url.starts_with("http") {
        return AutoResult {
            outcome: AutoOutcome::Error,
            detail: "CAS 未返回有效跳转地址".to_string(),
            cookies: Vec::new(),
        };
    }
    if let Err(e) = client.get(&service_url) {
        return AutoResult {
            outcome: AutoOutcome::Error,
            detail: format!("兑换票据失败: {}", e),
            cookies: Vec::new(),
        };
    }

    // 4) Verify the session actually works.
    match site::check(&client) {
        Ok(probe) if probe.verdict == site::Verdict::Courses => {}
        Ok(probe) => {
            return AutoResult {
                outcome: AutoOutcome::Error,
                detail: format!("登录后会话未生效: {}", probe.reason),
                cookies: Vec::new(),
            }
        }
        Err(e) => {
            return AutoResult {
                outcome: AutoOutcome::Error,
                detail: format!("登录后会话未生效: {}", e),
                cookies: Vec::new(),
            }
        }
    }
    let cookies = jar.all();
    let detail = format!("静默登录成功({} 个 Cookie)", cookies.len());
    log.info(&detail);
    AutoResult {
        outcome: AutoOutcome::Success,
        detail,
        cookies,
    }
}

/// Parses the site's public key (URL-safe base64 DER) and returns the PKCS#1
/// v1.5 ciphertext as standard base64.
fn rsa_encrypt_base64(pub_key: &str, plaintext: &str) -> Result<String> {
    let der = decode_base64_any(pub_key)?;
    let key = RsaPublicKey::from_public_key_der(&der).map_err(|e| anyhow!("解析公钥失败: {}", e))?;
    let enc = key
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, plaintext.as_bytes())
        .map_err(|e| anyhow!("加密失败: {}", e))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(enc))
}

fn decode_base64_any(s: &str) -> Result<Vec<u8>> {
    use base64::engine::general_purpose::{
        STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD,
    };
    for enc in [URL_SAFE_NO_PAD, URL_SAFE, STANDARD_NO_PAD, STANDARD] {
        if let Ok(b) = enc.decode(s) {
            return Ok(b);
        }
    }
    Err(anyhow!("无法解码公钥"))
}

/// Returns a stable 32-hex-char id, mirroring the SPA's `stable_<hex>`
/// fingerprint.
fn device_fingerprint() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

// ---------------------------------------------------------------- interactive

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
            return Ok(cookies.into_iter().map(StoredCookie::from).collect());
        }
    }
}

fn try_fill_credentials(b: &mut Session, opts: &LoginOptions<'_>) -> bool {
    let Ok(probe) = b.eval(&crate::login_scripts::ready_script()) else {
        return false;
    };
    if !probe.get("ready").and_then(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }
    let Ok(filled) = b.eval(&crate::login_scripts::fill_credentials_script(
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
    let stored: Vec<StoredCookie> = cookies.iter().cloned().map(StoredCookie::from).collect();
    let jar = Arc::new(Jar::empty());
    jar.set_stored(&stored);
    let client = session::Client::with_jar(jar, Duration::from_secs(15));
    matches!(site::check(&client), Ok(p) if p.verdict == site::Verdict::Courses)
}
