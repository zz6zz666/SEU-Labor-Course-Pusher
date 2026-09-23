/**
 * 应用入口:生命周期、单实例锁、模块装配。
 *
 * 启动流程对应 PLAN.md 9.1:
 *   单实例锁 → 重定向 userData → 加载校验配置 → 初始化 logger/store/session
 *   → 首启则弹向导 → 校验登录态(失效则自动登录,再失败转告警态)
 *   → 建立托盘 → 预建隐藏窗口 → 启动轮询循环
 *
 * 注意:登录态校验与自动登录**不由本文件重复实现**,而是统一交给 Watcher 的首轮 tick ——
 * 这样「启动时的失效处理」与「运行中的失效处理」走的是同一条代码路径,不会出现两套逻辑。
 */
import { app, ipcMain, type IpcMainInvokeEvent } from 'electron';
import fs from 'fs';
import { configStore, getConfig, hasCredentials, safeSummary, updateConfig } from '../config';
import { createLogger, getLogDir, initLogger, reconfigureLogger } from '../logger';
import { resolvePaths } from '../paths';
import { flushState, getState, initStore, setAuthState, updateState } from '../store';
import { probeAuth } from '../core/auth';
import { clearSession, getSession } from '../core/session';
import { Watcher, type WatcherStatus } from '../core/watcher';
import { setNativeChannel } from '../notify/notifier';
import { applyAutoStart, isAutoStartEnabled, syncAutoStartFromConfig } from './autostart';
import { sendNativeNotification, setAuthExpiredClickHandler } from './notifications';
import { TrayController } from './tray';
import {
  closeWizardWindow,
  getHiddenWindow,
  getWindowContextFor,
  openConfigFile,
  openExternalUrl,
  openInteractiveWindow,
  openLogFolder,
  openLoginWindow,
  openWizardWindow,
} from './windows';

const SMOKE_TEST = process.argv.includes('--smoke-test');

const paths = resolvePaths();
let log = createLogger('main');
let watcher: Watcher | null = null;
let tray: TrayController | null = null;

// ------------------------------------------------------------ 进程级前置

// 必须在 app ready 之前重定向 userData,之后 session 分区才会落到 data/Partitions/
if (!paths.isPackaged) {
  app.setPath('userData', paths.dataDir);
}

/**
 * 默认关闭硬件加速。
 *
 * 理由:这是个后台常驻服务,99% 的时间没有窗口,不需要 GPU;而一旦运行在没有可用 GPU 的
 * 环境里(云服务器、RDP 远程桌面、无显卡的虚拟机、受限容器),Chromium 的 GPU 子进程会反复
 * 启动失败,重试若干次后以 `GPU process isn't usable. Goodbye.` 的 FATAL 直接带走整个进程 ——
 * 对一个「应当在后台安静运行」的程序来说这是完全不可接受的失败模式。
 *
 * 实测:仅加 `--disable-gpu` 仍会崩溃(Chromium 依然要起一个软件渲染的 GPU 进程);
 * 必须再加 `--in-process-gpu` 把 GPU 服务收进主进程。选择它而不是 `--no-sandbox`,
 * 是为了不削弱渲染进程的沙箱隔离。
 *
 * PLAN.md 第 15 节明确把云服务器部署列为备选方案,所以这里按「能跑」优先。
 * 有独显、又希望登录页滚动更顺滑的,可用环境变量 `SEU_DAEMON_ENABLE_GPU=1` 重新打开。
 */
if (process.env.SEU_DAEMON_ENABLE_GPU !== '1') {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch('disable-gpu');
  app.commandLine.appendSwitch('disable-gpu-compositing');
  app.commandLine.appendSwitch('in-process-gpu');
}

// 通知正确显示应用名称的前提(需与 electron-builder 的 appId 一致)
app.setAppUserModelId('cn.edu.seu.laborpusher');

// ------------------------------------------------------------ 单实例锁

