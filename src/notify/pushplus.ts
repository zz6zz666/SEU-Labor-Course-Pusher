/**
 * PushPlus 微信通道(远程通道)。
 * 移植自原脚本 pushToWechat,改为在主进程直接发请求。
 */
import { getConfig } from '../config';
import { createLogger } from '../logger';
import { URLS } from '../core/session';
import { postJson } from '../core/fetch';
import type { NotifyEvent } from '../types';

const log = createLogger('pushplus');

interface PushplusResponse {
  code?: number;
  msg?: string;
  data?: unknown;
}

export interface PushResult {
  ok: boolean;
  detail: string;
}

export async function sendViaPushPlus(event: NotifyEvent): Promise<PushResult> {
  const cfg = getConfig().push.pushplus;

  if (!cfg.enabled) {
    return { ok: false, detail: 'PushPlus 通道已在配置中关闭' };
  }
  if (!cfg.token) {
    log.warn('未配置 PushPlus Token,跳过微信推送');
    return { ok: false, detail: '未配置 PushPlus Token' };
  }

  const payload = {
    token: cfg.token,
    title: event.title,
    content: event.markdown,
    template: 'markdown',
  };

  const res = await postJson(URLS.pushplus, payload, 15000);

  if (!res.ok) {
    const detail = res.error ?? `HTTP ${res.statusCode}`;
    log.error('微信推送失败:', detail);
    return { ok: false, detail };
  }

  let parsed: PushplusResponse | null = null;
  try {
    parsed = JSON.parse(res.body) as PushplusResponse;
  } catch {
    /* 保持 null,下面按原文判断 */
  }

  if (parsed && typeof parsed.code === 'number' && parsed.code !== 200) {
    log.error(`微信推送被拒绝: code=${parsed.code} msg=${parsed.msg ?? ''}`);
    return { ok: false, detail: `PushPlus code=${parsed.code} ${parsed.msg ?? ''}` };
  }

  log.info(`微信推送成功(${event.type})`);
  return { ok: true, detail: 'ok' };
}
