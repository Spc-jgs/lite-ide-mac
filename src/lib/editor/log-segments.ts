/**
 * 草稿里的日志段（M10 ②）：markdown 里连续几行像日志的，按 LogView 那套着色，
 * 段头一条小工具条能「只看 WARN+」。②b 加了 diff 段：`diff --git` / `@@` 起头的连续行
 * 按 DiffView 统一视图那套着色（只着色，没有段头）。两种段共用一个 field，
 * markdown-live 对两种都让路。
 *
 * # 为什么在前端算、不走 Rust
 *
 * 方案里写的是「判定放 Rust 侧（`level::detect` 在那儿）」。改主意的原因：
 * LogView 本来就有一套前端解析器（`logview/parse.ts`，按格式把一行切成
 * 时间戳 / 级别 / 线程 / logger / 正文几段），可见行都是它着的色 —— 用同一套，
 * 「同一段在 LogPane 和草稿里颜色一致」就不是对比出来的，是同一份代码。
 * 走 Rust 只能拿到级别，拿不到分段；而且每敲一个字往 IPC 送一遍整份草稿，
 * 粘了一段 1MB 的日志就是 1MB 一趟。
 *
 * # 两层
 *
 * - **段**（`findLogSegments`）：文档一变就重算，几万行也只是几万次正则。
 * - **着色**：只对视口里的行 `parse()`，和 LogView 一样 —— 段可能上万行，
 *   全切一遍是白干。视口一滚就重算。
 *
 * # 正文永远是纯文本
 *
 * 这里全是 decoration：line class 给级别色，mark 给分段色，block widget 当段头。
 * 文件里一个私有标记都不写 —— 用 Sublime 打开这份草稿要一样能读。
 *
 * # 「只看 WARN+」= 折叠
 *
 * 过滤就是把不匹配的行折起来（`foldEffect`），正文一个字不动，和 gutter 上的折叠
 * 是同一个东西 —— 折错了人自己点开就行。堆栈续行跟着它上一条日志走：那条留它就留。
 * 段头上的开关状态只在内存里：它是显示状态，不是文档状态。
 */
import { foldEffect, unfoldEffect, foldedRanges } from "@codemirror/language";
import { StateEffect, StateField, type EditorState, type Extension, type Range, type Text } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import { findLogSegments, formatOfLine, isStackLine, parse, FORMAT_LABEL, type LogSegment, type Level } from "../logview/parse";
import { findDiffSegments, diffLineKind, type DiffSegment, type LineKind } from "../git/diff";
import { makeLogDiffBlocks } from "./md-blocks";

/**
 * 给 markdown 解析器的块扩展：日志段 / diff 段在语法树里是不做 inline 解析的叶子。
 * 判据和下面 `scan()` 用的是同一批函数 —— 为什么要有它、为什么判据在这儿绑，见 md-blocks.ts 头上。
 */
export const logDiffBlocks = makeLogDiffBlocks({
  logLine: (t) => formatOfLine(t.trim()) !== null,
  stackLine: isStackLine,
  diffKind: diffLineKind,
});

/** 段头。按钮通过 `toggle(view, seg)` 走 effect，widget 自己不存状态 */
class HeadWidget extends WidgetType {
  constructor(
    readonly seg: LogSegment,
    readonly filtered: boolean,
  ) {
    super();
  }
  override eq(o: HeadWidget): boolean {
    return o.seg.from === this.seg.from && o.seg.to === this.seg.to && o.seg.count === this.seg.count && o.filtered === this.filtered;
  }
  override toDOM(view: EditorView): HTMLElement {
    const el = document.createElement("div");
    el.className = "cm-log-head";
    const label = document.createElement("span");
    label.className = "cm-log-head-label";
    label.textContent = `日志 · ${FORMAT_LABEL[this.seg.fmt]} · ${this.seg.count} 行`;
    const btn = document.createElement("button");
    btn.className = "cm-log-head-btn" + (this.filtered ? " on" : "");
    btn.type = "button";
    btn.textContent = this.filtered ? "显示全部" : "只看 WARN+";
    btn.onmousedown = (e) => e.preventDefault(); // 别把编辑器的焦点抢走
    btn.onclick = () => toggle(view, this.seg);
    el.append(label, btn);
    return el;
  }
  override ignoreEvent(): boolean {
    return true;
  }
}

