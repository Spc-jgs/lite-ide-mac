/** 构建产物门禁：入口与它的静态依赖分别计数，动态 import 留给功能场景测量。 */
import { readFileSync, statSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { gzipSync } from 'node:zlib';

// 160/152 KiB 延续原入口决策；总静态闭包留 2 KiB 构建支持余量（当前 helper 1110 B）。
// 卡总量而非共享块大小：同量移到共享块应允许，不能以切块绕过。依据见 docs/BUNDLE-GUARD.md。
export const LIMITS = Object.freeze({ entryWarn: 152 * 1024, entryHard: 160 * 1024, initialHard: 162 * 1024 });

export function inspectBundle(directory) {
  const root = resolve(directory);
  const manifest = JSON.parse(readFileSync(resolve(root, '.vite/manifest.json'), 'utf8'));
  const entries = Object.keys(manifest).filter(k => manifest[k].isEntry && manifest[k].file.endsWith('.js'));
  if (!entries.length) throw new Error('manifest 没有 JavaScript 入口；先构建 --manifest，不能把缺失当 0');
  if (entries.length !== 1) throw new Error('本项目应为一个 HTML/JavaScript 入口；新增入口需重审启动口径');
  const seen = new Set();
  function visit(key) {
    if (seen.has(key)) return;
    const chunk = manifest[key];
    if (!chunk) throw new Error(`manifest 缺静态依赖：${key}`);
    seen.add(key);
    for (const dependency of chunk.imports ?? []) visit(dependency);
  }
  entries.forEach(visit);
  const files = [...new Set([...seen].map(k => manifest[k].file).filter(f => f.endsWith('.js')))];
  const entryFiles = new Set(entries.map(k => manifest[k].file));
  const measure = file => {
    const bytes = readFileSync(resolve(root, file));
    return { file, bytes: bytes.length, gzipBytes: gzipSync(bytes).length };
  };
  const initial = files.map(measure);
  const entryRows = initial.filter(x => entryFiles.has(x.file));
  const dependencies = initial.filter(x => !entryFiles.has(x.file));
  const cssFiles = [...new Set([...seen].flatMap(k => manifest[k].css ?? []))];
  const allJs = [...new Set(Object.values(manifest).map(c => c.file).filter(f => f.endsWith('.js')))];
  return {
    schema: 1,
    entries: entryRows,
    staticDependencies: dependencies,
    initialJsBytes: initial.reduce((n, x) => n + x.bytes, 0),
    initialJsGzipBytes: initial.reduce((n, x) => n + x.gzipBytes, 0),
    staticDependencyBytes: dependencies.reduce((n, x) => n + x.bytes, 0),
    initialCssBytes: cssFiles.reduce((n, f) => n + statSync(resolve(root, f)).size, 0),
    allJsBytes: allJs.reduce((n, f) => n + statSync(resolve(root, f)).size, 0),
    allJsChunks: allJs.length,
  };
}

export function assessBundle(report) {
  const findings = [];
  for (const entry of report.entries) {
    if (entry.bytes > LIMITS.entryHard) findings.push({ level: 'error', metric: 'entry', file: entry.file, bytes: entry.bytes, limit: LIMITS.entryHard });
    else if (entry.bytes > LIMITS.entryWarn) findings.push({ level: 'warning', metric: 'entry', file: entry.file, bytes: entry.bytes, limit: LIMITS.entryWarn });
  }
  if (report.initialJsBytes > LIMITS.initialHard) {
    findings.push({ level: 'error', metric: 'initial-static-js', bytes: report.initialJsBytes, limit: LIMITS.initialHard });
  }
  return { ...report, limits: LIMITS, findings, status: findings.some(x => x.level === 'error') ? 'FAIL' : findings.length ? 'WARN' : 'PASS' };
}

if (import.meta.url === pathToFileURL(resolve(process.argv[1] ?? '')).href) {
  try {
    const report = assessBundle(inspectBundle(process.argv[2] ?? 'dist'));
    if (process.argv[3]) writeFileSync(process.argv[3], JSON.stringify(report, null, 2) + '\n');
    for (const e of report.entries) console.log(`入口 ${e.file}: ${e.bytes} B / ${(e.bytes / 1024).toFixed(2)} KiB`);
    console.log(`入口静态 JS 合计 ${report.initialJsBytes} B，额外静态依赖 ${report.staticDependencyBytes} B，静态 CSS ${report.initialCssBytes} B`);
    console.log(`gzip 参考 ${report.initialJsGzipBytes} B；全部 JS ${report.allJsBytes} B / ${report.allJsChunks} 块（不是启动执行量）`);
    for (const f of report.findings) console.log(`::${f.level}::${f.metric}: ${f.bytes} B > ${f.limit} B；核对依赖边界和启动实测，见 docs/BUNDLE-GUARD.md`);
    if (report.status === 'FAIL') process.exitCode = 1;
  } catch (error) {
    console.error(`::error::bundle budget: ${error.message}`);
    process.exitCode = 1;
  }
}
