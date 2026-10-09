import type { Settings } from "../ipc/commands";

/**
 * 设置落到界面上的纯函数（issue #44 第 3 步，docs/SETTINGS.md 7.3）。和 `settings.svelte.ts` 分开放：
 * 那边要碰 `document` 和 IPC，这边要能在裸 node 里测。
 */

/**
 * Rust 那边还没回话（理论上不会：挂载前就 `await` 了）、或者问不到（浏览器里没装桩）时用的。
 * 和 Rust 的 `settings::Settings::default()` + `UiState::default()` 合起来一致
 */
export const DEFAULT_SETTINGS: Settings = {
  editorFontFamily: "JetBrains Mono",
  editorFontSize: 13,
  editorFontBase: 13,
  terminalFontFamily: "JetBrains Mono",
  terminalFontSize: 13,
  terminalShell: "",
  minimap: true,
  treeCompact: true,
  treeFollow: true,
  gitGrouped: false,
  problems: [],
  path: "",
};

/** 内置字体后面跟着的退路。和原来 app.css 的 `--code-font`、Terminal.svelte 的 `TERM_FONT` 一致 */
const FALLBACK = '"JetBrains Mono", "SF Mono", Menlo, ui-monospace, monospace';

/**
 * 用户写的字体名 → CSS 的 font-family。
 *
 * - **总带着退路**：写了一个系统里没有的字体，退回内置的 JetBrains Mono，而不是浏览器默认的等宽字体（又丑、字距还不准）
 * - **整个当一个名字、加引号**：`Fira Code` 里有空格，不加引号也行，但 `"` 和 `\` 会把声明弄坏 —— 转义掉。
 *   写成 `"Fira Code, Menlo"` 想当字体栈用的，会被当成一个叫这个名字的字体、找不到就退回内置的；设置说明里写着「写具体的字体名」
 * - **终端也用它**：xterm 量字符宽度时解析不了 `var(--code-font)`（Terminal.svelte 那段注释），要具体的字符串
 */
export function fontStack(family: string): string {
  const f = family.trim();
  if (!f || f === "JetBrains Mono") return FALLBACK;
  return `"${f.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}", ${FALLBACK}`;
}

/** 要写到 `:root` 上的 CSS 变量。编辑器、日志视图、差异都读它们 —— 改变量不重建编辑器，光标和撤销栈都不动 */
export function cssVars(s: Settings): [string, string][] {
  return [
    ["--code-font", fontStack(s.editorFontFamily)],
    ["--editor-font-size", `${s.editorFontSize}px`],
  ];
}

/** 改界面状态的那几个键（Rust 的 `UiState::set`） */
export type UiKey = "editor.minimap" | "tree.compact" | "tree.follow" | "git.grouped";

/**
 * 迁过一次就记一笔。旧键不删（回退到旧版本时偏好还在，SETTINGS.md 第 10 节），所以要另外记「迁过了」——
 * 不记的话，你哪天删了 `ui-state.json` 想回到默认，下次启动又把几个月前的旧值迁回来
 */
export const LEGACY_MOVED_KEY = "lite-ide.prefs-moved";

/** 交给 `adopt_ui_state` 的旧偏好。没存过的是 null（Rust 那边用默认值） */
export interface LegacyPrefs {
  minimap: boolean | null;
  treeCompact: boolean | null;
  treeFollow: boolean | null;
  gitGrouped: boolean | null;
  editorFont: number | null;
}

/**
 * 读 localStorage 里升级前那 5 个偏好（原来 `prefs.ts` 的 `readPref` / `readNumPref` 写的格式：开关是 "1" / "0"，字号是数字）。
 * 迁过了、或者一个都没存过，返回 null —— 不用去问 Rust。读不出来的那个当没存过，不抛（启动路径上）
 */
export function readLegacyPrefs(get: (key: string) => string | null): LegacyPrefs | null {
  if (get(LEGACY_MOVED_KEY) !== null) return null;
  const bool = (k: string) => {
    const v = get(`lite-ide.${k}`);
    return v === "1" ? true : v === "0" ? false : null;
  };
  const font = (() => {
    const v = get("lite-ide.editorFont");
    const n = v === null ? NaN : Number(v);
    return Number.isFinite(n) && n > 0 ? Math.round(n) : null;
  })();
  const p: LegacyPrefs = {
    minimap: bool("minimap"),
    treeCompact: bool("tree-compact"),
    treeFollow: bool("tree-follow"),
    gitGrouped: bool("git-grouped"),
    editorFont: font,
  };
  return Object.values(p).every((v) => v === null) ? null : p;
}