if (!app.requestSingleInstanceLock()) {
  // 已在运行:唤起现有实例并退出
  console.log('[main] 检测到已有实例在运行,本次启动退出');
  app.quit();
} else {
  app.on('second-instance', () => {
    log.info('收到第二次启动请求,已打开设置向导');
    openWizardWindow();
  });

  app.whenReady().then(() => {
    void bootstrap();
  });
}

// 常驻托盘:关掉所有窗口也不退出
app.on('window-all-closed', () => {
  // 有意留空 —— 后台服务不该因为窗口关闭而退出
});

app.on('before-quit', () => {
  watcher?.stop();
  flushState();
  tray?.destroy();
  log.info('=== 退出 ===');
});

process.on('uncaughtException', (err) => {
  log.error('未捕获异常:', err);
});
process.on('unhandledRejection', (reason) => {
  log.error('未处理的 Promise 拒绝:', reason);
});

// ------------------------------------------------------------ 启动装配

async function bootstrap(): Promise<void> {
  // 1) 配置
  const { warnings } = configStore.load();
  const cfg = getConfig();

  // 2) 日志
  initLogger(cfg.logging.level, cfg.logging.retentionDays);
  log = createLogger('main');

  log.info('================ SEU 劳动教育课程监控守护进程 ================');
  log.info(`版本 ${app.getVersion()} · ${paths.isPackaged ? '打包态' : '开发态'}`);
  log.info('数据目录:', paths.dataDir);
  log.info('配置文件:', paths.configPath);
  log.info('运行配置:', safeSummary(cfg));
  warnings.forEach((w) => log.warn(w));

  // 3) 状态
  initStore();
  log.info('已记录课程数:', getState().pushedUniqueIds.length);

  // 4) 会话分区(此时才真正创建 persist:seu-labor)
  getSession();

  // 5) 配置热重载
  configStore.watch();
  configStore.on('change', (next, warns: string[]) => {
    reconfigureLogger(next.logging.level, next.logging.retentionDays);
    log.info('配置已热重载:', safeSummary(next));
    warns.forEach((w) => log.warn(w));
    // 让系统自启与配置保持一致:否则手动改 config.json 后,
    // 托盘勾选(读系统实际)会与配置分叉。
    syncAutoStartFromConfig();
    tray?.refresh();
  });

  // 6) 通知通道装配(原生通道由 main 注入,避免 notify/ 反向依赖 main/)
  setNativeChannel(sendNativeNotification);
  // 点击「登录失效」通知后走与托盘一致的完整流程(开窗 → 关闭后校验 → 立即恢复监控)
  setAuthExpiredClickHandler(openLoginAndTrack);

  // 7) 先对齐开机自启(再建托盘,保证托盘勾选第一次就反映系统实际),再建隐藏窗口与托盘
  syncAutoStartFromConfig();
  getHiddenWindow();
  createTray();

  // 8) IPC
  registerIpc();

  if (SMOKE_TEST) {
    await runSmokeTest();
    return;
  }

  // 9) 首启向导
  if (!getState().firstRunCompleted) {
    log.info('检测到首次运行,弹出首启向导');
    openWizardWindow();
  }

  // 10) 启动监控循环(登录态校验与自动登录由首轮 tick 统一处理)
  if (hasCredentials()) {
    startWatcher();
  } else {
    log.warn('config.json 尚未填写账号密码,监控循环暂不启动;请在向导或配置文件中补全');
  }
}

/**
 * 登录态一旦有效就自动开始监控 —— 不再需要用户点「完成并开始监控」。
 * 关闭向导、登录窗口关闭后校验通过、手动检查登录态通过,都会走到这里。
 */
function ensureMonitoring(): void {
  if (!watcher && hasCredentials()) {
    log.info('检测到已登录,自动开始监控');
    startWatcher();
  } else {
    watcher?.onLoginRestored();
  }
}

function startWatcher(): void {
  if (watcher) return;
  watcher = new Watcher({
    onStatusChange: () => {
      tray?.refresh();
    },
    onNeedManualLogin: () => {
      const cfg = getConfig();
      const firstRun = getState().firstRunCompleted;
      if (!firstRun) {
        // 首次运行:由向导负责引导,不额外弹窗
        openWizardWindow();
        return;
      }
      if (cfg.behavior.autoOpenOnAuthFailure) {
        log.info('按配置自动弹出内置浏览器登录页');
        openLoginAndTrack();
      }
    },
  });
  watcher.start();
}

