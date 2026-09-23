/**
 * 内置浏览器窗口的 preload。
 *
 * 职责(PLAN.md 9.4):
 * 1. 自动预填账号密码(移植自原脚本的 forceSetValue,兼容 React 受控组件)
 * 2. 首启时在页面顶部插入说明横幅,把「需要短信验证」这件事讲清楚
 *
 * 注意:**不自动点击登录按钮**。这是留给人工的窗口 ——
 * 首次登录必然涉及短信验证码,任何自动化都绕不过去。
 */
import { contextBridge, ipcRenderer } from 'electron';

interface WindowContext {
  mode: 'login' | 'course';
  firstRun: boolean;
}

async function getContext(): Promise<WindowContext | null> {
  try {
    return (await ipcRenderer.invoke('window:context')) as WindowContext | null;
  } catch {
    return null;
  }
}

/** 兼容 React 等框架的强制赋值(原脚本 forceSetValue 的移植) */
function forceSetValue(input: HTMLInputElement, value: string): void {
  const lastValue = input.value;
  input.value = value;
  const event = new Event('input', { bubbles: true });
  (event as Event & { simulated?: boolean }).simulated = true;
  const tracker = (input as HTMLInputElement & { _valueTracker?: { setValue(v: string): void } })
    ._valueTracker;
  if (tracker) tracker.setValue(lastValue);
  input.dispatchEvent(event);
}

function findUsername(): HTMLInputElement | null {
  return (
    document.querySelector<HTMLInputElement>('input.input-username-pc[type="text"]') ||
    document.querySelector<HTMLInputElement>('input.input-username-mobile[type="text"]') ||
    document.querySelector<HTMLInputElement>('input[type="text"][placeholder*="一卡通号"]') ||
    document.querySelector<HTMLInputElement>('input[type="text"][placeholder*="学号"]')
  );
}

function findPassword(): HTMLInputElement | null {
  return (
    document.querySelector<HTMLInputElement>('input[type="password"]') ||
    document.querySelector<HTMLInputElement>('input.input-password-pc') ||
    document.querySelector<HTMLInputElement>('input.input-password-mobile input.ant-input')
  );
}

/** 预填凭据:轮询等待表单出现,填完即停,不做提交 */
async function prefillCredentials(): Promise<void> {
  let creds: { username: string; password: string } | null = null;
  try {
    creds = (await ipcRenderer.invoke('auth:get-prefill')) as {
      username: string;
      password: string;
    } | null;
  } catch {
    return;
  }
  if (!creds || !creds.username || !creds.password) return;

  const deadline = Date.now() + 60000;
  const timer = window.setInterval(() => {
    if (Date.now() > deadline) {
      window.clearInterval(timer);
      return;
    }
    const usernameInput = findUsername();
    const passwordInput = findPassword();
    if (!usernameInput || !passwordInput) return;

    window.clearInterval(timer);
    // 只在为空时填充,避免覆盖用户手输的内容
    if (!usernameInput.value) forceSetValue(usernameInput, creds!.username);
    if (!passwordInput.value) forceSetValue(passwordInput, creds!.password);
    renderBanner(
      '账号密码已自动预填',
      '请按页面提示完成短信验证，然后点击登录。完成后本窗口会自动关闭。',
      'info'
    );
  }, 700);
}

type BannerTone = 'info' | 'warn' | 'ok';

const BANNER_STYLE_ID = 'seu-daemon-banner-style';

const BANNER_ICON: Record<BannerTone, string> = {
  info: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.8"/><path d="M12 11v5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/><circle cx="12" cy="7.8" r="1.1" fill="currentColor"/></svg>',
  warn: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none"><path d="M12 3.5 2.8 19.5h18.4L12 3.5Z" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/><path d="M12 10v4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/><circle cx="12" cy="16.8" r="1.05" fill="currentColor"/></svg>',
  ok: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.8"/><path d="m8.2 12.3 2.5 2.5 5-5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>',
};

const BANNER_COLOR: Record<BannerTone, { bg: string; border: string; accent: string; text: string }> = {
  info: { bg: '#e8f1ff', border: '#b7d2ff', accent: '#2f6feb', text: '#0b1f3a' },
  warn: { bg: '#fff4e5', border: '#f0c48a', accent: '#c47f00', text: '#4a3200' },
  ok: { bg: '#e6f6ef', border: '#a9dcc4', accent: '#127a51', text: '#0f5e40' },
};

