//! The unattended CAS login. It replays the flow over plain HTTP (public key →
//! RSA-encrypt password → casLogin → redeem ticket) and only asks a human when
//! the server actually demands a captcha/SMS.
//!
//! The interactive variant (a visible browser or WebView) lives in each
//! frontend; it only has to harvest cookies and hand them to [`crate::session`].

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use base64::Engine;
use rsa::pkcs8::DecodePublicKey;
use rsa::rand_core::OsRng;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};

use crate::logging::Logger;
use crate::session;
use crate::session::jar::{Jar, StoredCookie};
use crate::site;

const CAS_SERVICE: &str = "https://labor.seu.edu.cn/UnifiedAuth/CASLogin";
const CAS_NEED_CAPTCHA: &str = "https://auth.seu.edu.cn/auth/casback/needCaptcha";
const CAS_CHIPER_KEY: &str = "https://auth.seu.edu.cn/auth/casback/getChiperKey";
const CAS_LOGIN_URL: &str = "https://auth.seu.edu.cn/auth/casback/casLogin";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoOutcome {
    Success,
    NoCredentials,
    NeedManual,
    Error,
}

impl AutoOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            AutoOutcome::Success => "success",
            AutoOutcome::NoCredentials => "noCredentials",
            AutoOutcome::NeedManual => "needManual",
            AutoOutcome::Error => "error",
        }
    }
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
