/**
 * 通知内容格式化。
 * `formatCoursesMarkdown` 移植自原脚本的 formatToMarkdown,保持推送排版一致。
 */
import type { Course, NotifyEvent } from '../types';

export interface DailySummaryStats {
  date: string;
  /** 当日抓取次数 */
  ticks: number;
  /** 当日成功抓取次数 */
  successes: number;
  /** 当日发现并推送的新课程数 */
  pushedNew: number;
  /** 当前符合推送条件的课程总数 */
  currentValidCount: number;
  /** 当前登录态 */
  authState: string;
  /** 已累计推送过的课程 id 数 */
  trackedCount: number;
}

const now = (): string => new Date().toLocaleString('zh-CN', { hour12: false });

/** 原脚本 formatToMarkdown 的原样移植 */
export function formatCoursesMarkdown(courses: Course[], title?: string): string {
  let md = '| 序号 | 项目名称 | 项目类别 | 实施时间 | 开课地点 | 选课情况 | 教师 |\n';
  md += '|------|----------|----------|----------|----------|----------|------|\n';
  courses.forEach((course) => {
    md += `| ${course.序号} | ${course.项目名称} | ${course.项目类别} | ${course.实施时间} | ${course.开课地点} | ${course.选课人数_容纳人数} | ${course.授课教师} |\n`;
  });
  if (title) md = `## ${title}\n\n${md}`;
  return md + `\n提取时间：${now()}`;
}

/** 把 markdown 压成适合 Windows 通知的纯文本(通知正文会被系统截断,只留最关键的) */
export function toToastBody(markdown: string, maxLen = 200): string {
  const text = markdown
    .replace(/^\s*\|.*\|\s*$/gm, (line) => line.replace(/\|/g, ' ').replace(/\s+/g, ' ').trim())
    .replace(/^#{1,6}\s*/gm, '')
    .replace(/^[-| ]+$/gm, '')
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .join('\n');
  return text.length > maxLen ? `${text.slice(0, maxLen)}…` : text;
}

/** 事件:发现新课程 */
export function buildNewCourseEvent(courses: Course[]): NotifyEvent {
  const markdown = formatCoursesMarkdown(courses, `发现 ${courses.length} 门新课程`);
  const preview = courses
    .slice(0, 3)
    .map((c) => `· ${c.项目名称}｜${c.实施时间}｜${c.开课地点}`)
    .join('\n');
  const more = courses.length > 3 ? `\n…另有 ${courses.length - 3} 门` : '';
  return {
    type: 'newCourse',
    title: `劳动教育新课程 · ${courses.length} 门`,
    markdown,
    body: `${preview}${more}\n\n点击在内置浏览器中打开选课页`,
  };
}

/** 事件:自动选课成功 */
export function buildAutoSelectEvent(courses: Course[]): NotifyEvent {
  const markdown = formatCoursesMarkdown(courses, `已自动选课 ${courses.length} 门`);
  const preview = courses
    .slice(0, 3)
    .map((c) => `· ${c.项目名称}｜${c.实施时间}｜${c.开课地点}`)
    .join('\n');
  const more = courses.length > 3 ? `\n…另有 ${courses.length - 3} 门` : '';
  return {
    type: 'autoSelect',
    title: `已自动选课 · ${courses.length} 门`,
    markdown,
    body: `${preview}${more}\n\n自动选课已提交。点击在内置浏览器中打开选课页核对。`,
  };
}

/** 事件:自动选课失败(首次即提醒,后续仍会继续重试) */
export function buildAutoSelectFailureEvent(courses: Course[], reasons: string[]): NotifyEvent {
  const markdown = formatCoursesMarkdown(courses, `自动选课失败 ${courses.length} 门`);
  const detail = courses
    .slice(0, 3)
    .map((c, i) => `· ${c.项目名称}｜${c.实施时间}：${reasons[i] ?? '未知原因'}`)
    .join('\n');
  return {
    type: 'autoSelect',
    title: `自动选课失败 · ${courses.length} 门`,
    markdown:
      `${markdown}\n\n**失败原因**：\n${detail}\n\n` +
      `程序会在后续每轮继续重试,直到成功或课程失效。`,
    body: `${detail}\n\n程序会继续重试。点击在内置浏览器中打开选课页`,
  };
}

/** 事件:登录失效 */
export function buildAuthExpiredEvent(reason: string): NotifyEvent {
  const time = now();
  return {
    type: 'authExpired',
    title: '登录失效 · 需要重新登录',
    markdown:
      `## 统一身份认证登录已失效\n\n` +
      `**原因**：${reason}\n\n` +
      `**时间**：${time}\n\n` +
      `后台已停止取数,需要重新登录后才会恢复监控。\n\n` +
      `点击本机通知将打开**内置浏览器**的登录页(可能需短信验证);` +
      `也可以右键托盘图标 → 「用内置浏览器打开登录页」。\n\n` +
      `> 登录完成后可直接在内置浏览器中选课。`,
    body: `原因:${reason}\n点击打开内置浏览器登录页`,
  };
}

/** 事件:运行异常 */
export function buildRuntimeErrorEvent(detail: string, extra?: Record<string, unknown>): NotifyEvent {
  const extraText = extra ? `\n\n\`\`\`\n${JSON.stringify(extra, null, 2)}\n\`\`\`` : '';
  return {
    type: 'runtimeError',
    title: '监控运行异常',
    markdown:
      `## 监控运行异常\n\n` +
      `**详情**：${detail}\n\n` +
      `**时间**：${now()}\n\n` +
      `已连续失败若干次,程序会自动退避重试。点击本机通知可打开日志目录查看原因。${extraText}`,
    body: `${detail}\n点击打开日志目录`,
  };
}

/** 事件:每日运行汇总 */
export function buildDailySummaryEvent(stats: DailySummaryStats): NotifyEvent {
  const authText =
    stats.authState === 'valid' ? '正常' : stats.authState === 'expired' ? '已失效' : '未知';
  return {
    type: 'dailySummary',
    title: `运行汇总 · ${stats.date}`,
    markdown:
      `## 每日运行汇总(${stats.date})\n\n` +
      `| 指标 | 数值 |\n|---|---|\n` +
      `| 抓取次数 | ${stats.ticks} |\n` +
      `| 成功次数 | ${stats.successes} |\n` +
      `| 新课程推送 | ${stats.pushedNew} |\n` +
      `| 当前可选课程 | ${stats.currentValidCount} |\n` +
      `| 登录态 | ${authText} |\n` +
      `| 已记录课程 | ${stats.trackedCount} |\n\n` +
      `统计时间：${now()}`,
    body:
      `抓取 ${stats.ticks} 次 / 成功 ${stats.successes} 次\n` +
      `新课程推送 ${stats.pushedNew} 门,当前可选 ${stats.currentValidCount} 门\n` +
      `登录态:${authText}`,
  };
}
