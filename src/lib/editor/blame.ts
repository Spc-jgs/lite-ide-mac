import { StateEffect, StateField, RangeSet, RangeSetBuilder, Compartment } from "@codemirror/state";
import { EditorView, gutter, GutterMarker } from "@codemirror/view";
import type { BlameHunk } from "../ipc/commands";

/**
 * 注解（blame）gutter（issue #33 ⑭）：每段的第一行写「作者 · 多久之前」，其余行空着，
 * 段与段之间靠上边一条细线分开 —— IDEA Annotate 的样子。点一下开那次提交的差异。
 *
 * 数据由 `Editor.svelte` 用 `setBlame` 喂；打字时旧段落按 `tr.changes` 平移
 * （同 `changemarks.ts`），新加的行落在段中间就当它属于那一段 —— 严格说它是
 * 「未提交」，但 blame 要重跑才知道，保存之后会重跑。
 *
 * 装在一个 Compartment 里：关掉时把 gutter 整个拿走，不是留一列空的。
 */
export const setBlame = StateEffect.define<BlameHunk[] | null>();

class BlameMarker extends GutterMarker {
  constructor(
    readonly hunk: BlameHunk,
    readonly first: boolean,
    readonly onPick: (h: BlameHunk) => void,
  ) {
    super();
  }
  override eq(other: BlameMarker) {
    return other.hunk === this.hunk && other.first === this.first;
  }
  override toDOM() {
    const el = document.createElement("div");
    const h = this.hunk;
    const uncommitted = /^0+$/.test(h.sha);
    el.className = `cm-blame${this.first ? " first" : ""}${uncommitted ? " wip" : ""}`;
    if (this.first) {
      el.textContent = uncommitted ? "未提交" : `${h.author} · ${ago(h.time)}`;
      el.title = uncommitted ? "工作区里还没提交的改动" : `${h.short} · ${h.summary}\n${new Date(h.time * 1000).toLocaleString()}`;
      if (!uncommitted) {
        el.onclick = () => this.onPick(h);
        el.style.cursor = "pointer";
      }
    }
    return el;
  }
}

/** 多久之前。粗一点没关系：注解看的是「谁、大概什么时候」，精确时间在 tooltip 里 */
export function ago(unix: number, now = Date.now() / 1000): string {
  const s = Math.max(0, now - unix);
  if (s < 60) return "刚刚";
  const m = s / 60;
  if (m < 60) return `${Math.floor(m)} 分钟前`;
  const h = m / 60;
  if (h < 24) return `${Math.floor(h)} 小时前`;
  const d = h / 24;
  if (d < 30) return `${Math.floor(d)} 天前`;
  const mo = d / 30;
  if (mo < 12) return `${Math.floor(mo)} 个月前`;
  return `${Math.floor(d / 365)} 年前`;
}

export function blameGutter(onPick: (h: BlameHunk) => void) {
  const field = StateField.define<RangeSet<GutterMarker>>({
    create: () => RangeSet.empty,
    update(set, tr) {
      for (const e of tr.effects) {
        if (e.is(setBlame)) return build(e.value ?? [], tr.state.doc);
      }
      return tr.docChanged ? set.map(tr.changes) : set;
    },
  });
  function build(hunks: BlameHunk[], doc: { lines: number; line(n: number): { from: number } }) {
    const b = new RangeSetBuilder<GutterMarker>();
    for (const h of hunks) {
      for (let k = 0; k < h.count; k++) {
        const n = h.start + k;
        if (n < 1 || n > doc.lines) break;
        const from = doc.line(n).from;
        b.add(from, from, new BlameMarker(h, k === 0, onPick));
      }
    }
    return b.finish();
  }
  return [
    field,
    gutter({
      class: "cm-blameGutter",
      markers: (view) => view.state.field(field),
    }),
    EditorView.baseTheme({
      ".cm-blameGutter": { width: "150px", borderRight: "1px solid var(--border-soft)" },
      ".cm-blameGutter .cm-gutterElement": { padding: "0 8px 0 6px" },
      ".cm-blame": {
        height: "100%",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
        fontSize: "11px",
        color: "var(--text-faint)",
      },
      ".cm-blame.first": { color: "var(--text-dim)", borderTop: "1px solid var(--border-soft)" },
      ".cm-blame.first:hover": { color: "var(--text)" },
      ".cm-blame.wip": { color: "var(--accent)" },
    }),
  ];
}

/** Editor 里装它用的槽：开关就是 reconfigure 成 [] 或 `blameGutter(...)` */
export const blameSlot = new Compartment();
