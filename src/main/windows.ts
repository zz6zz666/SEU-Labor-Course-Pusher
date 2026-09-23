/**
 * 窗口层:内置浏览器窗口、隐藏窗口,以及必要的系统跳转。
 *
 * 登录与选课**统一走内置浏览器**(BrowserWindow,persist:seu-labor 分区):
 * 与后台共用会话,打开即是已登录态,不需要用户在自己浏览器里再登一次。
 * 仅「外部链接」「日志目录」「config.json」这类系统动作交给 shell。
 */
import path from 'path';
import { BrowserWindow, shell } from 'electron';
import { PARTITION, URLS } from '../core/session';
import { getLogDir, createLogger } from '../logger';
import { resolvePaths } from '../paths';

const log = createLogger('windows');

const PRELOAD_PATH = path.join(__dirname, '..', 'renderer', 'preload.js');

export type WindowMode = 'login' | 'course';

interface WindowContext {
  mode: WindowMode;
  firstRun: boolean;
}

let hiddenWindow: BrowserWindow | null = null;
let interactiveWindow: BrowserWindow | null = null;

/**
 * 窗口上下文登记表(以 webContents.id 为键)。
 * preload 通过 IPC 询问自己处于哪种模式,避免依赖 sandboxed preload 里不一定存在的 process.argv。
 */
const windowContexts = new Map<number, WindowContext>();

export function getWindowContextFor(webContentsId: number): WindowContext | null {
  return windowContexts.get(webContentsId) ?? null;
}

/** 发给 preload 的上下文(同时通过命令行参数下发,便于调试时肉眼确认) */
function contextArgs(ctx: WindowContext): string[] {
  return [`--seu-window-mode=${ctx.mode}`, `--seu-first-run=${ctx.firstRun ? '1' : '0'}`];
}

function baseWebPreferences(extra: Electron.WebPreferences = {}): Electron.WebPreferences {
  return {
    partition: PARTITION,
    contextIsolation: true,
    nodeIntegration: false,
    sandbox: true,
    ...extra,
  };
}

// ---------------------------------------------------------------- 隐藏窗口

/**
 * 隐藏窗口:自动登录在这里执行(PLAN.md 4.4)。
 * `backgroundThrottling: false` 是关键 —— 避免隐藏窗口被浏览器节流导致 DOM 操作停摆。
 */
export function getHiddenWindow(): BrowserWindow {
  if (hiddenWindow && !hiddenWindow.isDestroyed()) return hiddenWindow;

  hiddenWindow = new BrowserWindow({
    show: false,
    width: 1200,
    height: 860,
    webPreferences: baseWebPreferences({ backgroundThrottling: false }),
  });

  hiddenWindow.on('closed', () => {
    hiddenWindow = null;
  });

  log.debug('隐藏窗口已创建');
  return hiddenWindow;
}

export function destroyHiddenWindow(): void {
  if (hiddenWindow && !hiddenWindow.isDestroyed()) hiddenWindow.destroy();
  hiddenWindow = null;
}

// ---------------------------------------------------------------- 交互窗口

function closeInteractiveWindow(): void {
  if (interactiveWindow && !interactiveWindow.isDestroyed()) {
    interactiveWindow.destroy();
  }
  interactiveWindow = null;
}

/** 自定义外链/新窗口行为:目标站点内跳转留在窗口内,其余交给系统默认浏览器 */
function installNavigationGuard(win: BrowserWindow): void {
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (isSchoolUrl(url)) {
      void win.loadURL(url);
    } else {
      void shell.openExternal(url);
    }
    return { action: 'deny' };
  });

  win.webContents.on('will-navigate', (event, url) => {
    if (!isSchoolUrl(url)) {
      event.preventDefault();
      void shell.openExternal(url);
    }
  });
}

function isSchoolUrl(url: string): boolean {
  try {
    const host = new URL(url).hostname;
    return host === 'labor.seu.edu.cn' || host === 'auth.seu.edu.cn' || host.endsWith('.seu.edu.cn');
  } catch {
    return false;
  }
}

/**
 * 打开内置浏览器窗口。
 * - mode = 'login'  → CAS 登录页
 * - mode = 'course' → 选课页(点击「发现新课程」通知、托盘菜单均走这里)
 *
 * 因为与后台共用分区,用户在这里登录完成后,后台**立即**就是已登录态。
 */
