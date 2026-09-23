/**
 * 差异视图的语法着色。
 *
 * # 为什么要有
 *
 * 2026-09-22 之前差异视图是纯白的等宽字：一份 Java 的 diff 看着像 .txt。IDEA / VS Code /
 * GitHub 的 diff 都是着了色的 —— 眼睛靠颜色分「这是关键字、这是字符串」，没有颜色，
 * 每一行都得从头读。
 *
 * # 怎么做
 *
 * 复用编辑器那套语言包和配色，不另起一套：`loadLang(langOf(path))` 拿到 CM6 的语言扩展，
 * 建一个**不挂到 DOM 的** `EditorState` 让它解析，`highlightTree` 把语法树扫成
 * `[from, to, class]` 区间，再按行切开。配色来自 `HIGHLIGHT_SPEC`（编辑器和缩略图也读它），
 * 这里把每条映射成一个类名 `dh0…dhN`，CSS 由 `ensureCss` 按同一张表生成一次注入 ——
 * 色值仍然只有一份。
 *
 * **按块解析，不按整份 diff 解析。** diff 里只有 hunk 和几行上下文，块与块之间是断的；
 * 把所有块拼成一段喂给解析器，块的接缝处会产生错误恢复的连锁反应，越往后越乱。
 * 一块一块解析，每块的旧文本、新文本各一遍：解析器对「没有 class 包着的几行方法体」
 * 已经够宽容，关键字 / 字符串 / 注释 / 数字都对，偶尔一个类型名当成变量名，无伤。
 *
 * 解析有上限（`ensureSyntaxTree` 300ms）：diff 最多 1MB（Rust 侧掐的），
 * 超时就拿部分树，没扫到的行是平的，不卡界面。
 */
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import { highlightTree, tagHighlighter } from "@lezer/highlight";
import { HIGHLIGHT_SPEC } from "../editor/theme-idea-dark";
import { loadLang } from "../editor/langs-load";
import { langOf } from "../editor/langs";
import type { DiffLine } from "./diff";

/** 一行里的一段：`cls` 空串 = 没有语法信息，直接印 */
export interface Tok {
  t: string;
  cls: string;
}

const highlighter = tagHighlighter(HIGHLIGHT_SPEC.map((r, i) => ({ tag: r.tag, class: `dh${i}` })));

let cssDone = false;
function ensureCss() {
  if (cssDone || typeof document === "undefined") return;
  cssDone = true;
  const rules = HIGHLIGHT_SPEC.map((r, i) => {
    const decl = [`color:${r.color}`];
    if (r.fontStyle) decl.push(`font-style:${r.fontStyle}`);
    if (r.fontWeight) decl.push(`font-weight:${r.fontWeight}`);
    if (r.textDecoration) decl.push(`text-decoration:${r.textDecoration}`);
    return `.dh${i}{${decl.join(";")}}`;
  });
  const el = document.createElement("style");
  el.id = "diff-highlight";
  el.textContent = rules.join("\n");
  document.head.appendChild(el);
}

/**
 * 给一份 diff 的每一行算语法段。返回 null = 这种文件没有语言包（纯文本 / 没认出来），
 * 调用方按平的画。上下文行在旧新两侧都出现，取先算到的那份。
 */
export async function highlightDiff(lines: DiffLine[], path: string): Promise<Map<DiffLine, Tok[]> | null> {
  const id = langOf(path);
  if (!id) return null;
  const ext = await loadLang(id);
  if (!ext) return null;
  ensureCss();

  const out = new Map<DiffLine, Tok[]>();
  let oldSide: DiffLine[] = [];
  let newSide: DiffLine[] = [];
  const flush = () => {
    paint(oldSide);
    paint(newSide);
    oldSide = [];
    newSide = [];
  };
  for (const l of lines) {
    if (l.kind === "hunk" || l.kind === "meta") {
      flush();
      continue;
    }
    if (l.kind !== "add") oldSide.push(l);
    if (l.kind !== "del") newSide.push(l);
  }
  flush();
  return out;

  function paint(ls: DiffLine[]) {
    if (ls.length === 0) return;
    const text = ls.map((l) => l.text).join("\n");
    const state = EditorState.create({ doc: text, extensions: ext! });
    const tree = ensureSyntaxTree(state, text.length, 300) ?? syntaxTree(state);
    const ranges: [number, number, string][] = [];
    highlightTree(tree, highlighter, (from, to, cls) => ranges.push([from, to, cls]));

    let off = 0;
    let ri = 0;
    for (const l of ls) {
      const start = off;
      const end = off + l.text.length;
      const toks: Tok[] = [];
      let pos = start;
      // 跳过整个落在这行之前的区间
      while (ri < ranges.length && ranges[ri][1] <= start) ri++;
      for (let j = ri; j < ranges.length && ranges[j][0] < end; j++) {
        const [f, t, c] = ranges[j];
        const a = Math.max(f, start);
        const b = Math.min(t, end);
        if (a > pos) toks.push({ t: text.slice(pos, a), cls: "" });
        if (b > a) toks.push({ t: text.slice(a, b), cls: c });
        pos = Math.max(pos, b);
      }
      if (pos < end) toks.push({ t: text.slice(pos, end), cls: "" });
      if (!out.has(l)) out.set(l, toks);
      off = end + 1;
    }
  }
}
