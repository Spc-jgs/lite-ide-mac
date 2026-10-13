import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { imeKey } from "../src/lib/shell/ime.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

ok(imeKey({ isComposing: true, keyCode: 13 }), "组字中（Chrome / Firefox 的形状）");
// WebKit 的形状：compositionend 已经过去了，isComposing 是 false —— 只看它的话这一下就漏了
ok(imeKey({ isComposing: false, keyCode: 229 }), "确认候选字的那一下回车（WebKit：isComposing 已经是 false，keyCode 229）");
ok(!imeKey({ isComposing: false, keyCode: 13 }), "一下真回车不算");
ok(!imeKey({ isComposing: false, keyCode: 27 }), "真 Esc 不算");

// ── 处理回车的地方都过了这道（2026-10-12：除了任务列表，还有 12 个文件没过 —— 文件树新建、草稿改名、替换框、分支名……）──
// 按文件判，粗：一个文件里有一处过了，别的处理器漏了也看不出来。它挡的是「新写一个处理回车的组件、压根没想到输入法」这一种，
// 那正是这 13 个文件的来历。没有文本框的放进 EXEMPT，写清理由
const EXEMPT: Record<string, string> = {
  "lib/shell/ime.ts": "就是它自己",
  "lib/shell/ContextMenu.svelte": "菜单里没有文本框，焦点在菜单上时输入法不组字",
  "lib/shell/dialog-focus.ts": "对话框里没有文本框（只有按钮），焦点在按钮上时输入法不组字；对话框外面的键本来就全挡",
};
const root = join(import.meta.dirname, "..", "src");
const miss: string[] = [];
const walk = (dir: string) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) { if (name !== "dev") walk(p); continue; }
    if (!/\.(svelte|ts)$/.test(name)) continue;
    const rel = relative(root, p);
    const src = readFileSync(p, "utf8");
    if (/key\s*[!=]==?\s*"Enter"/.test(src) && !src.includes("imeKey(") && !(rel in EXEMPT)) miss.push(rel);
  }
};
walk(root);
ok(miss.length === 0, `这些文件处理了回车，没问 imeKey（输入法确认候选字的回车会被当成提交）：\n    ${miss.join("\n    ")}`);

console.log(`${fail === 0 ? "✅" : "❌"} 输入法的键：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
