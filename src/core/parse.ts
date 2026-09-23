/**
 * 选课表格解析(可替换实现)。
 *
 * 与原脚本的关系(PLAN.md 第 10 节「必须原样保留的解析细节」):
 * - 表格 id:`c_app_page_index_XuanKe_table`
 * - 行选择器:`tbody .c--tr`(同时兼容 `.c-tr`,防服务端改类名)
 * - 列偏移:第 1 列为纯数字则 offset = 0,否则 offset = 1
 * - 列索引(含 offset):序号 / 项目名称[3] / 项目类别[4] / 开课地点[7] 的 `.limit-line` /
 *   实施时间[8] / 选课截止[9] / 选课状态[10] / 授课教师[15]
 * - 过滤:不含「已满」、不含「已截止」、地点与类别筛选命中
 * - 去重键:`项目名称|实施时间`
 *
 * 唯一的实现差异:原脚本在浏览器 DOM 里跑 querySelector,这里改为对 HTML 字符串做
 * 标签级解析,以便在主进程用 `net.request` 取数后直接解析——这正是「零标签页」的前提。
 * 若站点改为 AJAX 加载表格(纯 HTTP 拿不到数据),由 `core/dom-parse.ts` 走隐藏窗口兜底。
 */
import type { Course } from '../types';

export const COURSE_TABLE_ID = 'c_app_page_index_XuanKe_table';

/** 登录页特征(仅在未找到课程表格时才用于分类) */
const LOGIN_MARKERS = [
  'casLogin',
  'needCaptcha',
  'LoginSuccess',
  '统一身份认证',
  'XueXiaoCode',
  'CaptchaDeText',
  'CaptchaInputText',
  '__RequestVerificationToken',
  'input-username-pc',
  'input-username-mobile',
  'AuthServer/Login',
];

export interface ParseResult {
  /** 表格是否存在 */
  tableFound: boolean;
  /** 有效数据行数(未做筛选前) */
  rowCount: number;
  courses: Course[];
  /** 是否走了「地点回退」路径(未找到 .limit-line 结构时) */
  locationFallback: boolean;
  warnings: string[];
}

// ---------------------------------------------------------------- HTML 工具

/**
 * 开标签属性部分的匹配片段。
 *
 * 不能写成 `[^>]*`:站点会在属性值里出现 `>`,例如:
 *   <span class="c-link--line c--view-SJItem" data-responsive--bind-click="td>span.c-link--line.c--view-SJItem">
 * 用 `[^>]*` 会在属性值里的 `>` 处截断,把 `span.c-link--line.c--view-SJItem">` 当成正文,
 * 污染项目名称与去重键。这里把「带引号的属性值」整体匹配,引号内的 `>` 不再截断。
 */
const TAG_ATTRS = `(?:[^>"']|"[^"]*"|'[^']*')*`;

/** 匹配任意标签(含闭合标签、注释) */
const ANY_TAG_RE = new RegExp(`<${TAG_ATTRS}>`, 'g');

function openTagRe(tag: string, flags = 'gi'): RegExp {
  return new RegExp(`<${tag}\\b${TAG_ATTRS}>`, flags);
}

function stripTags(html: string): string {
  return html
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(new RegExp(`<(script|style)\\b${TAG_ATTRS}>[\\s\\S]*?<\\/\\1\\s*>`, 'gi'), ' ')
    .replace(/<br\s*\/?>/gi, ' ')
    .replace(new RegExp(`<\\/?(?:p|div|li|tr|td|th)\\b${TAG_ATTRS}>`, 'gi'), ' ')
    .replace(ANY_TAG_RE, '');
}

