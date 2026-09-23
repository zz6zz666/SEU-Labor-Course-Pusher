/**
 * 路径解析。
 *
 * 设计要点:
 * - 开发态(未打包):运行期数据落在项目 `data/` 下,配置落在 `config.json`
 *   —— 与 PLAN.md 第 5 节的目录结构一致,便于排查。
 * - 打包态:配置与数据一并落在 Electron 默认 userData 目录(可写),
 *   避免把数据写进 Program Files。
 * - 两种形态都可通过环境变量 `SEU_DAEMON_DATA_DIR` / `SEU_DAEMON_CONFIG` 覆盖。
 *
 * 本模块必须在 `app.setPath('userData', ...)` 之前调用,故不缓存 app 相关状态。
 */
import path from 'path';
import fs from 'fs';
import { app } from 'electron';

export interface ResolvedPaths {
  projectRoot: string;
  dataDir: string;
  configPath: string;
  logDir: string;
  snapshotDir: string;
  statePath: string;
  isPackaged: boolean;
}

let cached: ResolvedPaths | null = null;

/**
 * 定位工程根目录。
 *
 * 不能靠「__dirname 向上数几级」——`src/paths.ts` 编译后落在 `dist/paths.js`,
 * 而其他模块落在 `dist/main/*.js`,层数并不一致,数级必然会错。
 * 改为按特征文件向上查找:`config.example.json` 在开发态位于项目根目录,
 * 打包后位于 asar 根目录,两处都能命中且不会误命中别处。
 */
function findProjectRoot(startDir: string): string {
  let dir = startDir;
  for (let depth = 0; depth < 6; depth += 1) {
    if (fs.existsSync(path.join(dir, 'config.example.json'))) return dir;
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return path.resolve(startDir, '..', '..');
}

export function resolvePaths(): ResolvedPaths {
  if (cached) return cached;

  const projectRoot = findProjectRoot(__dirname);
  const isPackaged = app.isPackaged;

  const dataDir = process.env.SEU_DAEMON_DATA_DIR
    ? path.resolve(process.env.SEU_DAEMON_DATA_DIR)
    : isPackaged
      ? app.getPath('userData')
      : path.join(projectRoot, 'data');

  const configPath = process.env.SEU_DAEMON_CONFIG
    ? path.resolve(process.env.SEU_DAEMON_CONFIG)
    : isPackaged
      ? path.join(dataDir, 'config.json')
      : path.join(projectRoot, 'config.json');

  cached = {
    projectRoot,
    dataDir,
    configPath,
    logDir: path.join(dataDir, 'logs'),
    snapshotDir: path.join(dataDir, 'snapshots'),
    statePath: path.join(dataDir, 'state.json'),
    isPackaged,
  };

  for (const dir of [cached.dataDir, cached.logDir, cached.snapshotDir]) {
    try {
      fs.mkdirSync(dir, { recursive: true });
    } catch {
      /* 目录已存在或不可创建,由后续写入时报错 */
    }
  }
  return cached;
}
