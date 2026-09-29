package browser

import (
	"encoding/json"
	"strings"
)

// The site-specific DOM scripts are ported verbatim from the previous
// implementation. They run inside the page (Eval) against the school's own
// React login form. Only interactive login needs a JS engine; polling,
// selection and cancellation are plain HTTP.

const usernameSelect = `document.querySelector('input.input-username-pc[type="text"]')` +
	` || document.querySelector('input.input-username-mobile[type="text"]')` +
	` || document.querySelector('input[type="text"][placeholder*="一卡通号"]')` +
	` || document.querySelector('input[type="text"][placeholder*="学号"]')`

const passwordSelect = `document.querySelector('input[type="password"]')` +
	` || document.querySelector('input.input-password-pc')` +
	` || document.querySelector('input.input-password-mobile input.ant-input')`

const buttonSelect = `document.querySelector('button.login-button-pc')` +
	` || document.querySelector('button[type="button"].ant-btn-primary')` +
	` || document.querySelector('button[type="button"]')`

// ReadyScript probes the login form and JS-only challenges (captcha, SMS).
const readyScript = `(() => {
  const visible = (el) => {
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
  };
  const u = ` + usernameSelect + `;
  const p = ` + passwordSelect + `;
  const b = ` + buttonSelect + `;
  const text = document.body ? document.body.innerText : '';
  const codeInput = document.querySelector('input[placeholder*="验证码"], #CaptchaInputText, input[name*="aptcha"], input[name*="Captcha"]');
  const capImg = document.querySelector('img[src*="aptcha"], img[src*="Captcha"], .captcha-img');
  return {
    ready: Boolean(u && p && b),
    hasUsername: Boolean(u),
    hasPassword: Boolean(p),
    hasButton: Boolean(b),
    needStage2: visible(codeInput) && !p,
    hasCaptcha: visible(codeInput) || visible(capImg),
    text: text.slice(0, 500)
  };
})()`

func ReadyScript() string { return readyScript }

// FillCredentialsScript pre-fills the login form without submitting, so the
// user only has to solve the captcha. Existing input is left untouched.
func FillCredentialsScript(username, password string) string {
	fill := `(() => {
  const setVal = (input, value) => {
    const last = input.value;
    input.value = value;
    const ev = new Event('input', { bubbles: true });
    ev.simulated = true;
    const tracker = input._valueTracker;
    if (tracker) tracker.setValue(last);
    input.dispatchEvent(ev);
  };
  const u = ` + usernameSelect + `;
  const p = ` + passwordSelect + `;
  const b = ` + buttonSelect + `;
  if (!u || !p || !b) return false;
  if (u.value && p.value) return false;
  setVal(u, __USERNAME__);
  setVal(p, __PASSWORD__);
  return true;
})()`
	return strings.NewReplacer(
		"__USERNAME__", jsonString(username),
		"__PASSWORD__", jsonString(password),
	).Replace(fill)
}

func jsonString(s string) string {
	raw, _ := json.Marshal(s)
	return string(raw)
}