const lineDeco: Record<Exclude<Level, null> | "none", Decoration> = {
  error: Decoration.line({ class: "cm-log cm-log-error" }),
  warn: Decoration.line({ class: "cm-log cm-log-warn" }),
  info: Decoration.line({ class: "cm-log cm-log-info" }),
  debug: Decoration.line({ class: "cm-log cm-log-debug" }),
  trace: Decoration.line({ class: "cm-log cm-log-trace" }),
  none: Decoration.line({ class: "cm-log" }),
};
const stackDeco = Decoration.line({ class: "cm-log cm-log-stack" });
const diffDeco: Record<LineKind, Decoration> = {
  add: Decoration.line({ class: "cm-log cm-diff-add" }),
  del: Decoration.line({ class: "cm-log cm-diff-del" }),
  ctx: Decoration.line({ class: "cm-log cm-diff-ctx" }),
  hunk: Decoration.line({ class: "cm-log cm-diff-hunk" }),
  meta: Decoration.line({ class: "cm-log cm-diff-meta" }),
};
/** 行首那个 + / - 单独一个 mark，和 DiffView 里 `.sign` 一样只染那一格 */
const signDeco = { add: Decoration.mark({ class: "cm-diff-sign-add" }), del: Decoration.mark({ class: "cm-diff-sign-del" }) };
const partDeco: Record<string, Decoration> = {};
const partMark = (cls: string) => (partDeco[cls] ??= Decoration.mark({ class: `cm-log-${cls}` }));

/** 过滤线：WARN 及以上留下 */
const KEEP: Set<Level> = new Set(["error", "warn"]);

/** 段头上那个开关按了：`from` 是段的首行（0-based） */
const toggleEffect = StateEffect.define<number>();

interface SegState {
  segs: LogSegment[];
  /** diff 段（②b）：只着色，没有段头 —— 一段 diff 没有「只看什么」可选 */
  diffs: DiffSegment[];
  /** 开了「只看 WARN+」的段，按首行行号记 */
  filtered: Set<number>;
  /** 段头（块 widget）。**块装饰只能从 StateField 出**，ViewPlugin 里给会直接抛 */
  heads: DecorationSet;
}

function scan(doc: Text): { segs: LogSegment[]; diffs: DiffSegment[] } {
  // 上限：草稿不该是日志文件。真粘了十万行，段不认了（那时该把它存成文件用日志视图）
  if (doc.lines > 50_000) return { segs: [], diffs: [] };
  const lines: string[] = [];
  for (let n = 1; n <= doc.lines; n++) lines.push(doc.line(n).text);
  return { segs: findLogSegments(lines), diffs: findDiffSegments(lines) };
}

function heads(doc: Text, segs: LogSegment[], filtered: Set<number>): DecorationSet {
  return Decoration.set(
    segs.map((seg) =>
      Decoration.widget({
        widget: new HeadWidget(seg, filtered.has(seg.from)),
        block: true,
        side: -1,
      }).range(doc.line(seg.from + 1).from),
    ),
  );
}

/** 段和过滤开关。文档一变重扫；开关状态按「改动前后同一段」对过去，对不上的丢掉 */
export const logSegmentsField = StateField.define<SegState>({
  create(state) {
    const { segs, diffs } = scan(state.doc);
    return { segs, diffs, filtered: new Set(), heads: heads(state.doc, segs, new Set()) };
  },
  update(v, tr) {
    let { segs, diffs, filtered } = v;
    let dirty = false;
    if (tr.docChanged) {
      const before = segs;
      ({ segs, diffs } = scan(tr.state.doc));
      const next = new Set<number>();
      for (const f of filtered) {
        const seg = before.find((s) => s.from === f);
        if (!seg) continue;
        const ln = tr.state.doc.lineAt(tr.changes.mapPos(tr.startState.doc.line(seg.from + 1).from)).number - 1;
        const now = segs.find((s) => ln >= s.from && ln <= s.to);
        if (now) next.add(now.from);
      }
      filtered = next;
      dirty = true;
    }
    for (const e of tr.effects) {
      if (e.is(toggleEffect)) {
        filtered = new Set(filtered);
        if (filtered.has(e.value)) filtered.delete(e.value);
        else filtered.add(e.value);
        dirty = true;
      }
    }
    return dirty ? { segs, diffs, filtered, heads: heads(tr.state.doc, segs, filtered) } : v;
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.heads),
});

