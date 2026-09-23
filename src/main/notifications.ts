/**
 * Windows 原生通知(本机即时通道)。
 *
 * 要点(PLAN.md 8.3):
 * - 需通过 electron-builder 打包并设置 appId,通知才会正确显示应用名称
 *   (开发模式下可能以 "Electron" 名义出现,属正常现象)
 * - 设置 `timeoutType: 'never'` 避免通知过快消失,否则用户来不及点击
 *
 * 点击行为按事件类型分发(PLAN.md 8.2):
 * - 发现新课程 → 内置浏览器打开选课页(与后台共用会话,打开即已登录)
 * - 登录失效   → 内置浏览器打开登录页
 * - 运行异常   → 打开日志目录
 * - 每日汇总   → 不弹窗
 */
import { Notification, nativeImage } from 'electron';
import { getConfig } from '../config';
import { createLogger } from '../logger';
import type { NotifyEvent, NotifyEventType } from '../types';
import { TRAY_ICON_PNG_BASE64 } from './icon-data';
import { openInteractiveWindow, openLogFolder, openLoginWindow } from './windows';

const log = createLogger('notifications');

let icon: Electron.NativeImage | null = null;

/**
 * 「登录失效」通知的点击处理由 main/index.ts 注入。
 *
 * 不能只在这里 openLoginWindow:托盘入口走的是 index 的 openLoginAndTrack(),
 * 窗口关闭后会校验登录态并让 watcher 立即恢复;通知入口若只开窗不跟踪,
 * 用户重新登录成功后仍要等下一次退避轮询(最长 30 分钟)才恢复监控。
 */
let authExpiredClickHandler: (() => void) | null = null;

export function setAuthExpiredClickHandler(handler: (() => void) | null): void {
  authExpiredClickHandler = handler;
}

function getIcon(): Electron.NativeImage {
  if (!icon) icon = nativeImage.createFromBuffer(Buffer.from(TRAY_ICON_PNG_BASE64, 'base64'));
  return icon;
}

/** 点击本机通知后的分发 */
async function handleClick(type: NotifyEventType): Promise<void> {
  const cfg = getConfig();
  log.info(`本机通知被点击:${type}`);

  switch (type) {
    case 'newCourse':
      if (cfg.push.windows.openBrowserOnClick) {
        // 统一用内置浏览器:与后台共用会话,打开即已登录,可直接选课
        openInteractiveWindow({ mode: 'course', firstRun: false });
      } else {
        log.info('配置已关闭「点击打开浏览器」,忽略');
      }
      break;
    case 'autoSelect':
      // 自动选课结果:点击打开选课页核对
      if (cfg.push.windows.openBrowserOnClick) {
        openInteractiveWindow({ mode: 'course', firstRun: false });
      }
      break;
    case 'authExpired':
      // 登录一律走内置浏览器(后台需要这份会话);优先走带登录态跟踪的完整流程
      if (authExpiredClickHandler) {
        authExpiredClickHandler();
      } else {
        openLoginWindow(false);
      }
      break;
    case 'runtimeError':
      await openLogFolder();
      break;
    case 'dailySummary':
      // 汇总通知不弹窗,仅查看
      break;
  }
}

/**
 * 活跃通知的强引用集合。
 *
 * Electron 的 Notification 对象若只存在于局部变量,随时可能被 GC 回收;
 * 一旦对象被回收,Windows 侧再点击这条通知就**不会再触发 click 事件**
 * （表现为:通知能显示、点击无任何反应）。因此这里持有引用,
 * 直到用户点击 / 关闭 / 发送失败时再释放。
 */
const activeNotifications = new Set<Notification>();

export async function sendNativeNotification(event: NotifyEvent): Promise<boolean> {
  const cfg = getConfig().push.windows;

  if (!cfg.enabled) {
    log.debug('本机通知通道已在配置中关闭');
    return false;
  }
  if (!Notification.isSupported()) {
    log.warn('当前系统不支持原生通知');
    return false;
  }

  try {
    const notification = new Notification({
      title: event.title,
      body: event.body,
      icon: getIcon(),
      // 常驻直到用户处理,避免来不及点击
      timeoutType: 'never',
      silent: false,
    });

    activeNotifications.add(notification);
    const release = (): void => {
      activeNotifications.delete(notification);
    };

    notification.on('click', () => {
      release();
      void handleClick(event.type);
    });
    notification.on('close', release);
    notification.on('failed', (_e, error) => {
      release();
      log.warn('本机通知发送失败:', error);
    });

    notification.show();
    log.info(`本机通知已发出(${event.type})`);
    return true;
  } catch (err) {
    log.error('本机通知异常:', err);
    return false;
  }
}

export function getNotificationIcon(): Electron.NativeImage {
  return getIcon();
}
