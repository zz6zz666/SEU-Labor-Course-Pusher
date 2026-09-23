/**
 * 配置加载、校验与热重载。
 *
 * 原则(PLAN.md 第 6 节):配置与代码彻底分离;凭据只存在于本地 config.json,
 * 不写入日志、不随通知外发。
 */
import fs from 'fs';
import path from 'path';
import { EventEmitter } from 'events';
import type { AppConfig, LogLevel } from './types';
import { resolvePaths } from './paths';

export const DEFAULT_CONFIG: AppConfig = {
  credentials: { username: '', password: '' },
  filters: { locations: [], categories: [] },
  schedule: {
    refreshIntervalMs: 180000,
    jitterRatio: 0.1,
    dailySummaryHour: 21,
    failureAlertThreshold: 3,
    maxBackoffMs: 30 * 60 * 1000,
  },
  push: {
    pushplus: { enabled: true, token: '', title: '劳动教育课程推送' },
    windows: { enabled: true, openBrowserOnClick: true },
  },
  behavior: {
    autoLogin: true,
    autoOpenOnAuthFailure: true,
    autoLaunchAtLogin: false,
    autoSelect: false,
  },
  logging: { level: 'info', retentionDays: 7 },
};

const VALID_LEVELS: LogLevel[] = ['debug', 'info', 'warn', 'error'];

class ConfigStore extends EventEmitter {
  private config: AppConfig = clone(DEFAULT_CONFIG);
  private watched = false;

  get(): AppConfig {
    return this.config;
  }

  /** 用外部读到的配置替换内存态并广播(updateConfig 写回后调用) */
  replace(next: AppConfig, warnings: string[] = []): void {
    this.config = next;
    this.emit('change', next, warnings);
  }

  /** 首次加载:不存在则从模板复制一份;模板也没有则写入默认值 */
  load(): { config: AppConfig; warnings: string[] } {
    const { configPath, projectRoot } = resolvePaths();
    const warnings: string[] = [];

    if (!fs.existsSync(configPath)) {
      const example = path.join(projectRoot, 'config.example.json');
      try {
        if (fs.existsSync(example)) {
          // 用「读+写」而非 copyFile:打包后模板位于 asar 内部,读出来再写更稳妥
          const template = fs.readFileSync(example, 'utf8');
          fs.writeFileSync(configPath, template, 'utf8');
          warnings.push('未找到 config.json,已从 config.example.json 复制模板。');
        } else {
          fs.writeFileSync(configPath, JSON.stringify(DEFAULT_CONFIG, null, 2), 'utf8');
          warnings.push('未找到 config.json,已写入默认配置。');
        }
      } catch (err) {
        warnings.push(`无法创建 config.json:${(err as Error).message}`);
      }
    }

    const { config, warnings: w } = readAndValidate(configPath);
    warnings.push(...w);
    this.config = config;
    return { config, warnings };
  }

  /** 监听文件变化并热重载(轮询实现,兼容编辑器的「原子替换」保存方式) */
  watch(): void {
    if (this.watched) return;
    this.watched = true;
    const { configPath } = resolvePaths();

    let lastMtime = 0;
    try {
      lastMtime = fs.statSync(configPath).mtimeMs;
    } catch {
      /* 忽略 */
    }

    fs.watchFile(configPath, { interval: 700 }, (curr) => {
      if (!curr.mtimeMs || curr.mtimeMs === lastMtime) return;
      lastMtime = curr.mtimeMs;
      const { config, warnings } = readAndValidate(configPath);
      this.config = config;
      this.emit('change', config, warnings);
    });
  }

  stopWatch(): void {
    if (!this.watched) return;
    this.watched = false;
    fs.unwatchFile(resolvePaths().configPath);
  }
}

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** 深合并:以默认值为骨架,用用户配置覆盖 */
function deepMerge<T>(base: T, patch: unknown): T {
  if (!isPlainObject(patch)) return base;
  const out: Record<string, unknown> = isPlainObject(base) ? { ...(base as object) } : {};
  for (const [key, value] of Object.entries(patch)) {
    const baseVal = (base as Record<string, unknown> | undefined)?.[key];
    if (isPlainObject(value) && isPlainObject(baseVal)) {
      out[key] = deepMerge(baseVal, value);
    } else if (value !== undefined) {
      out[key] = value;
    }
  }
  return out as T;
}

