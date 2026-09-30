//! Site-specific DOM scripts, ported verbatim from the previous implementation.
//! They run inside the page (Eval) against the school's own React login form.
//! Only interactive login needs a JS engine; polling, selection and
//! cancellation are plain HTTP.

const USERNAME_SELECT: &str = concat!(
    r#"document.querySelector('input.input-username-pc[type="text"]')"#,
    r#"|| document.querySelector('input.input-username-mobile[type="text"]')"#,
    r#"|| document.querySelector('input[type="text"][placeholder*="一卡通号"]')"#,
    r#"|| document.querySelector('input[type="text"][placeholder*="学号"]')"#,
);

const PASSWORD_SELECT: &str = concat!(
    r#"document.querySelector('input[type="password"]')"#,
    r#"|| document.querySelector('input.input-password-pc')"#,
    r#"|| document.querySelector('input.input-password-mobile input.ant-input')"#,
);

const BUTTON_SELECT: &str = concat!(
    r#"document.querySelector('button.login-button-pc')"#,
    r#"|| document.querySelector('button[type="button"].ant-btn-primary')"#,
    r#"|| document.querySelector('button[type="button"]')"#,
);

/// Probes the login form and JS-only challenges (captcha, SMS).
pub fn ready_script() -> String {
    format!(
        r#"(() => {{
  const visible = (el) => {{
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
  }};
  const u = {u};
  const p = {p};
  const b = {b};
  const text = document.body ? document.body.innerText : '';
  const codeInput = document.querySelector('input[placeholder*="验证码"], #CaptchaInputText, input[name*="aptcha"], input[name*="Captcha"]');
  const capImg = document.querySelector('img[src*="aptcha"], img[src*="Captcha"], .captcha-img');
  return {{
    ready: Boolean(u && p && b),
    hasUsername: Boolean(u),
    hasPassword: Boolean(p),
    hasButton: Boolean(b),
    needStage2: visible(codeInput) && !p,
    hasCaptcha: visible(codeInput) || visible(capImg),
    text: text.slice(0, 500)
  }};
}})()"#,
        u = USERNAME_SELECT,
        p = PASSWORD_SELECT,
        b = BUTTON_SELECT,
    )
}

/// Pre-fills the login form without submitting, so the user only has to solve
/// the captcha. Existing input is left untouched.
pub fn fill_credentials_script(username: &str, password: &str) -> String {
    let fill = r#"(() => {
  const setVal = (input, value) => {
    const last = input.value;
    input.value = value;
    const ev = new Event('input', { bubbles: true });
    ev.simulated = true;
    const tracker = input._valueTracker;
    if (tracker) tracker.setValue(last);
    input.dispatchEvent(ev);
  };
  const u = __U__;
  const p = __P__;
  const b = __B__;
  if (!u || !p || !b) return false;
  if (u.value && p.value) return false;
  setVal(u, __USERNAME__);
  setVal(p, __PASSWORD__);
  return true;
})()"#;
    fill.replace("__U__", &USERNAME_SELECT)
        .replace("__P__", &PASSWORD_SELECT)
        .replace("__B__", &BUTTON_SELECT)
        .replace("__USERNAME__", &json_string(username))
        .replace("__PASSWORD__", &json_string(password))
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "null".to_string())
}
