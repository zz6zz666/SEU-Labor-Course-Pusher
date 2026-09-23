/**
 * 隐藏窗口 DOM 兜底解析。
 *
 * 触发条件(PLAN.md 13 风险表「选课表格为 AJAX 加载」):
 * 纯 HTTP 取到的 HTML 里找不到课程表格,但登录态是有效的 —— 说明表格由前端异步渲染。
 * 此时退化为「用浏览器取 DOM」:在隐藏窗口中打开选课页,直接执行原脚本的 querySelector 逻辑。
 *
 * 本文件里的提取脚本与原脚本 handleCoursePage / extractCourseInfo 保持同构,
 * 便于日后对照与回归。
 */
import { getHiddenWindow } from '../main/windows';
import { URLS } from './session';
import { createLogger } from '../logger';
import { filterCourses, type ParseResult } from './parse';
import { withHiddenWindowLock } from './hidden-window-lock';
import type { Course } from '../types';

const log = createLogger('dom-parse');

function buildExtractScript(locations: string[], categories: string[]): string {
  return `(() => {
  const cleanText = (text) => text ? text.trim().replace(/\\s+/g, ' ') : '无';
  const getWeekday = (dateStr) => {
    if (!dateStr) return '';
    const m = dateStr.match(/\\d{4}-\\d{2}-\\d{2}/);
    if (!m) return '';
    const d = new Date(m[0]);
    return isNaN(d.getTime()) ? '' : ['周日','周一','周二','周三','周四','周五','周六'][d.getDay()];
  };
  const isPureNumber = (text) => /^\\d+$/.test(text.trim());

  const table = document.getElementById('c_app_page_index_XuanKe_table');
  if (!table) return { tableFound: false, rowCount: 0, courses: [] };

  const rows = table.querySelectorAll('tbody .c--tr, tbody .c-tr');
  if (!rows.length) return { tableFound: true, rowCount: 0, courses: [] };

  const LOCATIONS = ${JSON.stringify(locations)}.map((s) => s.trim()).filter(Boolean);
  const CATEGORIES = ${JSON.stringify(categories)}.map((s) => s.trim()).filter(Boolean);

  const courses = Array.from(rows).map((row) => {
    const cellAt = (n) => row.querySelector('td:nth-child(' + n + ')');
    const col1 = cleanText(cellAt(1) ? cellAt(1).textContent : '');
    const col2 = cleanText(cellAt(2) ? cellAt(2).textContent : '');
    const isIndexInCol1 = isPureNumber(col1) && !isPureNumber(col2);
    const offset = isIndexInCol1 ? 0 : 1;

    const originalTime = cleanText(cellAt(8 + offset) ? cellAt(8 + offset).textContent : '');
    const weekday = getWeekday(originalTime);
    const 选课状态 = cleanText(cellAt(10 + offset) ? cellAt(10 + offset).textContent : '');
    const 截止状态 = cleanText(cellAt(9 + offset) ? cellAt(9 + offset).textContent : '');
    const locCell = cellAt(7 + offset);
    const locNode = locCell ? locCell.querySelector('.limit-line') : null;
    const 开课地点 = cleanText(locNode ? locNode.textContent : '');
    const 项目名称 = cleanText(cellAt(3 + offset) ? cellAt(3 + offset).textContent : '');
    const 项目类别 = cleanText(cellAt(4 + offset) ? cellAt(4 + offset).textContent : '');
    const 实施时间 = weekday ? originalTime + '（' + weekday + '）' : originalTime;

    const isFull = 选课状态.indexOf('已满') >= 0;
    const isExpired = 截止状态.indexOf('已截止') >= 0;

    let sjItemId = '';
    let sjItemKaiKeId = '';
    row.querySelectorAll('td .hidden.td-data td-data').forEach((n) => {
      const k = n.getAttribute('data-name');
      if (k === 'SJItemID') sjItemId = n.getAttribute('data-value') || '';
      if (k === 'SJItemKaiKeID') sjItemKaiKeId = n.getAttribute('data-value') || '';
    });
    if ((!sjItemId || !sjItemKaiKeId) && window.jQuery) {
      const td = window.jQuery(row).data('trData');
      if (td) {
        sjItemId = sjItemId || td.SJItemID || '';
        sjItemKaiKeId = sjItemKaiKeId || td.SJItemKaiKeID || '';
      }
    }

    return {
      uniqueId: 项目名称 + '|' + 实施时间,
      序号: isIndexInCol1 ? col1 : col2,
      项目名称: 项目名称,
      项目类别: 项目类别,
      开课地点: 开课地点,
      实施时间: 实施时间,
      选课截止时间: 截止状态,
      选课人数_容纳人数: 选课状态,
      授课教师: cleanText(cellAt(15 + offset) ? cellAt(15 + offset).textContent : ''),
      isInvalid: isFull || isExpired,
      isFull: isFull,
      isExpired: isExpired,
      // 地点=关键字白名单(包含即命中);类别=黑名单(完全匹配即排除)
      locationMatch: LOCATIONS.length === 0 ? true : LOCATIONS.some((f) => 开课地点.indexOf(f) >= 0),
      categoryAllowed: CATEGORIES.length === 0 ? true : !CATEGORIES.some((f) => 项目类别 === f),
      sjItemId: sjItemId,
      sjItemKaiKeId: sjItemKaiKeId
    };
  });

  return { tableFound: true, rowCount: rows.length, courses: courses };
})()`;
}

interface RawExtract {
  tableFound: boolean;
  rowCount: number;
  courses: Course[];
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export interface DomParseOptions {
  locations: string[];
  categories: string[];
  /** 等待表格出现的上限 */
  timeoutMs?: number;
}

/**
 * 在隐藏窗口中打开选课页并提取课程。
 * 仅在 `parseCourses` 因缺少表格而失败时调用。
 */
export async function parseCoursesViaHiddenWindow(options: DomParseOptions): Promise<ParseResult> {
  return withHiddenWindowLock(async () => {
    const timeoutMs = options.timeoutMs ?? 25000;
    const win = getHiddenWindow();
    const script = buildExtractScript(options.locations, options.categories);

    log.info('回退到隐藏窗口 DOM 提取');

    try {
      await win.loadURL(URLS.coursePage);
    } catch (err) {
      log.error('隐藏窗口加载选课页失败:', err);
      return {
        tableFound: false,
        rowCount: 0,
        courses: [],
        locationFallback: false,
        warnings: [`隐藏窗口加载失败:${(err as Error).message}`],
      };
    }

    const deadline = Date.now() + timeoutMs;
    let last: RawExtract | null = null;

    while (Date.now() < deadline) {
      try {
        const result = (await win.webContents.executeJavaScript(script, true)) as RawExtract;
        last = result;
        if (result.tableFound && result.rowCount > 0) {
          log.info(`隐藏窗口提取到 ${result.rowCount} 行`);
          return {
            tableFound: true,
            rowCount: result.rowCount,
            courses: result.courses,
            locationFallback: false,
            warnings: [],
          };
        }
        if (result.tableFound && result.rowCount === 0) {
          // 表格在但没数据,可能是渲染未完成,再等一轮
        }
      } catch {
        // 页面导航中,继续等待
      }
      await wait(1200);
    }

    const warnings = ['隐藏窗口等待课程表格超时或表格为空'];
    if (last?.tableFound) {
      return { tableFound: true, rowCount: 0, courses: [], locationFallback: false, warnings };
    }
    return { tableFound: false, rowCount: 0, courses: [], locationFallback: false, warnings };
  });
}
