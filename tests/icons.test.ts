/**
 * 图标表（`Icon.svelte` 的 `ICON_NAMES`）里的每一个都得有一个**真画了东西**的 SVG 文件。
 *
 * 2026-10-12 走查时发现 `tag.svg` 从 09-24 起是 0 字节：分支浮层里「检出标签或提交…」前面一直空着，画廊里一排图标中间空一格也没人看出来 ——
 * 浅灰的小图标少了一个不显眼。读源码里那张表，不 import 组件（裸 node 跑不了 .svelte）
 */
import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const src = readFileSync(join(root, "src/lib/shell/Icon.svelte"), "utf8");
const table = src.slice(src.indexOf("ICON_NAMES = ["), src.indexOf("] as const"));
const names = [...table.matchAll(/"([a-z0-9-]+)"/g)].map((m) => m[1]);
ok(names.length > 30, `认出了图标表（${names.length} 个）`);
for (const n of names) {
  const p = join(root, "public/icons", `${n}.svg`);
  if (!existsSync(p)) { ok(false, `${n}.svg 不存在`); continue; }
  const svg = readFileSync(p, "utf8");
  ok(/<svg[\s>]/.test(svg) && /<(path|circle|rect|polygon|ellipse|line)\b/.test(svg), `${n}.svg 里得真画了东西（${svg.length} 字节）`);
}

console.log(`${fail === 0 ? "✅" : "❌"} 图标文件：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