export function openInteractiveWindow(ctx: WindowContext): BrowserWindow {
  const url = ctx.mode === 'login' ? URLS.casLogin : URLS.coursePage;

  if (interactiveWindow && !interactiveWindow.isDestroyed()) {
    interactiveWindow.destroy();
    interactiveWindow = null;
  }

  const win = new BrowserWindow({
    width: 1080,
    height: 780,
    minWidth: 720,
    minHeight: 560,
    title: ctx.mode === 'login' ? '登录 · 东南大学统一身份认证' : '选课 · 劳动教育',
    autoHideMenuBar: true,
    show: false,
    webPreferences: baseWebPreferences({
      preload: PRELOAD_PATH,
      additionalArguments: contextArgs(ctx),
      backgroundThrottling: false,
    }),
  });

  interactiveWindow = win;
  // 提前取 id:窗口 closed 时 webContents 已销毁,再访问 win.webContents.id 会抛
  // "Object has been destroyed",导致 windowContexts 里的登记项清不掉。
  const webContentsId = win.webContents.id;
  windowContexts.set(webContentsId, ctx);
  installNavigationGuard(win);

  win.once('ready-to-show', () => {
    // 内置浏览器一律默认最大化打开:选课页表格较宽,最大化更省事
    win.maximize();
    win.show();
  });
  win.on('closed', () => {
    if (interactiveWindow === win) interactiveWindow = null;
    windowContexts.delete(webContentsId);
    log.debug('内置浏览器窗口已关闭');
  });

  void win.loadURL(url).catch((err) => log.error('加载内置窗口失败:', err));
  log.info(`已打开内置浏览器窗口(mode=${ctx.mode})`);
  return win;
}

export function getInteractiveWindow(): BrowserWindow | null {
  return interactiveWindow && !interactiveWindow.isDestroyed() ? interactiveWindow : null;
}

/** 弹出登录页供人工完成短信验证与重新登录 */
export function openLoginWindow(firstRun = false): BrowserWindow {
  return openInteractiveWindow({ mode: 'login', firstRun });
}

// ---------------------------------------------------------------- 系统动作

/** 用系统资源管理器打开日志目录(点击「运行异常」通知时) */
export async function openLogFolder(): Promise<void> {
  try {
    await shell.openPath(getLogDir());
  } catch (err) {
    log.error('打开日志目录失败:', err);
  }
}

/** 用系统默认编辑器打开 config.json */
export async function openConfigFile(): Promise<void> {
  try {
    const err = await shell.openPath(resolvePaths().configPath);
    if (err) log.warn('打开配置文件返回:', err);
  } catch (err) {
    log.error('打开配置文件失败:', err);
  }
}

/** 用系统默认浏览器打开外部链接(仅允许 http/https,其余一律拒绝) */
export async function openExternalUrl(url: string): Promise<boolean> {
  if (!/^https?:\/\//i.test(url)) {
    log.warn('拒绝打开非 http(s) 链接:', url);
    return false;
  }
  try {
    await shell.openExternal(url);
    return true;
  } catch (err) {
    log.error('打开外部链接失败:', err);
    return false;
  }
}

export function closeAllWindows(): void {
  closeInteractiveWindow();
  destroyHiddenWindow();
  closeWizardWindow();
}

// ---------------------------------------------------------------- 首启向导窗口

const WIZARD_PRELOAD_PATH = path.join(__dirname, '..', 'renderer', 'wizard-preload.js');
const WIZARD_HTML_PATH = path.join(__dirname, '..', 'renderer', 'wizard.html');

let wizardWindow: BrowserWindow | null = null;

/** 首启向导(PLAN.md 9.5):首次登录必然涉及短信验证码,向导负责把这一步讲清楚并引导完成 */
export function openWizardWindow(): BrowserWindow {
  if (wizardWindow && !wizardWindow.isDestroyed()) {
    wizardWindow.show();
    wizardWindow.focus();
    return wizardWindow;
  }

  wizardWindow = new BrowserWindow({
    width: 980,
    height: 720,
    minWidth: 860,
    minHeight: 640,
    resizable: true,
    maximizable: false,
    title: '设置向导 · SEU 劳动教育课程推送助手',
    autoHideMenuBar: true,
    show: false,
    webPreferences: {
      preload: WIZARD_PRELOAD_PATH,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  wizardWindow.once('ready-to-show', () => wizardWindow?.show());
  wizardWindow.on('closed', () => {
    wizardWindow = null;
  });

  void wizardWindow.loadFile(WIZARD_HTML_PATH).catch((err) => log.error('加载向导页失败:', err));
  return wizardWindow;
}

export function closeWizardWindow(): void {
  if (wizardWindow && !wizardWindow.isDestroyed()) wizardWindow.destroy();
  wizardWindow = null;
}