function readAndValidate(configPath: string): { config: AppConfig; warnings: string[] } {
  const warnings: string[] = [];
  let raw: string;
  try {
    raw = fs.readFileSync(configPath, 'utf8');
  } catch (err) {
    warnings.push(`读取 config.json 失败:${(err as Error).message},已回退默认配置`);
    return { config: clone(DEFAULT_CONFIG), warnings };
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw.replace(/^\uFEFF/, ''));
  } catch (err) {
    warnings.push(`config.json 不是合法 JSON:${(err as Error).message},已回退默认配置`);
    return { config: clone(DEFAULT_CONFIG), warnings };
  }

  const config = deepMerge(clone(DEFAULT_CONFIG), parsed);

  // ---- 校验与纠正 ----
  if (!VALID_LEVELS.includes(config.logging.level)) {
    warnings.push(`logging.level 非法(${String(config.logging.level)}),已重置为 info`);
    config.logging.level = 'info';
  }
  if (typeof config.logging.retentionDays !== 'number' || config.logging.retentionDays < 1) {
    warnings.push('logging.retentionDays 非法,已重置为 7');
    config.logging.retentionDays = 7;
  }
  if (typeof config.schedule.refreshIntervalMs !== 'number' || config.schedule.refreshIntervalMs < 30000) {
    warnings.push('schedule.refreshIntervalMs 不得小于 30000,已重置为 180000');
    config.schedule.refreshIntervalMs = 180000;
  }
  if (typeof config.schedule.jitterRatio !== 'number' || config.schedule.jitterRatio < 0 || config.schedule.jitterRatio > 0.5) {
    warnings.push('schedule.jitterRatio 应在 0~0.5 之间,已重置为 0.1');
    config.schedule.jitterRatio = 0.1;
  }
  if (config.schedule.dailySummaryHour !== null &&
      (typeof config.schedule.dailySummaryHour !== 'number' ||
       config.schedule.dailySummaryHour < 0 ||
       config.schedule.dailySummaryHour > 23)) {
    warnings.push('schedule.dailySummaryHour 应为 0~23 或 null,已重置为 21');
    config.schedule.dailySummaryHour = 21;
  }
  if (!Array.isArray(config.filters.locations)) {
    warnings.push('filters.locations 应为数组,已重置为空');
    config.filters.locations = [];
  }
  if (!Array.isArray(config.filters.categories)) {
    warnings.push('filters.categories 应为数组,已重置为空');
    config.filters.categories = [];
  }
  if (typeof config.credentials.username !== 'string') config.credentials.username = '';
  if (typeof config.credentials.password !== 'string') config.credentials.password = '';

  return { config, warnings };
}

export const configStore = new ConfigStore();
export const getConfig = (): AppConfig => configStore.get();

/**
 * 就地修改并写回 config.json。
 *
 * 存在的理由:有些设置既存在于配置也存在于系统(开机自启就是典型)——
 * 用户在托盘里改了开关,如果不写回配置,下次启动 `syncAutoStartFromConfig()`
 * 会按配置里的旧值把系统设置改回去,用户的开关等于白按。
 *
 * 写文件会触发自身的文件监听,但那只会做一次无害的同值热重载。
 */
export function updateConfig(mutate: (cfg: AppConfig) => void): { ok: boolean; error?: string } {
  const { configPath } = resolvePaths();
  const next = clone(configStore.get());
  mutate(next);
  try {
    fs.writeFileSync(configPath, JSON.stringify(next, null, 2), 'utf8');
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
  // 同步内存态,避免等文件监听那一拍
  const { config, warnings } = readAndValidate(configPath);
  configStore.replace(config, warnings);
  return { ok: true };
}

/** 凭据是否已填写(仅用于判断,不输出内容) */
export function hasCredentials(cfg: AppConfig = getConfig()): boolean {
  return Boolean(cfg.credentials.username && cfg.credentials.password);
}

/** 脱敏摘要,可安全写日志 */
export function safeSummary(cfg: AppConfig): Record<string, unknown> {
  return {
    usernameConfigured: Boolean(cfg.credentials.username),
    passwordConfigured: Boolean(cfg.credentials.password),
    locations: cfg.filters.locations,
    categories: cfg.filters.categories,
    refreshIntervalMs: cfg.schedule.refreshIntervalMs,
    jitterRatio: cfg.schedule.jitterRatio,
    dailySummaryHour: cfg.schedule.dailySummaryHour,
    pushplusEnabled: cfg.push.pushplus.enabled,
    pushplusTokenConfigured: Boolean(cfg.push.pushplus.token),
    windowsNotifyEnabled: cfg.push.windows.enabled,
    autoLogin: cfg.behavior.autoLogin,
    autoOpenOnAuthFailure: cfg.behavior.autoOpenOnAuthFailure,
    autoLaunchAtLogin: cfg.behavior.autoLaunchAtLogin,
    autoSelect: cfg.behavior.autoSelect,
    logLevel: cfg.logging.level,
  };
}
