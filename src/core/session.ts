/**
 * 会话分区:守护进程自己的 Cookie 仓库。
 *
 * 关键设计(PLAN.md 4.3 / 4.5):
 * - 后台取数与内置浏览器窗口共用 `persist:seu-labor` 分区 → 登录一次,两边都生效,
 *   窗口关闭后后台立即就是已登录状态,无需任何 Cookie 迁移。
 * - 分区数据落在 userData/Partitions/seu-labor(开发态即 data/Partitions/)。
 */
import { app, session, type Session } from 'electron';
import { createLogger } from '../logger';

const log = createLogger('session');

export const PARTITION = 'persist:seu-labor';

/**
 * 浏览器 UA —— 实测侦察结论(PLAN.md 11):labor.seu.edu.cn 对 UA 敏感,
 * 使用默认 UA 访问 /AuthServer/Login 会被 302 到 /Error/NotFound。
 */
export const BROWSER_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36';

export const URLS = {
  /** 选课页 —— 后台唯一的取数入口 */
  coursePage: 'https://labor.seu.edu.cn/SJItemKaiKe/XuanKe/Index',
  /** 旧登录入口(会自动转 CAS) */
  legacyLogin: 'https://labor.seu.edu.cn/AuthServer/Login',
  /** 统一身份认证(CAS)登录页 */
  casLogin:
    'https://auth.seu.edu.cn/dist/#/dist/main/login?service=https://labor.seu.edu.cn/UnifiedAuth/CASLogin',
  /** 登录成功后的落点 */
  laborHome: 'https://labor.seu.edu.cn/System/Home',
  laborOrigin: 'https://labor.seu.edu.cn',
  authOrigin: 'https://auth.seu.edu.cn',
  pushplus: 'https://www.pushplus.plus/send',
  /** 心跳用的轻量资源(仅用于「探活探测」,不用于登录态判定) */
  favicon: 'https://labor.seu.edu.cn/favicon.ico',
} as const;

let cached: Session | null = null;

export function getSession(): Session {
  if (cached) return cached;

  const s = session.fromPartition(PARTITION);
  s.setUserAgent(BROWSER_UA);

  // 站点使用自签/非常规证书时不影响取数;此处保持默认严格校验,不放开。
  s.setPermissionRequestHandler((_wc, _permission, callback) => callback(false));

  cached = s;
  log.info('会话分区已就绪:', PARTITION);
  return s;
}

/** 启动早期调用:让所有隐式请求也带上浏览器 UA */
export function installUserAgentFallback(): void {
  app.userAgentFallback = BROWSER_UA;
}

/** 诊断用:列出当前分区内与目标站点相关的 Cookie 名(不输出值) */
export async function describeCookies(): Promise<string[]> {
  try {
    const cookies = await getSession().cookies.get({});
    return cookies
      .filter((c) => /seu\.edu\.cn$/.test((c.domain ?? '').replace(/^\./, '')))
      .map((c) => `${c.name}@${c.domain ?? '?'}`);
  } catch {
    return [];
  }
}

/** 清空分区会话(托盘菜单「清除登录态」用) */
export async function clearSession(): Promise<void> {
  const s = getSession();
  await s.clearStorageData({ storages: ['cookies', 'localstorage', 'cachestorage'] });
  log.warn('已清空会话分区数据');
}
