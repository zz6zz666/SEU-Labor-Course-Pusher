/**
 * 托盘图标与菜单(PLAN.md 8.4)。
 *
 * 选课与登录统一走内置浏览器:与后台共用会话,打开即已登录,不需要再登一次。
 */
import { Menu, Tray, nativeImage, app } from 'electron';
import { createLogger } from '../logger';
import { TRAY_ICON_PNG_BASE64 } from './icon-data';

const log = createLogger('tray');

export interface TrayDeps {
  /** 状态摘要,显示为托盘提示与菜单首项 */
  getStatusLine(): { title: string; detail: string };
  onOpenCourse: () => void;
  onOpenLogin: () => void;
  onFetchNow: () => void;
  onOpenLog: () => void;
  onOpenConfig: () => void;
  isAutoStart: () => boolean;
  onToggleAutoStart: (enabled: boolean) => void;
  isAutoSelect: () => boolean;
  onToggleAutoSelect: (enabled: boolean) => void;
  onClearSession: () => void;
  onOpenWizard: () => void;
  onQuit: () => void;
}

export class TrayController {
  private tray: Tray | null = null;

  constructor(private deps: TrayDeps) {}

  create(): void {
    if (this.tray) return;

    const image = nativeImage.createFromBuffer(Buffer.from(TRAY_ICON_PNG_BASE64, 'base64'));
    image.setTemplateImage(false);

    this.tray = new Tray(image);
    this.tray.setToolTip('SEU 劳动教育课程推送助手');
    // 单击托盘图标直接弹出菜单(Windows 上更符合直觉)
    this.tray.on('click', () => this.tray?.popUpContextMenu());
    this.refresh();
    log.info('托盘图标已建立');
  }

  /** 状态变化时重建菜单 */
  refresh(): void {
    if (!this.tray) return;

    const { title, detail } = this.deps.getStatusLine();
    this.tray.setToolTip(`SEU 劳动教育课程推送助手\n${title}\n${detail}`);

    const menu = Menu.buildFromTemplate([
      { label: title, enabled: false },
      { label: detail, enabled: false },
      { type: 'separator' },
      {
        label: '用内置浏览器打开选课页',
        click: () => this.deps.onOpenCourse(),
      },
      {
        label: '用内置浏览器打开登录页 / 重新登录',
        click: () => this.deps.onOpenLogin(),
      },
      { type: 'separator' },
      { label: '立即抓取一次', click: () => this.deps.onFetchNow() },
      { label: '查看日志', click: () => this.deps.onOpenLog() },
      { label: '打开 config.json', click: () => this.deps.onOpenConfig() },
      { label: '设置向导 / 检查配置', click: () => this.deps.onOpenWizard() },
      { type: 'separator' },
      {
        label: '开机自启',
        type: 'checkbox',
        checked: this.deps.isAutoStart(),
        click: (item) => {
          this.deps.onToggleAutoStart(item.checked);
          this.refresh();
        },
      },
      {
        label: '自动选课(默认关)',
        type: 'checkbox',
        checked: this.deps.isAutoSelect(),
        click: (item) => {
          this.deps.onToggleAutoSelect(item.checked);
          this.refresh();
        },
      },
      { label: '清除登录态(重新登录)', click: () => this.deps.onClearSession() },
      { type: 'separator' },
      { label: `版本 ${app.getVersion()}`, enabled: false },
      { label: '退出', click: () => this.deps.onQuit() },
    ]);

    this.tray.setContextMenu(menu);
  }

  destroy(): void {
    this.tray?.destroy();
    this.tray = null;
  }
}
