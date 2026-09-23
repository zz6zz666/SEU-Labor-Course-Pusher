/**
 * 开机自启开关(Windows 注册表 Run 项,由 Electron 封装)。
 */
import { app } from 'electron';
import { getConfig } from '../config';
import { createLogger } from '../logger';
import { resolvePaths } from '../paths';

const log = createLogger('autostart');

export function isAutoStartEnabled(): boolean {
  try {
    if (resolvePaths().isPackaged) {
      return app.getLoginItemSettings().openAtLogin;
    }
    // 开发态:带 --processStart 参数查询,匹配下面写入时的参数集合
    return app.getLoginItemSettings({ args: devArgs() }).openAtLogin;
  } catch (err) {
    log.warn('读取自启状态失败:', err);
    return false;
  }
}

function devArgs(): string[] {
  return [resolvePaths().projectRoot];
}

export function applyAutoStart(enabled: boolean): boolean {
  const { isPackaged } = resolvePaths();
  try {
    if (isPackaged) {
      app.setLoginItemSettings({
        openAtLogin: enabled,
        openAsHidden: true,
        path: process.execPath,
        args: [],
      });
    } else {
      // 开发态:让 electron.exe 带上项目路径
      app.setLoginItemSettings({
        openAtLogin: enabled,
        openAsHidden: true,
        path: process.execPath,
        args: devArgs(),
      });
    }
    log.info(`开机自启已${enabled ? '开启' : '关闭'}`);
    return isAutoStartEnabled();
  } catch (err) {
    log.error('设置开机自启失败:', err);
    return false;
  }
}

/** 启动时对齐配置与系统真实状态 */
export function syncAutoStartFromConfig(): void {
  const desired = getConfig().behavior.autoLaunchAtLogin;
  const actual = isAutoStartEnabled();
  if (desired !== actual) {
    log.info(`启动自启状态与配置不一致(配置 ${desired} / 实际 ${actual}),按配置纠正`);
    applyAutoStart(desired);
  }
}