function decodeEntities(text: string): string {
  return text
    .replace(/&nbsp;/gi, ' ')
    .replace(/&#(\d+);/g, (_m, d: string) => String.fromCharCode(Number(d)))
    .replace(/&#x([0-9a-fA-F]+);/g, (_m, h: string) => String.fromCharCode(parseInt(h, 16)))
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    .replace(/&#39;/gi, "'")
    .replace(/&amp;/gi, '&');
}

function plainText(html: string | undefined): string {
  if (!html) return '';
  return decodeEntities(stripTags(html)).replace(/\u00a0/g, ' ');
}

/** 原脚本的 cleanText:空值统一为「无」,其余 trim + 折叠空白 */
const cleanText = (text: string | undefined | null): string =>
  text ? text.trim().replace(/\s+/g, ' ') : '无';

/** 取出某个 `<tag ...>...</tag>` 的内层 HTML(不处理嵌套同名标签,站点结构用不到) */
function extractAttr(attrs: string, name: string): string | null {
  const re = new RegExp(`\\b${name}\\s*=\\s*("([^"]*)"|'([^']*)'|([^\\s>]+))`, 'i');
  const m = attrs.match(re);
  if (!m) return null;
  return m[2] ?? m[3] ?? m[4] ?? '';
}

/** 从 pos 起找到配对结束的 </table>(支持嵌套 table) */
function findTableEnd(html: string, start: number): number {
  let depth = 0;
  let i = start;
  const openRe = /<table\b/gi;
  const closeRe = /<\/table\s*>/gi;
  while (i < html.length) {
    openRe.lastIndex = i;
    closeRe.lastIndex = i;
    const open = openRe.exec(html);
    const close = closeRe.exec(html);
    if (!close) return html.length;
    if (open && open.index < close.index) {
      depth += 1;
      i = open.index + open[0].length;
      continue;
    }
    depth -= 1;
    i = close.index + close[0].length;
    if (depth === 0) return close.index;
  }
  return html.length;
}

/** 提取某标签在某段 HTML 内的全部「外层」片段(不处理嵌套同名标签) */
function extractElements(html: string, tag: string): Array<{ attrs: string; inner: string }> {
  const out: Array<{ attrs: string; inner: string }> = [];
  const openRe = new RegExp(`<${tag}\\b(${TAG_ATTRS})>`, 'gi');
  const closeRe = new RegExp(`</${tag}\\s*>`, 'gi');
  let m: RegExpExecArray | null;
  while ((m = openRe.exec(html)) !== null) {
    closeRe.lastIndex = openRe.lastIndex;
    const close = closeRe.exec(html);
    if (!close) break;
    out.push({ attrs: m[1] ?? '', inner: html.slice(openRe.lastIndex, close.index) });
    openRe.lastIndex = close.index + close[0].length;
  }
  return out;
}

/** 取单元格内 `.limit-line` 的文本(原脚本:`td:nth-child(n) .limit-line`) */
function extractLimitLine(cellHtml: string): string | null {
  const re = new RegExp(`<([a-zA-Z][\\w-]*)\\b(${TAG_ATTRS})>`, 'g');
  let m: RegExpExecArray | null;
  while ((m = re.exec(cellHtml)) !== null) {
    const attrs = m[2] ?? '';
    const cls = extractAttr(attrs, 'class');
    if (!cls) continue;
    if (!/(^|\s)limit-line(\s|$)/.test(cls)) continue;
    const tag = m[1] ?? 'div';
    const closeRe = new RegExp(`</${tag}\\s*>`, 'gi');
    closeRe.lastIndex = re.lastIndex;
    const close = closeRe.exec(cellHtml);
    const inner = close ? cellHtml.slice(re.lastIndex, close.index) : cellHtml.slice(re.lastIndex);
    return plainText(inner);
  }
  return null;
}

/**
 * 取行首单元格里隐藏的 `<td-data data-name="..." data-value="...">` 映射。
 * 站点把这些标识(ID / SJItemID / SJItemKaiKeID / ItemName ...)藏在第一个 td 里,
 * 自动选课时需要 SJItemID + SJItemKaiKeID 作为提交参数。
 */
function extractTdData(cellHtml: string): Record<string, string> {
  const out: Record<string, string> = {};
  const re = new RegExp(`<td-data\\b(${TAG_ATTRS})>`, 'gi');
  let m: RegExpExecArray | null;
  while ((m = re.exec(cellHtml)) !== null) {
    const attrs = m[1] ?? '';
    const name = extractAttr(attrs, 'data-name');
    if (!name) continue;
    out[name] = extractAttr(attrs, 'data-value') ?? '';
  }
  return out;
}

// ---------------------------------------------------------------- 表格定位

/** 取出表格内层 HTML;null 表示页面里没有这张表 */
export function extractCourseTableHtml(html: string): string | null {
  const tableRe = new RegExp(`<table\\b(${TAG_ATTRS})>`, 'gi');
  let m: RegExpExecArray | null;
  while ((m = tableRe.exec(html)) !== null) {
    const attrs = m[1] ?? '';
    const id = extractAttr(attrs, 'id');
    if (id !== COURSE_TABLE_ID) continue;
    const innerStart = tableRe.lastIndex;
    const end = findTableEnd(html, innerStart);
    return html.slice(innerStart, end);
  }
  return null;
}

/** 取出数据行(原脚本:`tbody .c--tr`) */
function extractRows(tableHtml: string): string[] {
  const tbodyRe = openTagRe('tbody', 'i');
  const tbodyMatch = tbodyRe.exec(tableHtml);
  const scope = tbodyMatch
    ? tableHtml.slice(tbodyMatch.index + tbodyMatch[0].length, findTbodyEnd(tableHtml, tbodyMatch.index))
    : tableHtml;

  const trs = extractElements(scope, 'tr');
  const rows: string[] = [];
  for (const tr of trs) {
    const cls = extractAttr(tr.attrs, 'class');
    if (!cls) continue;
    // 原脚本用 `tbody .c--tr`;同时兼容 `.c-tr`
    if (!/(^|\s)c--tr(\s|$)/.test(cls) && !/(^|\s)c-tr(\s|$)/.test(cls)) continue;
    rows.push(tr.inner);
  }
  return rows;
}

function findTbodyEnd(html: string, from: number): number {
  const close = /<\/tbody\s*>/i.exec(html.slice(from));
  return close ? from + close.index : html.length;
}

// ---------------------------------------------------------------- 行解析

function getWeekday(dateStr: string): string {
  if (!dateStr) return '';
  const dateMatch = dateStr.match(/\d{4}-\d{2}-\d{2}/);
  if (!dateMatch) return '';
  const date = new Date(dateMatch[0]);
  return isNaN(date.getTime())
    ? ''
    : ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][date.getDay()] ?? '';
}

const isPureNumber = (text: string): boolean => /^\d+$/.test(text.trim());

function buildCourse(
  cells: string[],
  locationFallback: boolean,
  locations: string[],
  categories: string[],
): Course {
  const cellAt = (index1Based: number): string => plainText(cells[index1Based - 1]);

  const col1Text = cleanText(cellAt(1));
  const col2Text = cleanText(cellAt(2));
  const isIndexInCol1 = isPureNumber(col1Text) && !isPureNumber(col2Text);
  const offset = isIndexInCol1 ? 0 : 1;

  const originalTime = cleanText(cellAt(8 + offset));
  const weekday = getWeekday(originalTime);
  const 选课状态 = cleanText(cellAt(10 + offset));
  const 截止状态 = cleanText(cellAt(9 + offset));

  let 开课地点: string;
  if (locationFallback) {
    // 站点结构调整时的自救路径:退回整个单元格文本
    开课地点 = cleanText(cellAt(7 + offset));
  } else {
    const limit = extractLimitLine(cells[7 + offset - 1] ?? '');
    开课地点 = cleanText(limit ?? '');
  }

  const 项目名称 = cleanText(cellAt(3 + offset));
  const 项目类别 = cleanText(cellAt(4 + offset));
  const 实施时间 = weekday ? `${originalTime}（${weekday}）` : originalTime;

  const uniqueId = `${项目名称}|${实施时间}`;
  const isFull = 选课状态.includes('已满');
  const isExpired = 截止状态.includes('已截止');
  const isInvalid = isFull || isExpired;

  const tdData = extractTdData(cells[0] ?? '');

  const locationMatch =
    locations.length === 0 ? true : locations.some((filter) => 开课地点.includes(filter));
  // 类别是黑名单:命中任一即排除(完全匹配)
  const categoryAllowed =
    categories.length === 0 ? true : !categories.some((filter) => 项目类别 === filter);

  return {
    uniqueId,
    序号: isIndexInCol1 ? col1Text : col2Text,
    项目名称,
    项目类别,
    开课地点,
    实施时间,
    选课截止时间: 截止状态,
    选课人数_容纳人数: 选课状态,
    授课教师: cleanText(cellAt(15 + offset)),
    isInvalid,
    isFull,
    isExpired,
    locationMatch,
    categoryAllowed,
    sjItemId: tdData.SJItemID ?? '',
    sjItemKaiKeId: tdData.SJItemKaiKeID ?? '',
  };
}

// ---------------------------------------------------------------- 对外入口

/** 页面是否包含选课表格本体 —— 这是「登录态有效」最强的正向信号 */
export function hasCourseTable(html: string): boolean {
  return html.includes(COURSE_TABLE_ID);
}

/** 页面是否是选课路由(但表格可能由前端 AJAX 渲染,正文里看不到) */
export function looksLikeCourseRoute(html: string): boolean {
  return /SJItemKaiKe\/XuanKe/.test(html);
}

/** 页面是否是登录页 —— 这是「登录态失效」的判定依据 */
export function looksLikeLoginPage(html: string): boolean {
  return LOGIN_MARKERS.some((marker) => html.includes(marker));
}

export interface ParseOptions {
  locations: string[];
  categories: string[];
}

export function parseCourses(
  html: string,
  options: ParseOptions = { locations: [], categories: [] },
): ParseResult {
  const warnings: string[] = [];
  // 过滤词统一去空白并丢弃空串,避免配置里多敲的空格导致误匹配/漏匹配
  const locations = options.locations.map((s) => s.trim()).filter(Boolean);
  const categories = options.categories.map((s) => s.trim()).filter(Boolean);
  const tableHtml = extractCourseTableHtml(html);

  if (tableHtml === null) {
    return { tableFound: false, rowCount: 0, courses: [], locationFallback: false, warnings };
  }

  const rows = extractRows(tableHtml);
  if (rows.length === 0) {
    warnings.push('表格存在但没有数据行(可能是当前无课程,或页面未渲染完成)');
    return { tableFound: true, rowCount: 0, courses: [], locationFallback: false, warnings };
  }

  const cellCache = rows.map((row) => extractElements(row, 'td').map((td) => td.inner));

  // 先按原逻辑解析(开课地点取 .limit-line)
  let locationFallback = false;
  let courses = cellCache.map((cells) => buildCourse(cells, false, locations, categories));

  // 自救:如果一行都没取到 .limit-line,说明站点结构变了,退回整格文本并告警
  const locatedRows = courses.filter((c) => c.开课地点 !== '无').length;
  if (locatedRows === 0) {
    locationFallback = true;
    warnings.push(
      '未在任何行中匹配到 `.limit-line` 结构,开课地点已回退为整格文本 —— 站点结构可能已变化',
    );
    courses = cellCache.map((cells) => buildCourse(cells, true, locations, categories));
  }

  return {
    tableFound: true,
    rowCount: rows.length,
    courses,
    locationFallback,
    warnings,
  };
}

/**
 * 过滤:
 * - 地点为**关键字白名单**(包含任一关键字即命中,空数组=不限制)
 * - 类别为**黑名单**(命中任一即排除,空数组=不限制)
 * - 已满 / 已截止一律排除
 */
export function filterCourses(courses: Course[]): Course[] {
  return courses.filter((c) => c.locationMatch && c.categoryAllowed && !c.isInvalid);
}

/** 计算差集:新课程 = 符合条件但未推送过 */
export function diffNewCourses(valid: Course[], pushedIds: Set<string>): Course[] {
  return valid.filter((c) => !pushedIds.has(c.uniqueId));
}

/**
 * 过期清理(原脚本逻辑 + 补上类别条件):
 * 当前列表中已不存在 / 已失效 / 不再匹配筛选条件的记录予以移除。
 */
export function reconcileIds(allCourses: Course[], pushedIds: Set<string>): string[] {
  const byId = new Map(allCourses.map((c) => [c.uniqueId, c]));
  return Array.from(pushedIds).filter((id) => {
    const course = byId.get(id);
    if (!course) return false;
    if (course.isInvalid) return false;
    if (!course.locationMatch || !course.categoryAllowed) return false;
    return true;
  });
}
