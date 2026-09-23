/**
 * 监控状态机:轮询 → 判定 → 解析 → 差集 → 推送 → 退避。
 *
 * 与原脚本的心跳机制彻底切割:
 * - 原:靠「页面是否在写心跳」推测会话存活,被浏览器冻结骗过 → 标签页增殖
 * - 新:靠服务端响应判定,失败就走指数退避;没有标签页这个对象,也就无从增殖
 */
import fs from 'fs';
import path from 'path';
import type { Course, FetchVerdict } from '../types';
import { getConfig } from '../config';
import { createLogger } from '../logger';
import { resolvePaths } from '../paths';
import {
  addAutoHandledIds,
  addPushedIds,
  getState,
  reconcileAutoHandledIds,
  reconcilePushedIds,
  setAuthState,
  updateState,
} from '../store';
import { probeAuth, runAutoLogin, type AutoLoginResult } from './auth';
import { diffNewCourses, filterCourses, parseCourses, reconcileIds } from './parse';
import { parseCoursesViaHiddenWindow } from './dom-parse';
import { selectCourses, type SelectOutcome, type SelectTarget } from './select';
import { notify } from '../notify/notifier';
import {
  buildAuthExpiredEvent,
  buildAutoSelectEvent,
  buildAutoSelectFailureEvent,
  buildDailySummaryEvent,
  buildNewCourseEvent,
  buildRuntimeErrorEvent,
  type DailySummaryStats,
} from '../notify/format';

const log = createLogger('watcher');

export interface WatcherStatus {
  /** 最近一次抓取的结论 */
  lastVerdict: FetchVerdict | 'error' | 'idle';
  /** 人类可读的最近状态 */
  lastMessage: string;
  lastRunAt: string | null;
  lastSuccessAt: string | null;
  consecutiveFailures: number;
  nextRunAt: string | null;
  authState: string;
  currentValidCount: number;
  pushedTodayCount: number;
}

export interface WatcherHooks {
  /** 状态变化时回调(用于更新托盘提示、向导状态) */
  onStatusChange?(status: WatcherStatus): void;
  /** 判定登录失效且自动化无法恢复时,请求人工介入(由 index 决定是否弹窗) */
  onNeedManualLogin?(reason: string): void;
}

interface DayStats {
  date: string;
  ticks: number;
  successes: number;
  pushedNew: number;
}

export class Watcher {
  private timer: NodeJS.Timeout | null = null;
  private summaryTimer: NodeJS.Timeout | null = null;
  private stopped = true;
  private ticking = false;

  private failures = 0;
  /** 本轮失效事件是否已告警过(避免每轮都推) */
  private authAlerted = false;
  /** 本轮失效是否已尝试过自动重登 */
  private reloginAttempted = false;
  /** 自动选课失败时,哪些课程已经提醒过(避免每轮刷屏;课程仍会继续重试) */
  private selectFailedNotified = new Set<string>();
  /** 本轮结束后立刻重跑一次(自动重登成功后用) */
  private immediate = false;

  private status: WatcherStatus = {
    lastVerdict: 'idle',
    lastMessage: '等待首次抓取',
    lastRunAt: null,
    lastSuccessAt: null,
    consecutiveFailures: 0,
    nextRunAt: null,
    authState: 'unknown',
    currentValidCount: 0,
    pushedTodayCount: 0,
  };

  private stats: DayStats = { date: todayKey(), ticks: 0, successes: 0, pushedNew: 0 };

  constructor(private hooks: WatcherHooks = {}) {}

  // ---------------------------------------------------------------- 生命周期

