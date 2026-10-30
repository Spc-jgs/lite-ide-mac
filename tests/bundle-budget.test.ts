import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inspectBundle, assessBundle, LIMITS } from '../scripts/bundle-budget.mjs';

const root = mkdtempSync(join(tmpdir(), 'lite-bundle-budget-'));
mkdirSync(join(root, '.vite'));
const manifest = {
  'index.html': { file: 'entry.js', isEntry: true, imports: ['shared', 'shared'], dynamicImports: ['editor'], css: ['style.css'] },
  shared: { file: 'shared.js', imports: ['index.html'] },
  editor: { file: 'editor.js', isDynamicEntry: true },
};
function fixture(entryBytes = LIMITS.entryHard, sharedBytes = 1110) {
  writeFileSync(join(root, '.vite/manifest.json'), JSON.stringify(manifest));
  writeFileSync(join(root, 'entry.js'), 'x'.repeat(entryBytes));
  writeFileSync(join(root, 'shared.js'), 'y'.repeat(sharedBytes));
  writeFileSync(join(root, 'editor.js'), 'z'.repeat(400_000));
  writeFileSync(join(root, 'style.css'), 'body{}');
}
try {
  fixture();
  let report = inspectBundle(root);
  assert.equal(report.initialJsBytes, LIMITS.entryHard + 1110, '静态依赖去重并处理环');
  assert.equal(report.staticDependencyBytes, 1110);
  assert.equal(report.initialCssBytes, 6);
  assert.equal(report.allJsBytes, LIMITS.entryHard + 1110 + 400_000, '动态块仍进总量，不进入启动静态闭包');
  assert.equal(assessBundle(report).status, 'WARN', '刚好到硬线允许，告警仍保留');
  fixture(LIMITS.entryHard + 1);
  assert.equal(assessBundle(inspectBundle(root)).status, 'FAIL', '超过一字节就失败，不能先除 1024 取整');
  fixture(LIMITS.entryWarn);
  assert.equal(assessBundle(inspectBundle(root)).status, 'PASS');
  fixture(LIMITS.entryWarn + 1);
  assert.equal(assessBundle(inspectBundle(root)).status, 'WARN');
  fixture(80_000, 80_000);
  assert.equal(assessBundle(inspectBundle(root)).status, 'PASS', '同量代码移到共享块不应受罚');
  fixture(1000, LIMITS.initialHard - 1000);
  assert.equal(assessBundle(inspectBundle(root)).status, 'PASS', '静态闭包刚好到线允许');
  fixture(1000, LIMITS.initialHard - 1000 + 1);
  assert.equal(assessBundle(inspectBundle(root)).status, 'FAIL', '小入口不能掩盖超预算静态 vendor');
  fixture();
  const broken = { ...manifest, 'index.html': { ...manifest['index.html'], imports: ['missing'] } };
  writeFileSync(join(root, '.vite/manifest.json'), JSON.stringify(broken));
  assert.throws(() => inspectBundle(root), /缺静态依赖/);
  writeFileSync(join(root, '.vite/manifest.json'), JSON.stringify({ ...manifest, second: { file: 'editor.js', isEntry: true } }));
  assert.throws(() => inspectBundle(root), /一个 HTML/);
  writeFileSync(join(root, '.vite/manifest.json'), '{}');
  assert.throws(() => inspectBundle(root), /没有 JavaScript 入口/);
  console.log('✅ bundle budget：字节边界、静态依赖、动态排除、去重与缺失检查通过');
} finally {
  rmSync(root, { recursive: true });
}
