/**
 * 登录态探测 + 隐藏窗口自动登录。
 *
 * 与老方案最本质的差别(PLAN.md 2.3):
 * 不再问「目标页面还活着吗」这种需要心跳做代理指标的问题,
 * 而是直接问服务端 —— 302 跳登录就是失效,返回表格就是有效。
 *
 * 原脚本的 loginFailStatus / loginFailTime / 15 分钟禁用期整体删除,
 * 由 watcher 的指数退避 + 告警替代。
 */
import type { BrowserWindow } from 'electron';
import type { AuthState } from '../types';
import { getConfig } from '../config';
import { createLogger } from '../logger';
import { URLS } from './session';
import { getHtml } from './fetch';
import { hasCourseTable, looksLikeCourseRoute, looksLikeLoginPage } from './parse';
import { getHiddenWindow } from '../main/windows';

const log = createLogger('auth');

export interface AuthProbe {
  state: AuthState;
  reason: string;
  html: string;
}

// ---------------------------------------------------------------- 选择器

/** 用户名输入框(PC / 移动端 / 兜底) */
const SEL_USERNAME =
  `document.querySelector('input.input-username-pc[type="text"]')` +
  ` || document.querySelector('input.input-username-mobile[type="text"]')` +
  ` || document.querySelector('input[type="text"][placeholder*="一卡通号"]')` +
  ` || document.querySelector('input[type="text"][placeholder*="学号"]')`;

/** 密码输入框 */
const SEL_PASSWORD =
  `document.querySelector('input[type="password"]')` +
  ` || document.querySelector('input.input-password-pc')` +
  ` || document.querySelector('input.input-password-mobile input.ant-input')`;

/** 登录按钮 */
const SEL_BUTTON =
  `document.querySelector('button.login-button-pc')` +
  ` || document.querySelector('button[type="button"].ant-btn-primary')` +
  ` || document.querySelector('button[type="button"]')`;

// ---------------------------------------------------------------- 探测

function isAuthHost(url: string): boolean {
  try {
    return new URL(url).hostname === 'auth.seu.edu.cn';
  } catch {
    return false;
  }
}

/**
 * 请求一次选课页并判定登录态。
 *
 * 判定顺序经不起随意调换:**先找正向信号(课程表格),再找负向信号(登录页特征)**。
 * 原因是「登录页特征词」里有些词(如 `casLogin`)在已登录页面的页脚/登出链接里也可能出现,
 * 反过来把有效会话误判成失效。表格 id 则是唯一的、不会被误伤的强信号。
 */
export async function probeAuth(timeoutMs = 20000): Promise<AuthProbe> {
  const res = await getHtml(URLS.coursePage, timeoutMs);

  if (!res.ok && !res.body) {
    return {
      state: 'unknown',
      reason: `请求失败:${res.error ?? `HTTP ${res.statusCode}`}`,
      html: '',
    };
  }

  // 1) 正向强信号:正文里就有课程表格
  if (hasCourseTable(res.body)) {
    return { state: 'valid', reason: '返回选课表格', html: res.body };
  }

  // 2) 负向信号:被重定向到统一身份认证
  const redirectedToAuth =
    isAuthHost(res.finalUrl) || res.redirects.some((u) => isAuthHost(u));
  if (redirectedToAuth) {
    return { state: 'expired', reason: `被重定向到统一身份认证(${res.finalUrl})`, html: res.body };
  }

  if (res.redirects.some((u) => u.includes('AuthServer/Login'))) {
    return { state: 'expired', reason: '被重定向到 AuthServer/Login', html: res.body };
  }

  // 3) 负向信号:正文含登录页特征
  if (looksLikeLoginPage(res.body)) {
    return { state: 'expired', reason: '正文含登录页特征', html: res.body };
  }

  // 4) 弱正向信号:已是选课路由但表格由前端异步渲染
  if (looksLikeCourseRoute(res.body)) {
    return {
      state: 'valid',
      reason: '页面为选课路由,表格由前端渲染(将走隐藏窗口 DOM 提取)',
      html: res.body,
    };
  }

  return {
    state: 'unknown',
    reason: `无法判定(HTTP ${res.statusCode},正文 ${res.body.length} 字节)`,
    html: res.body,
  };
}

