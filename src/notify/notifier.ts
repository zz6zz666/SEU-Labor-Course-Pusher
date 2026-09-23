/**
 * 通知路由。
 *
 * 核心规则(PLAN.md 8.1/8.2):**两条通道平等并行,不按事件类型分工**。
 * 任何事件都同时经由 Windows 原生通知与微信通知发出 —— 既是可靠性冗余,
 * 也同时覆盖「人在电脑前」与「人在外面」两种场景。
 * 事件类型的差别只体现在「点击本机通知后打开什么」(由 main/notifications.ts 决定)。
 */
import type { NotifyEvent } from '../types';
import { createLogger } from '../logger';
import { sendViaPushPlus } from './pushplus';

const log = createLogger('notifier');

/** 原生通知通道由 main 层注入,避免 notify/ 反向依赖 main/ */
export type NativeChannel = (event: NotifyEvent) => Promise<boolean>;

let nativeChannel: NativeChannel | null = null;

export function setNativeChannel(channel: NativeChannel | null): void {
  nativeChannel = channel;
}

export interface NotifyReport {
  type: NotifyEvent['type'];
  windows: boolean;
  pushplus: boolean;
}

const recent: Array<{ at: string; report: NotifyReport; title: string }> = [];

export function getRecentNotifications(limit = 20): Array<{ at: string; report: NotifyReport; title: string }> {
  return recent.slice(-limit).reverse();
}

/**
 * 双通道并行发出。任一条通道失败不影响另一条,也不抛出异常 ——
 * 通知失败绝不能把监控主循环带崩。
 */
export async function notify(event: NotifyEvent): Promise<NotifyReport> {
  log.info(`推送事件 [${event.type}] ${event.title}`);

  const [nativeResult, pushResult] = await Promise.allSettled([
    nativeChannel ? nativeChannel(event) : Promise.resolve(false),
    sendViaPushPlus(event),
  ]);

  const report: NotifyReport = {
    type: event.type,
    windows: nativeResult.status === 'fulfilled' ? nativeResult.value : false,
    pushplus: pushResult.status === 'fulfilled' ? pushResult.value.ok : false,
  };

  if (nativeResult.status === 'rejected') {
    log.warn('本机通知通道异常:', nativeResult.reason);
  }
  if (pushResult.status === 'rejected') {
    log.warn('微信通道异常:', pushResult.reason);
  }

  log.info(
    `推送结果 [${event.type}] 本机${report.windows ? '成功' : '未送达'} / 微信${report.pushplus ? '成功' : '未送达'}`,
  );

  recent.push({ at: new Date().toISOString(), report, title: event.title });
  if (recent.length > 100) recent.splice(0, recent.length - 100);

  return report;
}
