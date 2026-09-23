/**
 * 状态持久化(原子写:先写临时文件,再 rename 覆盖)。
 *
 * 对应原脚本的 GM_setValue('pushedCourseUniqueIds')。
 */
import fs from 'fs';
import path from 'path';
import type { AuthState, PersistedState } from './types';
import { resolvePaths } from './paths';

const DEFAULT_STATE: PersistedState = {
  pushedUniqueIds: [],
  autoHandledIds: [],
  authState: 'unknown',
  firstRunCompleted: false,
  lastRunAt: null,
  lastSuccessAt: null,
  lastAuthFailureAt: null,
  consecutiveFailures: 0,
  lastSummaryDate: null,
};

let state: PersistedState = { ...DEFAULT_STATE };
let statePath = '';
let writeTimer: NodeJS.Timeout | null = null;

export function initStore(): PersistedState {
  statePath = resolvePaths().statePath;
  try {
    if (fs.existsSync(statePath)) {
      const raw = fs.readFileSync(statePath, 'utf8').replace(/^\uFEFF/, '');
      const parsed = JSON.parse(raw) as Partial<PersistedState>;
      state = {
        ...DEFAULT_STATE,
        ...parsed,
        pushedUniqueIds: Array.isArray(parsed.pushedUniqueIds) ? parsed.pushedUniqueIds.filter((x) => typeof x === 'string') : [],
        autoHandledIds: Array.isArray(parsed.autoHandledIds) ? parsed.autoHandledIds.filter((x) => typeof x === 'string') : [],
      };
    } else {
      state = { ...DEFAULT_STATE };
      persistNow();
    }
  } catch {
    // 状态文件损坏:备份后重建,避免程序起不来
    try {
      fs.copyFileSync(statePath, `${statePath}.corrupt`);
    } catch {
      /* 忽略 */
    }
    state = { ...DEFAULT_STATE };
    persistNow();
  }
  return state;
}

export function getState(): PersistedState {
  return state;
}

function persistNow(): void {
  if (!statePath) return;
  const tmp = `${statePath}.tmp`;
  try {
    fs.writeFileSync(tmp, JSON.stringify(state, null, 2), 'utf8');
    fs.renameSync(tmp, statePath);
  } catch {
    /* 落盘失败不影响内存状态 */
  }
}

/** 合并更新并落盘(合并写,避免高频 IO) */
export function updateState(patch: Partial<PersistedState>): PersistedState {
  state = { ...state, ...patch };
  if (writeTimer) clearTimeout(writeTimer);
  writeTimer = setTimeout(persistNow, 120);
  return state;
}

/** 立即落盘(退出前调用) */
export function flushState(): void {
  if (writeTimer) {
    clearTimeout(writeTimer);
    writeTimer = null;
  }
  persistNow();
}

export function setAuthState(authState: AuthState): void {
  const patch: Partial<PersistedState> = { authState };
  if (authState === 'expired') patch.lastAuthFailureAt = new Date().toISOString();
  updateState(patch);
}

/** 新增已推送课程 id */
export function addPushedIds(ids: string[]): void {
  if (ids.length === 0) return;
  const set = new Set(state.pushedUniqueIds);
  ids.forEach((id) => set.add(id));
  updateState({ pushedUniqueIds: Array.from(set) });
}

/** 过期清理:用最新一次的全量列表重建已推送集合 */
export function reconcilePushedIds(next: string[]): void {
  const current = state.pushedUniqueIds;
  if (current.length === next.length && current.every((v, i) => v === next[i])) return;
  updateState({ pushedUniqueIds: next });
}

/** 新增「已处理」的课程 id(已成功选课或已确认不可选) */
export function addAutoHandledIds(ids: string[]): void {
  if (ids.length === 0) return;
  const set = new Set(state.autoHandledIds);
  ids.forEach((id) => set.add(id));
  updateState({ autoHandledIds: Array.from(set) });
}

/** 过期清理:只保留当前列表里仍存在的已处理记录 */
export function reconcileAutoHandledIds(next: string[]): void {
  const current = state.autoHandledIds;
  if (current.length === next.length && current.every((v, i) => v === next[i])) return;
  updateState({ autoHandledIds: next });
}