/** 仅判断是否已登录 */
export async function isLoggedIn(): Promise<boolean> {
  const probe = await probeAuth();
  return probe.state === 'valid';
}

// ---------------------------------------------------------------- 窗口内脚本

const READY_SCRIPT = `(() => {
  const visible = (el) => {
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
  };
  const u = ${SEL_USERNAME};
  const p = ${SEL_PASSWORD};
  const b = ${SEL_BUTTON};
  const text = document.body ? document.body.innerText : '';
  const codeInput = document.querySelector('input[placeholder*="验证码"], #CaptchaInputText, input[name*="aptcha"], input[name*="Captcha"]');
  const capImg = document.querySelector('img[src*="aptcha"], img[src*="Captcha"], .captcha-img');
  return {
    ready: Boolean(u && p && b),
    hasUsername: Boolean(u),
    hasPassword: Boolean(p),
    hasButton: Boolean(b),
    // 短信二次验证:验证码框可见但没有密码框
    needStage2: visible(codeInput) && !p,
    // 图形验证码必须按「可见性」判断:CAS 登录页常驻一个隐藏的验证码输入框
    // (input.login-captcha-pc-hidden),只判断存在会把正常情况下也误判成需要验证码,
    // 导致自动登录永远返回 needManual、真实凭据也无法自动重登。
    hasCaptcha: visible(codeInput) || visible(capImg),
    text: text.slice(0, 500)
  };
})()`;

function buildFillScript(username: string, password: string): string {
  return `(() => {
  const setVal = (input, value) => {
    const last = input.value;
    input.value = value;
    const ev = new Event('input', { bubbles: true });
    ev.simulated = true;
    const tracker = input._valueTracker;
    if (tracker) tracker.setValue(last);
    input.dispatchEvent(ev);
  };
  const u = ${SEL_USERNAME};
  const p = ${SEL_PASSWORD};
  const b = ${SEL_BUTTON};
  if (!u || !p || !b) return false;
  setVal(u, ${JSON.stringify(username)});
  setVal(p, ${JSON.stringify(password)});
  setTimeout(() => {
    if (!b.disabled) { b.click(); }
    else { setTimeout(() => b.click(), 800); }
  }, 500);
  return true;
})()`;
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor<T>(
  probe: () => Promise<T | null>,
  timeoutMs: number,
  intervalMs = 600,
): Promise<T | null> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await probe();
    if (value !== null && value !== undefined) return value;
    await wait(intervalMs);
  }
  return null;
}

async function evalInWindow<T>(win: BrowserWindow, script: string): Promise<T | null> {
  if (win.isDestroyed()) return null;
  try {
    return (await win.webContents.executeJavaScript(script, true)) as T;
  } catch {
    // 页面正在导航时 executeJavaScript 会失败,视为「还没ready」交给外层重试
    return null;
  }
}

interface ReadyProbe {
  ready: boolean;
  hasUsername: boolean;
  hasPassword: boolean;
  hasButton: boolean;
  needStage2: boolean;
  hasCaptcha: boolean;
  text: string;
}

// ---------------------------------------------------------------- 自动登录

export type AutoLoginOutcome =
  | 'success'
  | 'noCredentials'
  | 'needManual'
  | 'timeout'
  | 'error';

export interface AutoLoginResult {
  outcome: AutoLoginOutcome;
  detail: string;
}

export interface AutoLoginOptions {
  /** 等待表单出现的上限 */
  formTimeoutMs?: number;
  /** 等待跳转完成的上限 */
  navTimeoutMs?: number;
}

/**
 * 在隐藏窗口中执行自动登录。
 *
 * 回退策略(PLAN.md 4.4):一旦出现验证码 / 短信二次验证 / 超时,
 * **不再反复重试**,直接返回 needManual 交由调用方转入告警态请求人工介入。
 */
