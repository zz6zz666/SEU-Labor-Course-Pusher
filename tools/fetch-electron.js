/**
 * 手动补齐 Electron 运行时。
 *
 * 为什么需要它：`npm install` 里 Electron 的 postinstall 会去下载约 110MB 的运行时，
 * 镜像不稳时这一步会长时间静默卡住（不会报错，只是不动）。此时可以中断 npm，
 * 用本脚本直连镜像把运行时补齐，不必重跑整个安装。
 *
 * 用法： node tools/fetch-electron.js
 */
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const { Readable } = require('stream');
const { pipeline } = require('stream/promises');

const ROOT = path.resolve(__dirname, '..');
const ELECTRON_DIR = path.join(ROOT, 'node_modules', 'electron');
const DIST_DIR = path.join(ELECTRON_DIR, 'dist');

const MIRRORS = [
  (v) => `https://npmmirror.com/mirrors/electron/v${v}/electron-v${v}-win32-x64.zip`,
  (v) => `https://cdn.npmmirror.com/binaries/electron/v${v}/electron-v${v}-win32-x64.zip`,
];

function human(bytes) {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

async function main() {
  if (!fs.existsSync(ELECTRON_DIR)) {
    console.error('找不到 node_modules/electron，请先运行 npm install');
    process.exit(1);
  }

  const version = JSON.parse(
    fs.readFileSync(path.join(ELECTRON_DIR, 'package.json'), 'utf8'),
  ).version;

  const exePath = path.join(DIST_DIR, 'electron.exe');
  if (fs.existsSync(exePath)) {
    console.log(`Electron 运行时已就绪：${exePath}`);
    return;
  }

  const sevenZip = path.join(ROOT, 'node_modules', '7zip-bin', 'win', 'x64', '7za.exe');
  if (!fs.existsSync(sevenZip)) {
    console.error('找不到 7za.exe（node_modules/7zip-bin），无法解压');
    process.exit(1);
  }

  const zipPath = path.join(ROOT, `.electron-v${version}-win32-x64.zip`);
  let lastError = null;

  for (const buildUrl of MIRRORS) {
    const url = buildUrl(version);
    try {
      console.log(`下载 Electron ${version}\n  ${url}`);
      const res = await fetch(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      const total = Number(res.headers.get('content-length') || 0);
      let received = 0;
      let lastReport = 0;

      const source = Readable.fromWeb(res.body);
      source.on('data', (chunk) => {
        received += chunk.length;
        const now = Date.now();
        if (now - lastReport > 1500) {
          lastReport = now;
          const pct = total ? ` (${((received / total) * 100).toFixed(0)}%)` : '';
          console.log(`  ${human(received)}${pct}`);
        }
      });

      await pipeline(source, fs.createWriteStream(zipPath));
      console.log(`  下载完成：${human(received)}`);
      lastError = null;
      break;
    } catch (err) {
      lastError = err;
      console.warn(`  该镜像失败：${err.message}，换下一个`);
    }
  }

  if (lastError) {
    console.error(`全部镜像均失败：${lastError.message}`);
    process.exit(1);
  }

  console.log('解压到 node_modules/electron/dist …');
  fs.mkdirSync(DIST_DIR, { recursive: true });
  execFileSync(sevenZip, ['x', zipPath, `-o${DIST_DIR}`, '-y'], { stdio: 'inherit' });
  fs.unlinkSync(zipPath);

  // electron 的 index.js 靠 path.txt 找到可执行文件
  fs.writeFileSync(path.join(ELECTRON_DIR, 'path.txt'), 'electron.exe', 'utf8');

  if (!fs.existsSync(exePath)) {
    console.error('解压后仍未找到 electron.exe，请检查压缩包结构');
    process.exit(1);
  }
  console.log(`完成：${exePath}`);
  try {
    const out = execFileSync(exePath, ['--version'], { encoding: 'utf8' }).trim();
    console.log(`运行时自报版本：${out}`);
  } catch (err) {
    console.warn(`运行时自报版本失败（可能缺少系统依赖）：${err.message}`);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