function ensureBannerStyle(): void {
  if (document.getElementById(BANNER_STYLE_ID)) return;
  const style = document.createElement('style');
  style.id = BANNER_STYLE_ID;
  style.textContent = [
    '#seu-daemon-banner{position:fixed;left:0;right:0;top:0;z-index:2147483647;',
    'display:flex;align-items:center;gap:10px;padding:9px 16px;',
    'font:13px/1.55 "Microsoft YaHei","PingFang SC",system-ui,sans-serif;',
    'border-bottom:1px solid;box-shadow:0 2px 10px rgba(0,0,0,.10);}',
    '#seu-daemon-banner .seu-ic{display:inline-flex;flex:0 0 auto;align-items:center;justify-content:center;}',
    '#seu-daemon-banner .seu-title{font-weight:700;white-space:nowrap;}',
    '#seu-daemon-banner .seu-body{flex:1 1 auto;min-width:0;}',
    '#seu-daemon-banner .seu-close{margin-left:8px;flex:0 0 auto;border:0;background:transparent;cursor:pointer;font-size:14px;line-height:1;padding:4px 6px;border-radius:6px;opacity:.65;}',
    '#seu-daemon-banner .seu-close:hover{opacity:1;background:rgba(0,0,0,.06);}',
  ].join('');
  const attach = () => document.head && document.head.appendChild(style);
  if (document.head) attach();
  else document.addEventListener('DOMContentLoaded', attach, { once: true });
}

/** 在页面顶部渲染引导横幅;重复调用时原位更新,不会叠加多层 */
function renderBanner(title: string, body: string, tone: BannerTone): void {
  ensureBannerStyle();

  const colors = BANNER_COLOR[tone];
  let bar = document.getElementById('seu-daemon-banner');
  if (!bar) {
    bar = document.createElement('div');
    bar.id = 'seu-daemon-banner';

    const icon = document.createElement('span');
    icon.className = 'seu-ic';
    const titleEl = document.createElement('span');
    titleEl.className = 'seu-title';
    const bodyEl = document.createElement('span');
    bodyEl.className = 'seu-body';
    const close = document.createElement('button');
    close.className = 'seu-close';
    close.textContent = '✕';
    close.title = '关闭提示';
    close.onclick = () => bar?.remove();

    bar.append(icon, titleEl, bodyEl, close);

    const attach = () => document.body && document.body.appendChild(bar!);
    if (document.body) attach();
    else document.addEventListener('DOMContentLoaded', attach, { once: true });
  }

  bar.style.background = colors.bg;
  bar.style.borderBottomColor = colors.border;
  bar.style.color = colors.text;

  const icon = bar.querySelector<HTMLElement>('.seu-ic');
  if (icon) {
    icon.innerHTML = BANNER_ICON[tone];
    icon.style.color = colors.accent;
  }
  const titleEl = bar.querySelector<HTMLElement>('.seu-title');
  if (titleEl) titleEl.textContent = title;
  const bodyEl = bar.querySelector<HTMLElement>('.seu-body');
  if (bodyEl) bodyEl.textContent = body;
}

async function main(): Promise<void> {
  const ctx = await getContext();
  if (!ctx || ctx.mode !== 'login') return;

  if (ctx.firstRun) {
    renderBanner(
      '首次使用 · 请完成一次登录',
      '首次登录（或换网络出口）可能需要短信验证码，请按页面提示完成登录。完成后本窗口会自动关闭，后续长期免打扰。',
      'info'
    );
  } else {
    renderBanner(
      '登录态已失效 · 请重新登录',
      '请按页面提示重新登录。完成后本窗口会自动关闭，监控会立即恢复。',
      'warn'
    );
  }

  await prefillCredentials();

  // 站内跳转出认证域名即认为登录动作完成,提示用户可关闭窗口
  window.setInterval(() => {
    if (!location.hostname.includes('auth.seu.edu.cn')) {
      renderBanner(
        '看起来已经登录',
        '已离开统一身份认证页。若页面已进入选课系统，可直接关闭本窗口。',
        'ok'
      );
    }
  }, 1500);
}

void main();

contextBridge.exposeInMainWorld('seuDaemon', {
  isDaemonWindow: true,
});