export async function runAutoLogin(options: AutoLoginOptions = {}): Promise<AutoLoginResult> {
  const cfg = getConfig();
  const username = cfg.credentials.username;
  const password = cfg.credentials.password;

  if (!username || !password) {
    return { outcome: 'noCredentials', detail: 'config.json 中未填写凭据' };
  }

  const formTimeoutMs = options.formTimeoutMs ?? 30000;
  const navTimeoutMs = options.navTimeoutMs ?? 45000;

  const win = getHiddenWindow();

  try {
    log.info('开始自动登录(隐藏窗口)');
    await win.loadURL(URLS.casLogin);

    // 1) 等待登录表单渲染(SPA,需要轮询)
    //
    // 注意:探测脚本在「页面已加载但 SPA 还没渲染」时也会返回非 null(各项都是 false)。
    // 若直接把探测结果交给 waitFor,它会在 loadURL 刚结束的瞬间就返回,把「还没渲染」
    // 误判成「结构未识别」而立刻放弃。因此这里只在真正出现表单 / 短信阶段 / 验证码时
    // 才结束等待;超时后用最后一次探测结果给出可排查的详情。
    const lastProbe: { probe: ReadyProbe | null } = { probe: null };
    const state = await waitFor<ReadyProbe>(
      async () => {
        const probe = await evalInWindow<ReadyProbe>(win, READY_SCRIPT);
        if (!probe) return null;
        lastProbe.probe = probe;
        return probe.ready || probe.needStage2 || probe.hasCaptcha ? probe : null;
      },
      formTimeoutMs,
    );

    if (!state) {
      const p = lastProbe.probe;
      const detail = p
        ? `等待登录表单超时(${formTimeoutMs}ms);最后探测:用户名:${p.hasUsername},` +
          ` 密码:${p.hasPassword}, 按钮:${p.hasButton},` +
          ` 正文:${p.text.slice(0, 120)}`
        : `等待登录表单超时(${formTimeoutMs}ms);页面未就绪`;
      return { outcome: 'timeout', detail };
    }

    if (state.needStage2) {
      log.warn('检测到短信二次验证阶段,转人工');
      return { outcome: 'needManual', detail: '服务端要求短信验证码' };
    }

    if (state.hasCaptcha) {
      log.warn('检测到图形验证码,自动登录不可行');
      return { outcome: 'needManual', detail: '登录页要求图形验证码' };
    }

    if (!state.ready) {
      log.warn('登录表单结构未识别:', state);
      return {
        outcome: 'error',
        detail: `未找到登录表单元素(用户名:${state.hasUsername}, 密码:${state.hasPassword}, 按钮:${state.hasButton})`,
      };
    }

    // 2) 填表并点击(等 1 秒,与原脚本节奏一致,给 React 受控组件留时间)
    await wait(1000);
    const filled = await evalInWindow<boolean>(win, buildFillScript(username, password));
    if (!filled) {
      return { outcome: 'error', detail: '填表脚本执行失败(元素可能在执行瞬间被卸载)' };
    }
    log.info('凭据已填入,已触发登录按钮');
    log.debug('填写结果已提交'); // 不记录任何凭据内容

    // 3) 等待离开 CAS 域名
    const navigated = await waitFor<boolean>(
      async () => {
        if (win.isDestroyed()) return null;
        const url = win.webContents.getURL();
        if (url.includes('auth.seu.edu.cn')) {
          // 仍停留在 CAS:顺手检查是否被要求二次验证
          const probe = await evalInWindow<ReadyProbe>(win, READY_SCRIPT);
          if (probe?.needStage2) return null;
          return null;
        }
        return url.length > 0 ? true : null;
      },
      navTimeoutMs,
    );

    if (!navigated) {
      return { outcome: 'timeout', detail: `点击登录后 ${navTimeoutMs}ms 内未跳出认证页` };
    }

    // 4) 用服务端响应做最终判定 —— 不看页面 hash,不看心跳
    const probe = await probeAuth();
    if (probe.state === 'valid') {
      log.info('自动登录成功:', probe.reason);
      return { outcome: 'success', detail: probe.reason };
    }

    log.warn('登录流程结束但会话未生效:', probe.reason);
    return {
      outcome: probe.state === 'expired' ? 'timeout' : 'error',
      detail: `登录后探测仍失败:${probe.reason}`,
    };
  } catch (err) {
    return { outcome: 'error', detail: `自动登录异常:${(err as Error).message}` };
  }
}
