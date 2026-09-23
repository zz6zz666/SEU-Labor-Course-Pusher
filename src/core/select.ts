/**
 * 自动选课(默认关闭)。
 *
 * 选课协议(逆自站点 `assets/dist/js/Areas/SJItemKaiKe/XuanKe/Index.js`):
 * - 每行第 1 个 td 内藏 `<td-data data-name="SJItemID|SJItemKaiKeID" ...>`;
 * - 「选课」是 `data-command="StudentXuanKe"` 的按钮,提交
 *   `POST /SJItemKaiKe/XuanKe/StudentXuanKe`,参数 `{ SJItemID, SJItemKaiKeID }`;
 * - 请求由站点自身的 `changeAjax.postAntiForgery` 发出,自动附带
 *   `__RequestVerificationToken`(取自页面隐藏 input,同时放入 body 与 header);
 * - 响应 `{ Success: boolean, Message: string }`。
 *
 * 为不逆向表单编码与令牌管理,**不重造请求**,而是在隐藏窗口(与后台共用会话)里
 * 加载选课页,直接调用站点自己的 `changeAjax.postAntiForgery` —— 与用户点「选课」等价,
 * 只是省去了确认弹窗。执行结果由返回的 Promise 收集。
 */
import { getHiddenWindow } from '../main/windows';
import { URLS } from './session';
import { createLogger } from '../logger';
import { withHiddenWindowLock } from './hidden-window-lock';

const log = createLogger('select');

export interface SelectTarget {
  uniqueId: string;
  itemName: string;
  sjItemId: string;
  sjItemKaiKeId: string;
}

/** selected=选课成功;already=按钮不可用(已选/已满);notFound=页面上找不到;failed=提交失败 */
export type SelectStatus = 'selected' | 'already' | 'notFound' | 'failed';

export interface SelectOutcome {
  target: SelectTarget;
  status: SelectStatus;
  message: string;
}

interface RawOutcome {
  uniqueId: string;
  status: SelectStatus;
  message: string;
}

function buildScript(targets: SelectTarget[]): string {
  return `(async () => {
  const targets = ${JSON.stringify(targets)};
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const tableReady = () => Boolean(document.getElementById('c_app_page_index_XuanKe_table'));
  const scriptsReady = () =>
    Boolean(window.jQuery && window.changeAjax && typeof window.changeAjax.postAntiForgery === 'function');

  const deadline = Date.now() + 20000;
  while (Date.now() < deadline && !(tableReady() && scriptsReady())) { await sleep(400); }
  if (!scriptsReady()) return { ok: false, error: '选课页脚本未就绪', results: [] };
  if (!tableReady()) return { ok: false, error: '选课表格未出现', results: [] };

  const $ = window.jQuery;
  const rows = [];
  $('#c_app_page_index_XuanKe_table tbody>tr.c--tr, #c_app_page_index_XuanKe_table tbody>tr.c-tr').each(function () {
    const $tr = $(this);
    let data = $tr.data('trData');
    if (!data) {
      data = {};
      $tr.find('td .hidden.td-data td-data').each(function () {
        const k = $(this).attr('data-name');
        if (k) data[k] = $(this).attr('data-value');
      });
      $tr.data('trData', data);
    }
    rows.push({ $tr: $tr, data: data });
  });

  const results = [];
  for (const t of targets) {
    const found = rows.find((r) =>
      String(r.data.SJItemID || '') === String(t.sjItemId) &&
      String(r.data.SJItemKaiKeID || '') === String(t.sjItemKaiKeId));
    if (!found) {
      results.push({ uniqueId: t.uniqueId, status: 'notFound', message: '页面上找不到对应行' });
      continue;
    }
    const $btn = found.$tr.find('[data-command="StudentXuanKe"]');
    if (!$btn.length) {
      results.push({ uniqueId: t.uniqueId, status: 'notFound', message: '找不到选课按钮' });
      continue;
    }
    if ($btn.prop('disabled') || $btn.hasClass('c--lock')) {
      results.push({ uniqueId: t.uniqueId, status: 'already', message: '按钮不可用(已选或已满)' });
      continue;
    }
    const outcome = await new Promise((resolve) => {
      let done = false;
      const finish = (v) => { if (!done) { done = true; resolve(v); } };
      const timer = setTimeout(() => finish({ success: false, message: '提交超时' }), 15000);
      try {
        window.changeAjax.postAntiForgery({
          url: '/SJItemKaiKe/XuanKe/StudentXuanKe',
          data: { SJItemID: t.sjItemId, SJItemKaiKeID: t.sjItemKaiKeId },
          success: (r) => { clearTimeout(timer); finish({ success: !!(r && r.Success), message: (r && r.Message) || '' }); },
          error: () => { clearTimeout(timer); finish({ success: false, message: '请求失败' }); }
        });
      } catch (e) {
        clearTimeout(timer);
        finish({ success: false, message: String(e) });
      }
    });
    if (outcome.success) $btn.prop('disabled', true);
    results.push({
      uniqueId: t.uniqueId,
      status: outcome.success ? 'selected' : 'failed',
      message: outcome.message
    });
    await sleep(350);
  }
  return { ok: true, results: results };
})()`;
}

/**
 * 在隐藏窗口中对给定课程执行选课。
 * 调用方负责决定重试策略(本函数只报告单次结果)。
 */
export async function selectCourses(targets: SelectTarget[]): Promise<SelectOutcome[]> {
  if (targets.length === 0) return [];

  return withHiddenWindowLock(async () => {
    const win = getHiddenWindow();
    try {
      await win.loadURL(URLS.coursePage);
    } catch (err) {
      const message = `隐藏窗口加载选课页失败:${(err as Error).message}`;
      log.error(message);
      return targets.map((target) => ({ target, status: 'failed' as const, message }));
    }

    try {
      const res = (await win.webContents.executeJavaScript(buildScript(targets), true)) as
        | { ok: boolean; error?: string; results: RawOutcome[] }
        | null;

      if (!res || !res.ok) {
        const message = res?.error ?? '选课脚本执行失败';
        log.warn(message);
        return targets.map((target) => ({ target, status: 'failed' as const, message }));
      }

      const byId = new Map(res.results.map((r) => [r.uniqueId, r]));
      const outcomes: SelectOutcome[] = targets.map((target) => {
        const r = byId.get(target.uniqueId);
        return {
          target,
          status: (r?.status ?? 'failed') as SelectStatus,
          message: r?.message ?? '未返回结果',
        };
      });

      const summary = outcomes.reduce<Record<string, number>>((acc, o) => {
        acc[o.status] = (acc[o.status] ?? 0) + 1;
        return acc;
      }, {});
      log.info('自动选课执行结果:', summary);
      return outcomes;
    } catch (err) {
      const message = `选课脚本异常:${(err as Error).message}`;
      log.error(message);
      return targets.map((target) => ({ target, status: 'failed' as const, message }));
    }
  });
}