// ------------------------------------------------------------ 托盘

function createTray(): void {
  tray = new TrayController({
    getStatusLine: () => {
      const status = watcher?.getStatus();
      if (!status) {
        return { title: '状态:待配置', detail: '请在 config.json 中填写账号密码' };
      }
      const authText =
        status.authState === 'valid' ? '登录正常' : status.authState === 'expired' ? '登录已失效' : '登录待确认';
      const next = status.nextRunAt
        ? `下次 ${new Date(status.nextRunAt).toLocaleTimeString('zh-CN', { hour12: false })}`
        : '未排期';
      return {
        title: `${authText} · 符合条件 ${status.currentValidCount} 门 · 今日新推送 ${status.pushedTodayCount} 门`,
        detail: `${status.lastMessage}（${next}）`,
      };
    },
    onOpenCourse: () => openInteractiveWindow({ mode: 'course', firstRun: false }),
    onOpenLogin: () => openLoginAndTrack(),
    onFetchNow: () => watcher?.triggerNow(),
    onOpenLog: () => void openLogFolder(),
    onOpenConfig: () => void openConfigFile(),
    isAutoStart: () => isAutoStartEnabled(),
    onToggleAutoStart: (enabled) => {
      // 系统设置与 config.json 必须一起改:只改系统的话,下次启动
      // syncAutoStartFromConfig() 会按配置里的旧值把它改回去,用户的开关就白按了。
      // 以系统实际生效的结果写回,避免「配置写了但没生效」。
      const applied = applyAutoStart(enabled);
      const res = updateConfig((cfg) => {
        cfg.behavior.autoLaunchAtLogin = applied;
      });
      if (!res.ok) log.warn('开机自启写回 config.json 失败:', res.error);
      tray?.refresh();
    },
    isAutoSelect: () => getConfig().behavior.autoSelect,
    onToggleAutoSelect: (enabled) => {
      const res = updateConfig((cfg) => {
        cfg.behavior.autoSelect = enabled;
      });
      if (!res.ok) log.warn('自动选课写回 config.json 失败:', res.error);
      log.info(`用户从托盘切换自动选课:${enabled ? '开启' : '关闭'}`);
      tray?.refresh();
    },
    onClearSession: () => {
      void clearSessionAndRelogin();
    },
    onOpenWizard: () => openWizardWindow(),
    onQuit: () => {
      log.info('用户从托盘退出');
      app.quit();
    },
  });
  tray.create();
}

/** 清除登录态(托盘「清除登录态」用) */
async function clearSessionAndRelogin(): Promise<void> {
  await clearSession();
  setAuthState('expired');
  log.info('已清除登录态,弹出登录窗口');
  openLoginAndTrack();
}

// ------------------------------------------------------------ 登录窗口与闭环

/**
 * 打开内置浏览器登录窗口,并在窗口关闭后校验登录态(PLAN.md 9.4)。
 * 因为窗口与后台共用 session,校验只看服务端响应即可。
 */
function openLoginAndTrack(): void {
  const win = openLoginWindow(!getState().firstRunCompleted);
  win.once('closed', () => {
    void (async () => {
      const probe = await probeAuth();
      if (probe.state === 'valid') {
        log.info('内置窗口关闭后校验:登录态有效');
        setAuthState('valid');
        updateState({ firstRunCompleted: true });
        // 不关闭向导窗口:用户在向导里应能看到登录态变为「已登录」
        ensureMonitoring();
      } else {
        log.warn('内置窗口关闭后校验:仍未登录 —', probe.reason);
      }
    })();
  });
}

// ------------------------------------------------------------ IPC