  start(): void {
    if (!this.stopped) return;
    this.stopped = false;
    log.info('监控循环启动');
    this.summaryTimer = setInterval(() => void this.maybeSendSummary(), 60 * 1000);
    void this.tick('startup');
  }

  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    if (this.summaryTimer) clearInterval(this.summaryTimer);
    this.timer = null;
    this.summaryTimer = null;
    log.info('监控循环已停止');
  }

  /** 托盘「立即抓取一次」:清空退避,立刻跑一轮 */
  triggerNow(): void {
    log.info('手动触发抓取');
    this.failures = 0;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    void this.tick('manual');
  }

  /** 登录完成后调用:重置状态并立刻抓一次 */
  onLoginRestored(): void {
    this.failures = 0;
    this.authAlerted = false;
    this.reloginAttempted = false;
    setAuthState('valid');
    this.triggerNow();
  }

  getStatus(): WatcherStatus {
    return { ...this.status };
  }

  // ---------------------------------------------------------------- 调度

  private nextDelay(): number {
    const cfg = getConfig().schedule;
    const base = cfg.refreshIntervalMs;

    // 指数退避:连续失败时逐次拉长,避免触发风控
    const backoff = this.failures > 0
      ? Math.min(base * Math.pow(2, this.failures), cfg.maxBackoffMs)
      : base;

    const jitter = 1 + (Math.random() * 2 - 1) * cfg.jitterRatio;
    return Math.max(15000, Math.round(backoff * jitter));
  }

  private scheduleNext(): void {
    if (this.stopped) return;
    const delay = this.nextDelay();
    this.status.nextRunAt = new Date(Date.now() + delay).toISOString();
    this.emitStatus();
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.tick('scheduled'), delay);
    log.debug(
      `下次抓取在 ${Math.round(delay / 1000)} 秒后` +
        (this.failures > 0 ? `(退避中,连续失败 ${this.failures} 次)` : ''),
    );
  }

  private emitStatus(): void {
    this.status.consecutiveFailures = this.failures;
    this.status.authState = getState().authState;
    this.hooks.onStatusChange?.({ ...this.status });
  }

  // ---------------------------------------------------------------- 主循环

  private async tick(reason: 'scheduled' | 'manual' | 'startup'): Promise<void> {
    if (this.stopped || this.ticking) return;
    this.ticking = true;
    this.rolloverStats();

    const startedAt = new Date().toISOString();
    updateState({ lastRunAt: startedAt });
    this.status.lastRunAt = startedAt;
    this.status.lastVerdict = 'idle';
    this.status.lastMessage = '正在抓取…';
    this.emitStatus();

    try {
      const probe = await probeAuth();

      if (probe.state === 'valid') {
        await this.handleValid(probe.html);
      } else if (probe.state === 'expired') {
        await this.handleAuthExpired(probe.reason);
      } else {
        await this.handleUnknown(probe.reason, probe.html);
      }
    } catch (err) {
      await this.handleUnknown(`未捕获异常:${(err as Error).message}`, '');
    } finally {
      this.stats.ticks += 1;
      this.ticking = false;
      if (this.immediate) {
        this.immediate = false;
        log.info('自动重登成功,1 秒后立即重抓');
        if (this.timer) clearTimeout(this.timer);
        this.timer = setTimeout(() => void this.tick('manual'), 1000);
        this.status.nextRunAt = new Date(Date.now() + 1000).toISOString();
        this.emitStatus();
      } else {
        this.scheduleNext();
      }
      void reason;
    }
  }

  // ---------------------------------------------------------------- 分支处理

  private async handleValid(html: string): Promise<void> {
    const cfg = getConfig();
    const parseOptions = { locations: cfg.filters.locations, categories: cfg.filters.categories };

    let parsed = parseCourses(html, parseOptions);

    // 表格不在纯 HTML 里 → 大概率是 AJAX 渲染,退化为隐藏窗口 DOM 提取
    if (!parsed.tableFound) {
      log.warn('纯 HTTP 正文中未找到课程表格,尝试隐藏窗口 DOM 提取');
      try {
        parsed = await parseCoursesViaHiddenWindow({
          locations: cfg.filters.locations,
          categories: cfg.filters.categories,
        });
      } catch (err) {
        log.error('隐藏窗口提取失败:', err);
      }
    }

    parsed.warnings.forEach((w) => log.warn(w));

    if (!parsed.tableFound) {
      this.saveSnapshot(html, 'no-table');
      this.status.lastVerdict = 'error';
      await this.fail('页面可达但解析不到课程表格', { stage: 'table-missing' });
      return;
    }

    if (parsed.rowCount === 0) {
      // 与原脚本一致:无数据行时直接结束,不做差集,避免误清空历史记录
      log.info('表格为空(当前无课程或未渲染完成)');
      this.failures = 0;
      this.authAlerted = false;
      this.reloginAttempted = false;
      setAuthState('valid');
      const now = new Date().toISOString();
      updateState({ lastSuccessAt: now, consecutiveFailures: 0 });
      this.status.lastVerdict = 'courses';
      this.status.lastSuccessAt = now;
      this.status.lastMessage = '抓取成功,当前列表为空';
      this.status.currentValidCount = 0;
      this.stats.successes += 1;
      this.emitStatus();
      return;
    }

    const validCourses = filterCourses(parsed.courses);
    const allCourses: Course[] = parsed.courses;

    // ---- 差集(顺序与原脚本一致:先清理过期,再加入新增)----
    const pushedSet = new Set(getState().pushedUniqueIds);
    const reconciled = reconcileIds(allCourses, pushedSet);
    reconcilePushedIds(reconciled);
    const effectiveSet = new Set(reconciled);

    const newCourses = diffNewCourses(validCourses, effectiveSet);

    if (newCourses.length > 0) {
      log.info(`发现 ${newCourses.length} 门新课程,开始双通道推送`);
      await notify(buildNewCourseEvent(newCourses));
      addPushedIds(newCourses.map((c) => c.uniqueId));
      this.stats.pushedNew += newCourses.length;
    } else {
      log.info(`无新增课程(当前符合条件 ${validCourses.length} 门)`);
    }

    const now = new Date().toISOString();
    this.failures = 0;
    this.authAlerted = false;
    this.reloginAttempted = false;
    setAuthState('valid');
    updateState({ lastSuccessAt: now, consecutiveFailures: 0 });

    // 自动选课(默认关闭):对符合筛选条件且尚未处理的课程尝试选课
    let selectNote = '';
    if (cfg.behavior.autoSelect && validCourses.length > 0) {
      selectNote = await this.runAutoSelect(validCourses, allCourses);
    }

    this.stats.successes += 1;
    this.status.lastVerdict = 'courses';
    this.status.lastSuccessAt = now;
    this.status.currentValidCount = validCourses.length;
    this.status.pushedTodayCount = this.stats.pushedNew;
    this.status.lastMessage =
      `抓取成功 · 共 ${parsed.rowCount} 条,符合条件 ${validCourses.length} 门` +
      (newCourses.length > 0 ? `,新推送 ${newCourses.length} 门` : '') +
      selectNote;
    this.emitStatus();
  }

  /**
   * 自动选课:对「符合筛选条件且未处理过」的课程逐个尝试。
   * 成功/已确认不可选的记入 autoHandledIds,不再重复尝试;
   * 提交失败的每轮重试,并仅在首次失败时提醒一次(避免刷屏)。
   */
  private async runAutoSelect(courses: Course[], allCourses: Course[]): Promise<string> {
    // 清理已从列表消失的记录
    const allIds = new Set(allCourses.map((c) => c.uniqueId));
    reconcileAutoHandledIds(getState().autoHandledIds.filter((id) => allIds.has(id)));

    const handled = new Set(getState().autoHandledIds);
    const targets: SelectTarget[] = courses
      .filter((c) => c.sjItemId && c.sjItemKaiKeId && !handled.has(c.uniqueId))
      .map((c) => ({
        uniqueId: c.uniqueId,
        itemName: c.项目名称,
        sjItemId: c.sjItemId,
        sjItemKaiKeId: c.sjItemKaiKeId,
      }));

    if (targets.length === 0) return '';

    this.status.lastMessage = `自动选课:正在尝试 ${targets.length} 门…`;
    this.emitStatus();

    let outcomes: SelectOutcome[];
    try {
      outcomes = await selectCourses(targets);
    } catch (err) {
      log.error('自动选课异常:', err);
      return ' · 自动选课异常';
    }

    const byId = new Map(courses.map((c) => [c.uniqueId, c]));
    const pick = (list: SelectOutcome[]): Course[] =>
      list.map((o) => byId.get(o.target.uniqueId)).filter((c): c is Course => Boolean(c));

    const selected = outcomes.filter((o) => o.status === 'selected');
    const already = outcomes.filter((o) => o.status === 'already');
    const failed = outcomes.filter((o) => o.status === 'failed' || o.status === 'notFound');

    if (selected.length > 0) {
      addAutoHandledIds(selected.map((o) => o.target.uniqueId));
      selected.forEach((o) => this.selectFailedNotified.delete(o.target.uniqueId));
      log.info(`自动选课成功 ${selected.length} 门,已通知`);
      await notify(buildAutoSelectEvent(pick(selected)));
    }
    if (already.length > 0) {
      // 已选/已满:视为已处理,不再重复尝试
      addAutoHandledIds(already.map((o) => o.target.uniqueId));
    }

    const freshFailures = failed.filter((o) => !this.selectFailedNotified.has(o.target.uniqueId));
    if (freshFailures.length > 0) {
      freshFailures.forEach((o) => this.selectFailedNotified.add(o.target.uniqueId));
      log.warn(`自动选课失败 ${freshFailures.length} 门(将重试):`, freshFailures.map((o) => o.message));
      await notify(buildAutoSelectFailureEvent(pick(freshFailures), freshFailures.map((o) => o.message)));
    }

    return ` · 自动选课 成功${selected.length}/已选${already.length}/失败${failed.length}`;
  }

  private async handleAuthExpired(reason: string): Promise<void> {
    log.warn('判定登录失效:', reason);
    setAuthState('expired');
    this.failures += 1;

    // 会话过期但凭据仍有效时,先尝试一次自动重登(整轮失效只试一次,不反复重试)
    const cfg = getConfig();
    const canRelogin =
      cfg.behavior.autoLogin &&
      Boolean(cfg.credentials.username && cfg.credentials.password) &&
      !this.reloginAttempted &&
      !this.authAlerted;

    let autoLoginNote = '';
    if (canRelogin) {
      this.reloginAttempted = true;
      log.info('尝试自动重新登录(仅一次)');
      this.status.lastMessage = '登录失效,正在尝试自动重登…';
      this.emitStatus();

      const result = await runAutoLogin({ formTimeoutMs: 12000, navTimeoutMs: 18000 });
      log.info(`自动重登结果:${result.outcome} - ${result.detail}`);

      if (result.outcome === 'success') {
        this.status.lastMessage = '自动重登成功,继续监控';
        this.emitStatus();
        this.failures = 0;
        this.authAlerted = false;
        this.reloginAttempted = false;
        this.immediate = true;
        return;
      }
      // 把失败原因带进告警,否则用户只看到「被重定向到登录页」,
      // 无法判断是密码错了、需要验证码、还是站点故障
      autoLoginNote = `;${describeAutoLogin(result)}`;
    }

    if (!this.authAlerted) {
      this.authAlerted = true;
      await notify(buildAuthExpiredEvent(`${reason}${autoLoginNote}`));
      this.hooks.onNeedManualLogin?.(reason);
    }

    this.status.lastVerdict = 'loginPage';
    this.status.lastMessage = `登录失效:${reason}`;
    // authState 已在上面 setAuthState 里落盘(此处不再重复写 final 分支的 valid)
    this.emitStatus();
  }

  private async handleUnknown(reason: string, html: string): Promise<void> {
    log.warn('抓取未能判定:', reason);
    if (html) this.saveSnapshot(html, 'unknown');
    this.status.lastVerdict = 'error';
    await this.fail(`抓取失败:${reason}`, { stage: 'fetch' });
  }

  /** 统一的失败处理:计数 + 达阈值告警 */
  private async fail(detail: string, extra?: Record<string, unknown>): Promise<void> {
    this.failures += 1;
    updateState({ consecutiveFailures: this.failures });

    const cfg = getConfig();
    if (this.failures === cfg.schedule.failureAlertThreshold) {
      await notify(buildRuntimeErrorEvent(detail, extra));
    } else if (this.failures > cfg.schedule.failureAlertThreshold && this.failures % 20 === 0) {
      // 长时间未恢复,低频提醒一次,避免刷屏
      await notify(buildRuntimeErrorEvent(`${detail}(已连续失败 ${this.failures} 次)`, extra));
    }

    this.status.lastMessage = detail;
    this.status.lastVerdict = 'error';
    this.emitStatus();
  }

  // ---------------------------------------------------------------- 每日汇总

  private async maybeSendSummary(): Promise<void> {
    const cfg = getConfig();
    const hour = cfg.schedule.dailySummaryHour;
    if (hour === null) return;

    const today = todayKey();
    if (getState().lastSummaryDate === today) return;
    if (new Date().getHours() < hour) return;

    const stats: DailySummaryStats = {
      date: today,
      ticks: this.stats.date === today ? this.stats.ticks : 0,
      successes: this.stats.date === today ? this.stats.successes : 0,
      pushedNew: this.stats.date === today ? this.stats.pushedNew : 0,
      currentValidCount: this.status.currentValidCount,
      authState: getState().authState,
      trackedCount: getState().pushedUniqueIds.length,
    };

    log.info('推送每日运行汇总');
    await notify(buildDailySummaryEvent(stats));
    updateState({ lastSummaryDate: today });
  }

  private rolloverStats(): void {
    const today = todayKey();
    if (this.stats.date !== today) {
      this.stats = { date: today, ticks: 0, successes: 0, pushedNew: 0 };
    }
  }

  // ---------------------------------------------------------------- 诊断留痕

  /** 解析失败时留存原始 HTML 快照,便于事后排查(PLAN.md 13) */
  private saveSnapshot(html: string, tag: string): void {
    try {
      const dir = resolvePaths().snapshotDir;
      fs.mkdirSync(dir, { recursive: true });
      const stamp = new Date().toISOString().replace(/[:.]/g, '-');
      fs.writeFileSync(path.join(dir, `${tag}-${stamp}.html`), html, 'utf8');
      // 只保留最近 5 份
      const files = fs
        .readdirSync(dir)
        .filter((f) => f.endsWith('.html'))
        .sort();
      while (files.length > 5) {
        const victim = files.shift();
        if (victim) fs.unlinkSync(path.join(dir, victim));
      }
    } catch (err) {
      log.debug('保存快照失败:', err);
    }
  }
}

/** 把自动重登结果翻译成告警里可读的一句话 */
function describeAutoLogin(result: AutoLoginResult): string {
  switch (result.outcome) {
    case 'needManual':
      return `自动重登需人工验证(${result.detail})`;
    case 'timeout':
      return '自动重登未在限定时间内完成(可能是账号密码有误,或站点要求验证码)';
    case 'noCredentials':
      return '未配置账号密码';
    default:
      return `自动重登失败(${result.detail})`;
  }
}

function todayKey(): string {
  const d = new Date();
  const p = (n: number) => (n < 10 ? `0${n}` : String(n));
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}
