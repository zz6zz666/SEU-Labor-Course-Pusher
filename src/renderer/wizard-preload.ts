/**
 * 首启向导窗口的 preload：只暴露必要的动作，不暴露任何 Node 能力。
 */
import { contextBridge, ipcRenderer } from 'electron';

export interface WizardStatus {
  firstRunCompleted: boolean;
  credentialsConfigured: boolean;
  pushplusConfigured: boolean;
  windowsNotifyEnabled: boolean;
  authState: string;
  watcherMessage: string;
  lastSuccessAt: string | null;
  autoStartEnabled: boolean;
  autoSelectEnabled: boolean;
  filtersConfigured: boolean;
  configPath: string;
  dataDir: string;
  version: string;
}

export interface WizardConfigView {
  username: string;
  password: string;
  pushplusToken: string;
  pushplusEnabled: boolean;
  windowsNotifyEnabled: boolean;
  autoLaunchAtLogin: boolean;
  autoSelect: boolean;
  /** 地点关键字白名单(包含匹配) */
  locations: string[];
  /** 劳动类别黑名单(完全匹配即排除) */
  categories: string[];
}

export interface SaveResult {
  ok: boolean;
  error?: string;
}

export interface BehaviorOptions {
  /** 开机自启:系统设置与 config.json 一并写回 */
  autoStart?: boolean;
  /** 自动选课(默认关闭) */
  autoSelect?: boolean;
}

contextBridge.exposeInMainWorld('seuWizard', {
  getStatus: (): Promise<WizardStatus> => ipcRenderer.invoke('wizard:get-status'),
  getConfig: (): Promise<WizardConfigView> => ipcRenderer.invoke('wizard:get-config'),
  saveAccount: (data: { username: string; password: string }): Promise<SaveResult> =>
    ipcRenderer.invoke('wizard:save-account', data),
  saveNotify: (data: {
    pushplusToken: string;
    pushplusEnabled: boolean;
    windowsNotifyEnabled: boolean;
  }): Promise<SaveResult> => ipcRenderer.invoke('wizard:save-notify', data),
  saveFilters: (data: {
    locations: string[];
    categories: string[];
  }): Promise<SaveResult> => ipcRenderer.invoke('wizard:save-filters', data),
  openLogin: (): Promise<boolean> => ipcRenderer.invoke('wizard:open-login'),
  openCourse: (): Promise<boolean> => ipcRenderer.invoke('wizard:open-course'),
  openExternal: (url: string): Promise<boolean> => ipcRenderer.invoke('wizard:open-external', url),
  verify: (): Promise<{ state: string; reason: string }> => ipcRenderer.invoke('wizard:verify'),
  setBehavior: (opts: BehaviorOptions): Promise<SaveResult> =>
    ipcRenderer.invoke('wizard:set-behavior', opts),
  openConfig: (): Promise<boolean> => ipcRenderer.invoke('wizard:open-config'),
  openLogs: (): Promise<boolean> => ipcRenderer.invoke('wizard:open-logs'),
  close: (): Promise<boolean> => ipcRenderer.invoke('wizard:close'),
});