function registerIpc(): void {
  ipcMain.handle('window:context', (event: IpcMainInvokeEvent) =>
    getWindowContextFor(event.sender.id),
  );

  // 仅「登录窗口」可获取预填凭据,其他来源一律拒绝
  ipcMain.handle('auth:get-prefill', (event: IpcMainInvokeEvent) => {
    const ctx = getWindowContextFor(event.sender.id);
    if (!ctx || ctx.mode !== 'login') {
      log.warn('拒绝向非登录窗口下发预填凭据');
      return null;
    }
    const cfg = getConfig();
    return { username: cfg.credentials.username, password: cfg.credentials.password };
  });

  ipcMain.handle('wizard:get-status', () => {
    const status: WatcherStatus | null = watcher?.getStatus() ?? null;
    const cfg = getConfig();
    return {
      firstRunCompleted: getState().firstRunCompleted,
      credentialsConfigured: hasCredentials(cfg),
      pushplusConfigured: Boolean(cfg.push.pushplus.token),
      windowsNotifyEnabled: cfg.push.windows.enabled,
      authState: getState().authState,
      watcherMessage: status?.lastMessage ?? '监控循环未启动',
      lastSuccessAt: getState().lastSuccessAt,
      autoStartEnabled: isAutoStartEnabled(),
      autoSelectEnabled: cfg.behavior.autoSelect,
      filtersConfigured: cfg.filters.locations.length > 0 || cfg.filters.categories.length > 0,
      configPath: paths.configPath,
      dataDir: paths.dataDir,
      version: app.getVersion(),
    };
  });

  // 向导内置表单:读取当前配置用于回填。
  // 注意:仅本地向导窗口可调用;返回值不写日志、不外发。
  ipcMain.handle('wizard:get-config', () => {
    const cfg = getConfig();
    return {
      username: cfg.credentials.username,
      password: cfg.credentials.password,
      pushplusToken: cfg.push.pushplus.token,
      pushplusEnabled: cfg.push.pushplus.enabled,
      windowsNotifyEnabled: cfg.push.windows.enabled,
      autoLaunchAtLogin: cfg.behavior.autoLaunchAtLogin,
      autoSelect: cfg.behavior.autoSelect,
      locations: cfg.filters.locations,
      categories: cfg.filters.categories,
    };
  });

  ipcMain.handle('wizard:save-account', (_event, data?: { username?: string; password?: string }) => {
    const username = String(data?.username ?? '').trim();
    const password = String(data?.password ?? '');
    const res = updateConfig((cfg) => {
      cfg.credentials.username = username;
      cfg.credentials.password = password;
    });
    if (!res.ok) return { ok: false, error: res.error };
    // 不在此处启动监控:交给「完成」或关窗时统一处理,避免向导中途就开始轮询
    log.info('向导已保存账号(内容不落日志)');
    return { ok: true };
  });

  ipcMain.handle(
    'wizard:save-notify',
    (
      _event,
      data?: { pushplusToken?: string; pushplusEnabled?: boolean; windowsNotifyEnabled?: boolean }
    ) => {
      const token = String(data?.pushplusToken ?? '').trim();
      const pushplusEnabled = Boolean(data?.pushplusEnabled) && token.length > 0;
      const windowsNotifyEnabled = Boolean(data?.windowsNotifyEnabled);
      const res = updateConfig((cfg) => {
        cfg.push.pushplus.token = token;
        cfg.push.pushplus.enabled = pushplusEnabled;
        cfg.push.windows.enabled = windowsNotifyEnabled;
      });
      if (!res.ok) return { ok: false, error: res.error };
      log.info('向导已保存通知设置:', {
        pushplusEnabled,
        pushplusTokenConfigured: token.length > 0,
        windowsNotifyEnabled,
      });
      return { ok: true };
    }
  );

  // 筛选:向导 UI 是「地点关键字 + 劳动类型白名单多选」,写入 config 时
  // 地点仍是白名单数组,类型则转成黑名单(filters.categories = 未勾选的类型)。
  ipcMain.handle(
    'wizard:save-filters',
    (_event, data?: { locations?: unknown; categories?: unknown }) => {
      const locations = Array.isArray(data?.locations)
        ? data.locations.map((s) => String(s).trim()).filter(Boolean)
        : [];
      const categories = Array.isArray(data?.categories)
        ? data.categories.map((s) => String(s).trim()).filter(Boolean)
        : [];
      const res = updateConfig((cfg) => {
        cfg.filters.locations = locations;
        cfg.filters.categories = categories;
      });
      if (!res.ok) return { ok: false, error: res.error };
      log.info('向导已保存筛选设置:', { locations, categories });
      return { ok: true };
    }
  );

  ipcMain.handle('wizard:open-login', () => {
    openLoginAndTrack();
    return true;
  });

  // 欢迎页「打开选课页面」:走与托盘/通知一致的内置浏览器(共用会话,打开即已登录)
  ipcMain.handle('wizard:open-course', () => {
    openInteractiveWindow({ mode: 'course', firstRun: false });
    return true;
  });

  // PushPlus 申请地址等外链:交给系统默认浏览器,避免向导窗口自身被导航走
  ipcMain.handle('wizard:open-external', (_event, url?: string) => openExternalUrl(String(url ?? '')));

  ipcMain.handle('wizard:verify', async () => {
    const probe = await probeAuth();
    if (probe.state === 'valid') {
      setAuthState('valid');
      updateState({ firstRunCompleted: true });
      ensureMonitoring();
    } else if (probe.state === 'expired') {
      setAuthState('expired');
    }
    return { state: probe.state, reason: probe.reason };
  });

  // 开机自启 / 自动选课:向导里改动即写回,不再有「完成并开始监控」这个动作
  ipcMain.handle(
    'wizard:set-behavior',
    (_event, opts?: { autoStart?: boolean; autoSelect?: boolean }) => {
      if (typeof opts?.autoStart === 'boolean') {
        const applied = applyAutoStart(opts.autoStart);
        const res = updateConfig((cfg) => {
          cfg.behavior.autoLaunchAtLogin = applied;
        });
        if (!res.ok) return { ok: false, error: res.error };
        log.info(`向导切换开机自启:${applied ? '开启' : '关闭'}`);
      }
      if (typeof opts?.autoSelect === 'boolean') {
        const res = updateConfig((cfg) => {
          cfg.behavior.autoSelect = opts.autoSelect as boolean;
        });
        if (!res.ok) return { ok: false, error: res.error };
        log.info(`向导切换自动选课:${opts.autoSelect ? '开启' : '关闭'}`);
      }
      tray?.refresh();
      return { ok: true };
    }
  );

  ipcMain.handle('wizard:open-config', () => {
    void openConfigFile();
    return true;
  });

  ipcMain.handle('wizard:open-logs', () => {
    void openLogFolder();
    return true;
  });

  ipcMain.handle('wizard:close', () => {
    closeWizardWindow();
    ensureMonitoring();
    return true;
  });
}

