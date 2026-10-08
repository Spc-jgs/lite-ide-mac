/**
 * 桩：设置（issue #44）。和 `src-tauri/src/commands/settings.rs` 一一对应。
 *
 * 合并规则（终端字体空 = 跟编辑器、字号 = 基础 + 偏移、夹在 9–28）照 Rust 的 `settings::effective` 抄一份 ——
 * 浏览器里没有 settings.json，基础值就是默认值；`__mockSettingsProblems` 钩子让状态测试 / 调界面时
 * 能造出「设置文件写错了」那种状态（真实现里要去改文件才看得到）。
 */
import { type A, FILES, NOT_MINE } from "./data";

const PATH = "/Users/you/Library/Application Support/com.liteide.app/settings.json";
const MIN = 9;
const MAX = 28;
const BASE = 13;

const ui = { minimap: true, treeCompact: true, treeFollow: true, gitGrouped: false, offset: 0 };
let fresh = true;

const clamp = (n: number) => Math.max(MIN, Math.min(MAX, n));

function view() {
  const family = "JetBrains Mono";
  const problems = (globalThis as { __mockSettingsProblems?: unknown[] }).__mockSettingsProblems ?? [];
  return {
    editorFontFamily: family,
    editorFontSize: clamp(BASE + ui.offset),
    editorFontBase: BASE,
    terminalFontFamily: family,
    terminalFontSize: BASE,
    terminalShell: "",
    minimap: ui.minimap,
    treeCompact: ui.treeCompact,
    treeFollow: ui.treeFollow,
    gitGrouped: ui.gitGrouped,
    problems,
    path: PATH,
  };
}

const KEYS: Record<string, keyof typeof ui> = {
  "editor.minimap": "minimap",
  "tree.compact": "treeCompact",
  "tree.follow": "treeFollow",
  "git.grouped": "gitGrouped",
};

export async function settingsCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    case "settings":
      return view();
    // 和 Rust 的 settings::DEFS 一致（键、类型、默认值）；说明文字简写
    case "settings_schema":
      return [
        { key: "editor.fontFamily", kind: "string", default: '"JetBrains Mono"', doc: "代码字体", min: null, max: null },
        { key: "editor.fontSize", kind: "integer", default: "13", doc: "代码字号", min: MIN, max: MAX },
        { key: "terminal.fontFamily", kind: "string", default: '""', doc: "终端字体，空 = 跟编辑器", min: null, max: null },
        { key: "terminal.fontSize", kind: "integer", default: "13", doc: "终端字号", min: MIN, max: MAX },
        { key: "terminal.shell", kind: "string", default: '""', doc: "终端用的 shell，空 = $SHELL", min: null, max: null },
      ];
    case "set_ui_state": {
      const k = KEYS[String(a.key)];
      if (!k) throw new Error(`不认识的界面状态：${a.key}`);
      (ui as Record<string, unknown>)[k] = Boolean(a.value);
      fresh = false;
      return view();
    }
    case "step_font": {
      ui.offset = a.delta == null ? 0 : clamp(BASE + ui.offset + Number(a.delta)) - BASE;
      fresh = false;
      return view();
    }
    case "adopt_ui_state": {
      if (fresh) {
        fresh = false;
        if (a.minimap != null) ui.minimap = Boolean(a.minimap);
        if (a.treeCompact != null) ui.treeCompact = Boolean(a.treeCompact);
        if (a.treeFollow != null) ui.treeFollow = Boolean(a.treeFollow);
        if (a.gitGrouped != null) ui.gitGrouped = Boolean(a.gitGrouped);
        if (a.editorFont != null) ui.offset = clamp(Number(a.editorFont)) - BASE;
      }
      return view();
    }
    case "open_settings":
      // 同 Rust：不存在才建，已经有了不碰
      if (!(PATH in FILES)) FILES[PATH] = '{\n  // lite-ide 的设置（浏览器桩里的模板只是示意）\n  // "editor.fontSize": 13,\n}\n';
      return PATH;
    default:
      return NOT_MINE;
  }
}
