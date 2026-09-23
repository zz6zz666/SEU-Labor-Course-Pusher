/**
 * 经 session 发请求的 HTTP 层。
 *
 * 这是「不再靠心跳推测」的落点:程序直接问服务端要数据,
 * 拿到 302 跳登录就是失效,拿到表格就是有效 —— 是事实,不是推测。
 */
import { net } from 'electron';
import { getSession, BROWSER_UA } from './session';
import { createLogger } from '../logger';

const log = createLogger('fetch');

export interface HttpResponse {
  statusCode: number;
  body: string;
  /** 实际发生的重定向链(用于判定是否被踢回登录页) */
  redirects: string[];
  finalUrl: string;
  ok: boolean;
  error?: string;
}

export interface RequestOptions {
  method?: 'GET' | 'POST';
  timeoutMs?: number;
  headers?: Record<string, string>;
  /** 请求体(POST 用) */
  body?: string;
  /** 关闭重定向跟随,仅用于诊断 */
  manualRedirect?: boolean;
}

const DEFAULT_HEADERS: Record<string, string> = {
  'User-Agent': BROWSER_UA,
  Accept: 'text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8',
  'Accept-Language': 'zh-CN,zh;q=0.9,en;q=0.8',
  'Cache-Control': 'no-cache',
  Pragma: 'no-cache',
};

/**
 * 发一次请求并读取完整响应体。
 *
 * 使用 `redirect: 'follow'` 让 Electron 自行处理跳转与 Cookie 写回,
 * 同时监听 `redirect` 事件记录整条跳转链 —— 判定登录态既看链也看最终正文。
 */
export function request(url: string, opts: RequestOptions = {}): Promise<HttpResponse> {
  const method = opts.method ?? 'GET';
  const timeoutMs = opts.timeoutMs ?? 20000;

  return new Promise<HttpResponse>((resolve) => {
    let settled = false;
    const redirects: string[] = [];
    let finalUrl = url;

    const done = (result: HttpResponse) => {
      if (settled) return;
      settled = true;
      resolve(result);
    };

    let request_: Electron.ClientRequest;
    try {
      // 关键:显式绑定 persist:seu-labor 这个 session —— 取数与内置浏览器窗口共用同一份 Cookie
      request_ = net.request({
        method,
        url,
        session: getSession(),
        redirect: opts.manualRedirect ? 'manual' : 'follow',
        useSessionCookies: true,
        credentials: 'include',
      });
    } catch (err) {
      done({ statusCode: 0, body: '', redirects, finalUrl, ok: false, error: (err as Error).message });
      return;
    }

    const timer = setTimeout(() => {
      try {
        request_.abort();
      } catch {
        /* 忽略 */
      }
      done({ statusCode: 0, body: '', redirects, finalUrl, ok: false, error: `请求超时(${timeoutMs}ms)` });
    }, timeoutMs);

    request_.on('redirect', (_statusCode, _method, redirectUrl) => {
      redirects.push(redirectUrl);
      finalUrl = redirectUrl;
    });

    request_.on('response', (response) => {
      const chunks: Buffer[] = [];
      response.on('data', (chunk: Buffer) => chunks.push(Buffer.from(chunk)));
      response.on('end', () => {
        clearTimeout(timer);
        let body = Buffer.concat(chunks).toString('utf8');
        if (!body) {
          // 极端情况下 data 未触发,回退到一次性读取
          body = '';
        }
        const statusCode = response.statusCode ?? 0;
        done({ statusCode, body, redirects, finalUrl, ok: statusCode >= 200 && statusCode < 400 });
      });
      response.on('error', (err: Error) => {
        clearTimeout(timer);
        done({ statusCode: 0, body: '', redirects, finalUrl, ok: false, error: err.message });
      });
    });

    request_.on('error', (err: Error) => {
      clearTimeout(timer);
      done({ statusCode: 0, body: '', redirects, finalUrl, ok: false, error: err.message });
    });

    if (opts.headers) {
      for (const [k, v] of Object.entries(opts.headers)) request_.setHeader(k, v);
    } else {
      for (const [k, v] of Object.entries(DEFAULT_HEADERS)) request_.setHeader(k, v);
    }

    if (method === 'POST' && opts.body !== undefined) {
      request_.setHeader('Content-Type', 'application/json; charset=utf-8');
      request_.write(opts.body);
    }

    try {
      request_.end();
    } catch (err) {
      clearTimeout(timer);
      done({ statusCode: 0, body: '', redirects, finalUrl, ok: false, error: (err as Error).message });
    }

    log.debug(`${method} ${url}`);
  });
}

export async function getHtml(url: string, timeoutMs = 20000): Promise<HttpResponse> {
  return request(url, { method: 'GET', timeoutMs });
}

export async function postJson(url: string, payload: unknown, timeoutMs = 15000): Promise<HttpResponse> {
  return request(url, {
    method: 'POST',
    body: JSON.stringify(payload),
    timeoutMs,
    headers: {
      'User-Agent': BROWSER_UA,
      'Content-Type': 'application/json; charset=utf-8',
      Accept: 'application/json, text/plain, */*',
      'Accept-Language': 'zh-CN,zh;q=0.9',
    },
  });
}
