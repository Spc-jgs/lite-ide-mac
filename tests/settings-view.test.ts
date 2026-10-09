import { DEFAULT_SETTINGS, LEGACY_MOVED_KEY, cssVars, fontStack, readLegacyPrefs } from "../src/lib/state/settings-view.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);

// ── 字体栈 ──
const FALLBACK = '"JetBrains Mono", "SF Mono", Menlo, ui-monospace, monospace';
eq(fontStack("JetBrains Mono"), FALLBACK, "默认字体：就是原来那串，不重复");
eq(fontStack("  "), FALLBACK, "空的：退回默认");
eq(fontStack("Fira Code"), `"Fira Code", ${FALLBACK}`, "写了别的：加引号放最前，后面带着退路（系统里没有就退回内置的）");
eq(fontStack('Weird "Font" \\ X'), `"Weird \\"Font\\" \\\\ X", ${FALLBACK}`, "引号和反斜杠转义掉，不然整条 font-family 声明作废");

// ── CSS 变量 ──
eq(cssVars({ ...DEFAULT_SETTINGS, editorFontFamily: "Menlo", editorFontSize: 16 }), [
  ["--code-font", `"Menlo", ${FALLBACK}`],
  ["--editor-font-size", "16px"],
], "字体和实际字号（基础 + 偏移已经在 Rust 那边合好）");

// ── 升级前的 5 个偏好 ──
const from = (m: Record<string, string>) => readLegacyPrefs((k) => m[k] ?? null);
eq(from({}), null, "一个都没存过：不用去问 Rust");
eq(
  from({ "lite-ide.minimap": "0", "lite-ide.tree-follow": "1", "lite-ide.editorFont": "16" }),
  { minimap: false, treeCompact: null, treeFollow: true, gitGrouped: null, editorFont: 16 },
  "旧格式：开关是 1 / 0，字号是数字；没存过的是 null",
);
eq(from({ "lite-ide.minimap": "0", [LEGACY_MOVED_KEY]: "1" }), null, "迁过了：不再交（删了 ui-state.json 想回到默认时，旧值不能又迁回来）");
eq(
  from({ "lite-ide.minimap": "yes", "lite-ide.editorFont": "abc", "lite-ide.git-grouped": "1" }),
  { minimap: null, treeCompact: null, treeFollow: null, gitGrouped: true, editorFont: null },
  "读不懂的那个当没存过，别的照收",
);
eq(from({ "lite-ide.editorFont": "-3" }), null, "字号不是正数：当没存过（全都没有就是 null）");

console.log(`${fail === 0 ? "✅" : "❌"} 设置落到界面：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
