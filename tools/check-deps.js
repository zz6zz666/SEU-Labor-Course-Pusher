/**
 * 检查并修复 node_modules 的完整性。
 *
 * 存在理由：本项目的依赖安装曾多次被中断（镜像慢 / npm 解析阶段卡住），
 * 而 npm 只按「目录存在 + package.json 版本正确」判断包是否装好，
 * **不会发现「目录在但文件缺」**——典型表现是 `out/` 或 `lib/` 整个缺失，
 * 直到运行时才报 `Cannot find module '.../out/index.js'`。
 *
 * 更麻烦的是：中断的安装不会写出 package-lock.json，于是下次 install 又要从零解析
 * 整棵依赖树，慢到再被中断，形成死循环。所以这里绕开 npm 的解析器，直接按
 * 「声明的入口文件是否存在」逐个校验，并只修复真正坏掉的那些。
 *
 * 用法：
 *   node tools/check-deps.js          # 只检查
 *   node tools/check-deps.js --fix    # 检查并修复（从镜像取 tarball 解压回去）
 */
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const ROOT = path.resolve(__dirname, '..');
const NM = path.join(ROOT, 'node_modules');
const SEVEN_ZIP = path.join(NM, '7zip-bin', 'win', 'x64', '7za.exe');
const REGISTRY = 'https://registry.npmmirror.com';
const FIX = process.argv.includes('--fix');

