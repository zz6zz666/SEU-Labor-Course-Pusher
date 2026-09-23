// 构建后把静态资源复制到 dist（tsc 只处理 .ts）
const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '..');
const src = path.join(root, 'src', 'renderer');
const dest = path.join(root, 'dist', 'renderer');

fs.mkdirSync(dest, { recursive: true });

let copied = 0;
for (const name of fs.readdirSync(src)) {
  if (!/\.(html|css|svg|png|ico)$/i.test(name)) continue;
  fs.copyFileSync(path.join(src, name), path.join(dest, name));
  copied += 1;
  console.log(`  copied ${name} -> dist/renderer/`);
}

console.log(`copy-assets: ${copied} file(s)`);