// ------------------------------------------------------------ 冒烟自检

/**
 * `npm run smoke` —— 只验证骨架与网络可达性,不发通知、不进监控循环。
 * 便于在没有凭据的情况下确认:路径、日志、托盘、会话分区、UA 是否都正常。
 */
async function runSmokeTest(): Promise<void> {
  const lines: string[] = [];
  lines.push(`数据目录: ${paths.dataDir}`);
  lines.push(`配置文件: ${paths.configPath} (存在: ${fs.existsSync(paths.configPath)})`);
  lines.push(`日志目录: ${getLogDir()}`);
  lines.push(`托盘图标: ${tray ? '已创建' : '未创建'}`);
  lines.push(`隐藏窗口: ${getHiddenWindow() && !getHiddenWindow().isDestroyed() ? '已创建' : '未创建'}`);
  lines.push(`开机自启(系统实际): ${isAutoStartEnabled()}`);

  log.info('[冒烟] 会话分区 UA 已设置,开始探测站点可达性…');
  const probe = await probeAuth(15000);
  lines.push(`站点探测: ${probe.state} — ${probe.reason}`);
  lines.push(`正文长度: ${probe.html.length} 字节`);

  lines.forEach((l) => log.info(`[冒烟] ${l}`));
  log.info('[冒烟] SMOKE-TEST-OK');

  setTimeout(() => {
    app.quit();
  }, 1500);
}
