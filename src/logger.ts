/**
 * 文件日志(按天分文件 + 保留期轮转)。
 *
 * 注意:任何调用方都不得把凭据写入日志。所有输出都会经过 `redact()` 兜底过滤。
 */
import fs from 'fs';
import path from 'path';
import type { LogLevel } from './types';
import { resolvePaths } from './paths';

const LEVEL_WEIGHT: Record<LogLevel, number> = { debug: 10, info: 20, warn: 30, error: 40 };

let logDir = '';
let minLevel: LogLevel = 'info';
let retentionDays = 7;
let ready = false;
let currentFileDate = '';
let currentFilePath = '';
let cleanupDoneDate = '';

/** 兜底脱敏:阻止 password / token 之类的值被写进日志 */
function redact(text: string): string {
  return text
    .replace(/("?(?:password|pwd|token)"?\s*[:=]\s*")[^"]*(")/gi, '$1***$2')
    .replace(/(Password|Token|__RequestVerificationToken)([=:]\s*)[^\s&,"}]+/gi, '$1$2***');
}

export function initLogger(level: LogLevel, days: number): void {
  const paths = resolvePaths();
  logDir = paths.logDir;
  minLevel = level;
  retentionDays = days;
  ready = true;
  rotateIfNeeded();
  cleanupOldFiles();
}

/** 配置热重载时同步日志参数 */
export function reconfigureLogger(level: LogLevel, days: number): void {
  minLevel = level;
  retentionDays = days;
  cleanupOldFiles();
}

export function getLogDir(): string {
  return logDir || resolvePaths().logDir;
}

export function getLogFilePath(): string {
  rotateIfNeeded();
  return currentFilePath;
}

function todayKey(): string {
  const d = new Date();
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function pad(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

function rotateIfNeeded(): void {
  const key = todayKey();
  if (key !== currentFileDate || !currentFilePath) {
    currentFileDate = key;
    currentFilePath = path.join(logDir, `daemon-${key}.log`);
    if (!cleanupDoneDate) cleanupDoneDate = '';
    if (cleanupDoneDate !== key) {
      cleanupDoneDate = key;
      cleanupOldFiles();
    }
  }
}

function cleanupOldFiles(): void {
  if (!logDir) return;
  const cutoff = Date.now() - retentionDays * 24 * 60 * 60 * 1000;
  let entries: string[] = [];
  try {
    entries = fs.readdirSync(logDir);
  } catch {
    return;
  }
  for (const name of entries) {
    if (!/^daemon-\d{4}-\d{2}-\d{2}\.log$/.test(name)) continue;
    const full = path.join(logDir, name);
    try {
      if (fs.statSync(full).mtimeMs < cutoff) fs.unlinkSync(full);
    } catch {
      /* 单个文件删除失败不影响整体 */
    }
  }
}

function write(level: LogLevel, tag: string, parts: unknown[]): void {
  if (!ready) {
    return;
  }
  if (LEVEL_WEIGHT[level] < LEVEL_WEIGHT[minLevel]) return;

  const stamp = new Date().toISOString();
  const message = parts
    .map((p) => {
      if (p instanceof Error) return `${p.name}: ${p.message}${p.stack ? `\n${p.stack}` : ''}`;
      if (typeof p === 'string') return p;
      try {
        return JSON.stringify(p);
      } catch {
        return String(p);
      }
    })
    .join(' ');

  const line = `[${stamp}] [${level.toUpperCase()}] [${tag}] ${redact(message)}\n`;

  // 控制台:便于开发期直接观察
  const consoleFn = level === 'error' ? console.error : level === 'warn' ? console.warn : console.log;
  consoleFn(line.trimEnd());

  try {
    rotateIfNeeded();
    fs.appendFileSync(currentFilePath, line, 'utf8');
  } catch {
    /* 日志写失败不应让主流程崩溃 */
  }
}

export interface Logger {
  debug(...parts: unknown[]): void;
  info(...parts: unknown[]): void;
  warn(...parts: unknown[]): void;
  error(...parts: unknown[]): void;
  child(tag: string): Logger;
}

export function createLogger(tag: string): Logger {
  return {
    debug: (...p) => write('debug', tag, p),
    info: (...p) => write('info', tag, p),
    warn: (...p) => write('warn', tag, p),
    error: (...p) => write('error', tag, p),
    child: (sub: string) => createLogger(`${tag}:${sub}`),
  };
}