/** 收集一个包声明的入口文件（相对包根目录） */
function declaredEntries(meta) {
  const out = new Set();
  if (typeof meta.main === 'string') out.add(meta.main.replace(/^\.\//, ''));
  const walk = (node) => {
    if (typeof node === 'string') {
      if (node.startsWith('./')) out.add(node.replace(/^\.\//, ''));
      return;
    }
    if (Array.isArray(node)) return node.forEach(walk);
    if (node && typeof node === 'object') Object.values(node).forEach(walk);
  };
  if (meta.exports) walk(meta.exports);
  if (meta.bin) {
    if (typeof meta.bin === 'string') out.add(meta.bin.replace(/^\.\//, ''));
    else Object.values(meta.bin).forEach((v) => typeof v === 'string' && out.add(v.replace(/^\.\//, '')));
  }
  // 未声明 main/exports 时 Node 默认找 index.js
  if (out.size === 0 && !meta.exports && !meta.bin) out.add('index.js');
  return [...out];
}

const BUILD_DIRS = ['out', 'lib', 'dist', 'src', 'build', 'bin', 'cjs', 'esm'];

/**
 * 按 Node 的解析规则判断某个入口能否落地。
 *
 * 不能直接 existsSync：`commander` 的 main 是 `index`（无扩展名），
 * `stat-mode` 是 `dist/src/index`，Node 会补 `.js`。漏掉这一步会产出大量误报。
 * 另外 `undici-types`、`type-fest` 这类纯类型包只有 .d.ts，也算完好。
 */
function resolvesEntry(pkgDir, rel) {
  const clean = rel.replace(/\/+$/, '');
  const exts = ['', '.js', '.cjs', '.mjs', '.node', '.json'];
  for (const ext of exts) {
    const p = path.join(pkgDir, clean + ext);
    if (fs.existsSync(p) && fs.statSync(p).isFile()) return true;
  }
  // 纯类型声明
  const dts = ['.d.ts', '.d.mts', '.d.cts'];
  for (const ext of dts) {
    if (fs.existsSync(path.join(pkgDir, clean + ext))) return true;
  }
  // 目录入口：走 package.json 的 main 或 index.*
  const dir = path.join(pkgDir, clean);
  if (fs.existsSync(dir) && fs.statSync(dir).isDirectory()) {
    if (fs.existsSync(path.join(dir, 'package.json'))) return true;
    for (const ext of ['.js', '.cjs', '.mjs', '.json', '.d.ts']) {
      if (fs.existsSync(path.join(dir, 'index' + ext))) return true;
    }
  }
  return false;
}

/** 判断一个包是否完好 */
function inspect(pkgDir, name) {
  const pjPath = path.join(pkgDir, 'package.json');
  if (!fs.existsSync(pjPath)) return { name, reason: '缺少 package.json' };

  let meta;
  try {
    meta = JSON.parse(fs.readFileSync(pjPath, 'utf8'));
  } catch (err) {
    return { name, reason: `package.json 无法解析：${err.message}` };
  }

  const entries = declaredEntries(meta);

  // 纯类型包（type-fest / undici-types 之类）：没有 main/exports/bin，只发 .d.ts。
  // 这类包按类型声明文件校验，不能因为找不到 index.js 就判成损坏。
  const typesOnly = !meta.main && !meta.exports && !meta.bin;
  if (typesOnly) {
    const typeEntry = meta.types || meta.typings;
    if (typeEntry && resolvesEntry(pkgDir, typeEntry)) return null;
    if (!typeEntry) {
      const hasAny = fs
        .readdirSync(pkgDir)
        .some((f) => f.endsWith('.d.ts') || f === 'package.json');
      if (hasAny) return null;
    }
    return { name, reason: '纯类型包但找不到类型声明文件', version: meta.version };
  }

  if (entries.some((rel) => resolvesEntry(pkgDir, rel))) return null;

  // 兜底：声明全都对不上时，看构建目录里是否确有产物
  const hasBuildDir = BUILD_DIRS.some((d) => {
    const dir = path.join(pkgDir, d);
    if (!fs.existsSync(dir) || !fs.statSync(dir).isDirectory()) return false;
    return fs.readdirSync(dir).some((f) => /\.(js|cjs|mjs|node|json|d\.ts)$/.test(f));
  });
  if (hasBuildDir) return null;

  const declared = entries.length ? entries.slice(0, 2).join(', ') : '(未声明 main)';
  return { name, reason: `入口文件不存在：${declared}`, version: meta.version };
}

function scan() {
  const broken = [];
  for (const name of fs.readdirSync(NM)) {
    // 跳过点目录：包括 .bin，以及 @electron/.get-xxx 这类下载临时目录
    if (name.startsWith('.')) continue;
    const full = path.join(NM, name);
    if (!fs.statSync(full).isDirectory()) continue;

    if (name.startsWith('@')) {
      for (const sub of fs.readdirSync(full)) {
        if (sub.startsWith('.')) continue;
        const found = inspect(path.join(full, sub), `${name}/${sub}`);
        if (found) broken.push(found);
      }
      continue;
    }
    const found = inspect(full, name);
    if (found) broken.push(found);
  }
  return broken;
}

/** 从镜像取指定版本的 tarball 并解压回 node_modules */
function repair(name, version) {
  if (!version) {
    console.log(`  ! ${name} 的版本未知，跳过（请用 npm 重装）`);
    return false;
  }
  if (!fs.existsSync(SEVEN_ZIP)) {
    console.log('  ! 找不到 7za.exe，无法解压');
    return false;
  }

  const base = name.split('/').pop();
  const url = `${REGISTRY}/${name}/-/${base}-${version}.tgz`;
  const tmp = path.join(ROOT, `.repair-${base}`);
  fs.rmSync(tmp, { recursive: true, force: true });
  fs.mkdirSync(tmp, { recursive: true });
  const tgz = path.join(tmp, 'pkg.tgz');

  try {
    console.log(`  → 下载 ${url}`);
    execFileSync('curl', ['-sL', '-m', '120', '-o', tgz, url], { stdio: 'inherit' });
    if (!fs.existsSync(tgz) || fs.statSync(tgz).size < 100) throw new Error('下载内容为空');

    execFileSync(SEVEN_ZIP, ['x', tgz, `-o${tmp}`, '-y'], { stdio: 'ignore' });
    const tarFile = fs.readdirSync(tmp).find((f) => f.endsWith('.tar'));
    if (!tarFile) throw new Error('未从 tgz 中得到 tar');
    const outDir = path.join(tmp, 'out');
    execFileSync(SEVEN_ZIP, ['x', path.join(tmp, tarFile), `-o${outDir}`, '-y'], { stdio: 'ignore' });

    const pkgRoot = path.join(outDir, 'package');
    if (!fs.existsSync(pkgRoot)) throw new Error('tar 内没有 package/ 目录');

    const target = path.join(NM, name);
    fs.rmSync(target, { recursive: true, force: true });
    fs.renameSync(pkgRoot, target);
    console.log(`  ✓ ${name}@${version} 已修复`);
    return true;
  } catch (err) {
    console.log(`  ✗ ${name} 修复失败：${err.message}`);
    return false;
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function main() {
  if (!fs.existsSync(NM)) {
    console.error('node_modules 不存在，请先运行 npm install');
    process.exit(1);
  }

  console.log('扫描 node_modules 完整性…');
  const broken = scan();

  if (broken.length === 0) {
    console.log('全部完好。');
    return;
  }

  console.log(`\n发现 ${broken.length} 个损坏的包：`);
  broken.forEach((b) => console.log(`  - ${b.name}  (${b.reason})`));

  if (!FIX) {
    console.log('\n加 --fix 参数可自动修复。');
    process.exit(1);
  }

  console.log('\n开始修复…');
  let ok = 0;
  for (const b of broken) {
    if (repair(b.name, b.version)) ok += 1;
  }

  console.log(`\n修复完成：${ok}/${broken.length}`);
  const remaining = scan();
  if (remaining.length) {
    console.log('仍有问题的包：');
    remaining.forEach((r) => console.log(`  - ${r.name}  (${r.reason})`));
    process.exit(1);
  }
  console.log('复检通过，node_modules 已一致。');
}

main();
