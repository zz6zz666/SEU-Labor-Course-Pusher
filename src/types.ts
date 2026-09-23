/** 全局类型定义 */

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export type AuthState = 'unknown' | 'valid' | 'expired';

/** 单门课程(字段名沿用原脚本,含中文键,便于与原逻辑逐行对照) */
export interface Course {
  /** 去重键:项目名称|实施时间 */
  uniqueId: string;
  序号: string;
  项目名称: string;
  项目类别: string;
  开课地点: string;
  实施时间: string;
  选课截止时间: string;
  选课人数_容纳人数: string;
  授课教师: string;
  /** 已满 或 已截止 */
  isInvalid: boolean;
  isFull: boolean;
  isExpired: boolean;
  locationMatch: boolean;
  /** 类别是否允许 —— 类别配置为黑名单,命中任一即不允许 */
  categoryAllowed: boolean;
  /** 行内隐藏 td-data 的 SJItemID(自动选课用) */
  sjItemId: string;
  /** 行内隐藏 td-data 的 SJItemKaiKeID(自动选课用) */
  sjItemKaiKeId: string;
}

export interface PushplusConfig {
  enabled: boolean;
  token: string;
  title: string;
}

export interface WindowsNotifyConfig {
  enabled: boolean;
  openBrowserOnClick: boolean;
}

export interface ScheduleConfig {
  /** 基础轮询间隔(毫秒) */
  refreshIntervalMs: number;
  /** 抖动比例,0.1 表示 ±10% */
  jitterRatio: number;
  /** 每日汇总推送的整点小时(0-23);null 表示关闭 */
  dailySummaryHour: number | null;
  /** 连续失败多少次后推送「运行异常」告警 */
  failureAlertThreshold: number;
  /** 退避上限(毫秒) */
  maxBackoffMs: number;
}

export interface AppConfig {
  credentials: { username: string; password: string };
  filters: { locations: string[]; categories: string[] };
  schedule: ScheduleConfig;
  push: { pushplus: PushplusConfig; windows: WindowsNotifyConfig };
  behavior: {
    autoLogin: boolean;
    autoOpenOnAuthFailure: boolean;
    autoLaunchAtLogin: boolean;
    /** 自动选课(默认关闭):发现符合筛选条件的课程时自动提交选课 */
    autoSelect: boolean;
  };
  logging: { level: LogLevel; retentionDays: number };
}

/** 事件类型 —— 只影响「点击本机通知后打开什么」,不影响通道选择 */
export type NotifyEventType =
  | 'newCourse'
  | 'authExpired'
  | 'runtimeError'
  | 'dailySummary'
  | 'autoSelect';

export interface NotifyEvent {
  type: NotifyEventType;
  /** 通知标题 */
  title: string;
  /** 微信侧 markdown 正文 */
  markdown: string;
  /** Windows 原生通知正文(纯文本,取 markdown 首段) */
  body: string;
}

/** 抓取结果分类 —— 用服务端响应直接判定,不做心跳推测 */
export type FetchVerdict = 'courses' | 'loginPage' | 'unknown';

export interface FetchResult {
  verdict: FetchVerdict;
  statusCode: number;
  html: string;
  /** 命中登录页 / 重定向时的可读原因,用于日志 */
  reason: string;
}

export interface PersistedState {
  pushedUniqueIds: string[];
  /** 已成功自动选课或已确认不可选的课程 id(避免每轮重复尝试) */
  autoHandledIds: string[];
  authState: AuthState;
  firstRunCompleted: boolean;
  lastRunAt: string | null;
  lastSuccessAt: string | null;
  lastAuthFailureAt: string | null;
  consecutiveFailures: number;
  /** 当日已推送的汇总日期,避免重复推送 (YYYY-MM-DD) */
  lastSummaryDate: string | null;
}