/** 「只看 WARN+」：折掉不匹配的连续行；再点一次把这一段里折的全展开 */
function toggle(view: EditorView, seg: LogSegment) {
  const doc = view.state.doc;
  const on = !view.state.field(logSegmentsField).filtered.has(seg.from);
  const effects: StateEffect<unknown>[] = [toggleEffect.of(seg.from)];
  if (on) {
    let keepPrev = true;
    /** 这一串要藏的行从哪行起（1-based）；-1 = 没在藏 */
    let runStart = -1;
    const flush = (endLine: number) => {
      if (runStart < 0) return;
      // 折的范围：上一行行尾到这串末行行尾 —— 占位符挂在上一行末。
      // 要藏的就是段的第一行时没有「上一行」可挂（段头 widget 在它行首，折到前一行
      // 会把 widget 一起吞掉），改从它自己的行首折起：这一行只剩一个占位符
      const from = runStart === seg.from + 1 ? doc.line(runStart).from : doc.line(runStart - 1).to;
      const to = doc.line(endLine).to;
      if (from < to) effects.push(foldEffect.of({ from, to }));
      runStart = -1;
    };
    for (let n = seg.from + 1; n <= seg.to + 1; n++) {
      const text = doc.line(n).text;
      const s = parse(text, seg.fmt);
      const follows = s.stack || text.trim() === "";
      const keep: boolean = follows ? keepPrev : KEEP.has(s.lvl);
      if (!follows) keepPrev = keep;
      if (!keep) {
        if (runStart < 0) runStart = n;
      } else flush(n - 1);
    }
    flush(seg.to + 1);
  } else {
    const a = doc.line(seg.from + 1).from;
    const b = doc.line(seg.to + 1).to;
    foldedRanges(view.state).between(a, b, (from, to) => {
      effects.push(unfoldEffect.of({ from, to }));
    });
  }
  view.dispatch({ effects });
}

/** 视口里的行：级别色 + 分段色。只算看得见的，和 LogView 一样 */
const plugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(readonly view: EditorView) {
      this.decorations = this.build();
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged) this.decorations = this.build();
    }
    build(): DecorationSet {
      const { state, visibleRanges } = this.view;
      const doc = state.doc;
      const decos: Range<Decoration>[] = [];
      for (const seg of state.field(logSegmentsField).segs) {
        for (const { from, to } of visibleRanges) {
          const a = Math.max(doc.lineAt(from).number, seg.from + 1);
          const b = Math.min(doc.lineAt(to).number, seg.to + 1);
          for (let n = a; n <= b; n++) {
            const ln = doc.line(n);
            const s = parse(ln.text, seg.fmt);
            decos.push((s.stack ? stackDeco : lineDeco[s.lvl ?? "none"]).range(ln.from));
            if (s.stack) continue;
            // 各段的位置：解析器吐的段拼起来通常就是整行；对不上时逐段 indexOf 找，找不到的跳过
            let cur = 0;
            for (const p of s.parts) {
              if (!p.text) continue;
              const i = ln.text.indexOf(p.text, cur);
              if (i < 0) continue;
              if (p.cls !== "msg") decos.push(partMark(p.cls).range(ln.from + i, ln.from + i + p.text.length));
              cur = i + p.text.length;
            }
          }
        }
      }
      for (const d of state.field(logSegmentsField).diffs) {
        for (const { from, to } of visibleRanges) {
          const a = Math.max(doc.lineAt(from).number, d.from + 1);
          const b = Math.min(doc.lineAt(to).number, d.to + 1);
          for (let n = a; n <= b; n++) {
            const ln = doc.line(n);
            const k = d.kinds[n - 1 - d.from];
            decos.push(diffDeco[k].range(ln.from));
            if ((k === "add" || k === "del") && ln.length > 0) decos.push(signDeco[k].range(ln.from, ln.from + 1));
          }
        }
      }
      return Decoration.set(decos, true);
    }
  },
  { decorations: (v) => v.decorations },
);

/**
 * `[from, to]` 是不是整个落在同一个日志段里。markdown-live 用它跳过这些节点 ——
 * 日志里的 `[thread]` 不是链接。**要两头都查**：Document / Paragraph 这种大节点
 * 从日志行开始、却包着后面的正文，只看起点就会把整篇的渲染都跳掉。
 */
