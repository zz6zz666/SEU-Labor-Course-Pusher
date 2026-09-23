/**
 * 隐藏窗口互斥锁。
 *
 * 隐藏窗口同时被「自动登录」「DOM 兜底解析」「自动选课」复用。
 * watcher 的每次 tick 本身是串行的,但这里仍做一层互斥,
 * 避免未来出现并发调用时互相抢占同一个窗口。
 */
let busy: Promise<unknown> = Promise.resolve();

export function withHiddenWindowLock<T>(fn: () => Promise<T>): Promise<T> {
  const next = busy.then(fn, fn);
  busy = next.catch(() => undefined);
  return next;
}
