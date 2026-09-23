/**
 * 修复 electron-builder 的 winCodeSign 缓存。
 *
 * 为什么需要它：Windows 打包时 electron-builder 会下载 winCodeSign-2.6.0.7z（约 5.6MB），
 * 其中包含 macOS 的 .dylib 符号链接。Windows 默认不允许普通用户创建符号链接
 * （需要开发者模式或管理员权限），7-Zip 解压会因此报错退出：
 *
 *   ERROR: Cannot create symbolic link : 客户端没有所需的特权。
 *
 * electron-builder 重试 4 次后判定失败，`npm run pack` / `npm run dist` 整个中断。
 * 但那些符号链接只服务于 macOS，Windows 打包真正需要的只有 rcedit 与 signtool。
 *
 * 本脚本把归档解压出来、忽略符号链接错误，然后按 electron-builder 期望的目录名
 * （winCodeSign-2.6.0）放到缓存里；之后 electron-builder 检测到缓存存在就直接使用，
 * 不再重新下载解压。
 *
 * 用法： node tools/fetch-win-code-sign.js
 *        npm run fix-win-code-sign
 *
 * 说明：版本号 2.6.0 对应 electron-builder 25.x 内置的 app-builder 映射；
 * 若将来升级 electron-builder 后日志里出现别的 winCodeSign 版本，改这里即可。
 */
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');
const { Readable } = require('stream');
const { pipeline } = require('stream/promises');

const ROOT = path.resolve(__dirname, '..');
const VERSION = '2.6.0';
const NAME = `winCodeSign-${VERSION}`;

const CACHE_BASE = process.env.ELECTRON_BUILDER_CACHE
  ? path.resolve(process.env.ELECTRON_BUILDER_CACHE)
  : process.platform === 'win32'
    ? path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local'), 'electron-builder', 'Cache')
    : process.platform === 'darwin'
      ? path.join(os.homedir(), 'Library', 'Caches', 'electron-builder')
      : path.join(os.homedir(), '.cache', 'electron-builder');

const CACHE_DIR = path.join(CACHE_BASE, 'winCodeSign');
const TARGET = path.join(CACHE_DIR, NAME);

const MIRRORS = [
  (n) => `https://npmmirror.com/mirrors/electron-builder-binaries/${n}/${n}.7z`,
  (n) => `https://github.com/electron-userland/electron-builder-binaries/releases/download/${n}/${n}.7z`,
];

function human(bytes) {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 判断缓存是否可用：rcedit 与 signtool 都在即视为完好 */
function isUsable(dir) {
  return (
    fs.existsSync(path.join(dir, 'rcedit-x64.exe')) &&
    fs.existsSync(path.join(dir, 'windows-10', 'x64', 'signtool.exe'))
  );
}

async function download(url, output) {
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
  await pipeline(source, fs.createWriteStream(output));
  return received;
}

async function main() {
  if (isUsable(TARGET)) {
    console.log(`winCodeSign 缓存已就绪：${TARGET}`);
    return;
  }

  const sevenZip = path.join(ROOT, 'node_modules', '7zip-bin', 'win', 'x64', '7za.exe');
  if (!fs.existsSync(sevenZip)) {
    console.error('找不到 7za.exe（node_modules/7zip-bin），请先运行 npm install');
    process.exit(1);
  }

  fs.mkdirSync(CACHE_DIR, { recursive: true });
  const zipPath = path.join(CACHE_DIR, `${NAME}.download.7z`);
  const extractDir = path.join(CACHE_DIR, `${NAME}.extract`);

  let lastError = null;
  for (const buildUrl of MIRRORS) {
    const url = buildUrl(NAME);
    try {
      console.log(`下载 ${NAME}\n  ${url}`);
      const size = await download(url, zipPath);
      console.log(`  下载完成：${human(size)}`);
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

  fs.rmSync(extractDir, { recursive: true, force: true });
  fs.mkdirSync(extractDir, { recursive: true });

  console.log('解压（忽略 macOS 符号链接错误）…');
  try {
    execFileSync(sevenZip, ['x', zipPath, `-o${extractDir}`, '-y'], { stdio: 'ignore' });
  } catch (err) {
    // 7-Zip 遇到无法创建的符号链接会以退出码 2 结束，但其余文件已解压出来
    console.warn(`  7za 退出码非 0（${err.status}），继续校验解压结果`);
  }
  fs.rmSync(zipPath, { force: true });

  if (!isUsable(extractDir)) {
    console.error('解压结果缺少 rcedit-x64.exe 或 signtool.exe，请检查归档内容');
    process.exit(1);
  }

  fs.rmSync(TARGET, { recursive: true, force: true });
  fs.renameSync(extractDir, TARGET);
  console.log(`完成：${TARGET}`);
  console.log('现在可以重新运行 npm run pack / npm run dist。');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