export function inLogSegment(state: EditorState, from: number, to: number): boolean {
  const f = state.field(logSegmentsField, false);
  if (!f || (f.segs.length === 0 && f.diffs.length === 0)) return false;
  const a = state.doc.lineAt(from).number - 1;
  const b = state.doc.lineAt(to).number - 1;
  // diff 段也算：`- 删掉的行` 在 markdown 眼里是列表项，`+++ b/x` 里的 `+` 也是
  return f.segs.some((s) => a >= s.from && b <= s.to) || f.diffs.some((s) => a >= s.from && b <= s.to);
}

/*
 * 颜色照 LogView.svelte 那张表：级别色只染级别段，ERROR 例外整行扎眼；
 * 堆栈续行压暗。变量都是 app.css 的 --lvl-*，两边改一处都跟着变。
 */
const theme = EditorView.baseTheme({
  /*
   * 选择器都带 `.cm-log` 前缀抬一级特异性：语法高亮（`ideaDarkHighlight`）也在这些
   * 字上挂了 class —— markdown 把 `[http-nio-exec-4]` 认成链接，蓝色带下划线；
   * 同级的两个单类选择器谁后加载谁赢，靠顺序是靠不住的。`*` 那条把整行里所有
   * 语法高亮的色都压回本行的色，分段色再叠上去。
   */
  ".cm-log": { fontFamily: "var(--code-font)" },
  ".cm-log *": { color: "inherit", textDecoration: "none", fontStyle: "normal", fontWeight: "inherit" },
  ".cm-log .cm-log-ts, .cm-log .cm-log-thread, .cm-log .cm-log-dim": { color: "var(--text-faint)" },
  ".cm-log .cm-log-logger, .cm-log .cm-log-meta": { color: "var(--text-dim)" },
  ".cm-log .cm-log-key": { color: "var(--lvl-warn)" },
  ".cm-log .cm-log-level": { fontWeight: "600" },
  ".cm-log-error": { color: "var(--lvl-error)" },
  ".cm-log-error .cm-log-level": { color: "var(--lvl-error)" },
  ".cm-log-warn .cm-log-level": { color: "var(--lvl-warn)" },
  ".cm-log-info .cm-log-level": { color: "var(--lvl-info)" },
  ".cm-log-debug .cm-log-level, .cm-log-trace .cm-log-level": { color: "var(--lvl-debug)" },
  ".cm-log-debug, .cm-log-trace": { color: "var(--text-dim)" },
  ".cm-log-stack": { color: "var(--text-dim)", background: "rgba(255, 255, 255, 0.02)" },
  // diff 段：照 DiffView 统一视图那张表（.uni .row.add / .del / .ctx / .hunk / .meta）
  ".cm-diff-add": { background: "var(--diff-add-bg)" },
  ".cm-diff-del": { background: "var(--diff-del-bg)" },
  ".cm-diff-ctx": { color: "var(--text-dim)" },
  ".cm-diff-hunk": { background: "var(--hover)", color: "var(--text-faint)", fontStyle: "italic" },
  ".cm-diff-meta": { color: "var(--text-faint)" },
  ".cm-log .cm-diff-sign-add": { color: "var(--diff-add-fg)", fontWeight: "600" },
  ".cm-log .cm-diff-sign-del": { color: "var(--diff-del-fg)", fontWeight: "600" },
  // 段头：和 Git 日志过滤条一个尺度（岛内工具栏 34 太高，这是行内的东西，24 够了）
  ".cm-log-head": {
    display: "flex",
    alignItems: "center",
    gap: "8px",
    height: "24px",
    margin: "6px 0 2px",
    padding: "0 6px",
    borderRadius: "var(--r-sm)",
    background: "var(--hover)",
    fontFamily: "var(--ui-font)",
    fontSize: "11px",
    color: "var(--text-faint)",
    userSelect: "none",
  },
  ".cm-log-head-label": { flex: "1" },
  ".cm-log-head-btn": {
    height: "18px",
    padding: "0 7px",
    border: "1px solid var(--border)",
    borderRadius: "var(--r-sm)",
    background: "transparent",
    color: "var(--text-dim)",
    fontFamily: "var(--ui-font)",
    fontSize: "10.5px",
    cursor: "default",
  },
  ".cm-log-head-btn:hover": { background: "var(--hover)", color: "var(--text)" },
  ".cm-log-head-btn.on": { background: "var(--accent-sel)", color: "var(--text)", borderColor: "transparent" },
});

export const logSegments: Extension = [logSegmentsField, plugin, theme];
