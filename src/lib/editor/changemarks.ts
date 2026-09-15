import { StateEffect, StateField, RangeSet, RangeSetBuilder } from "@codemirror/state";
import { EditorView, gutter, GutterMarker } from "@codemirror/view";

/**
 * 改动行标记：哪些行相对 HEAD 动过（issue #33 ④）。
 *
 * 一份数据两处画：行号右边一条 3px 的色带（IDEA 的 gutter 标记，绿加 / 蓝改 /
 * 红删），和缩略图左缘那条（`minimap.ts` 读同一个字段）。字段放在这里而不是
 * 缩略图里，是因为**缩略图可以关**，关了标记不能跟着没。
 *
 * 数据从 `Editor.svelte` 用 `setChangeMarks` 喂进来，那边每次改动都重算一遍
 * （`git/linediff.ts`），所以这里存的是「此刻编辑器里的文本」的标记。
 * 文本变了、新标记还没算出来的那 150ms 里，旧标记按 `tr.changes` 平移 ——
 * 不平移的话在第 1 行前面回车一下，下面所有色带都会错一行，直到防抖到期。
 */
export type MarkKind = "add" | "mod" | "del";

/** 设置改动标记：行号（1-based）→ 类型。换一整份，不做增量 */
export const setChangeMarks = StateEffect.define<Map<number, MarkKind>>();

export const marksField = StateField.define<Map<number, MarkKind>>({
  create: () => new Map(),
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setChangeMarks)) return e.value;
    return value;
  },
});

class Stripe extends GutterMarker {
  constructor(readonly kind: MarkKind) {
    super();
  }
  override eq(other: Stripe) {
    return other.kind === this.kind;
  }
  override toDOM() {
    const el = document.createElement("div");
    el.className = `cm-changeStripe ${this.kind}`;
    return el;
  }
}
const STRIPES: Record<MarkKind, Stripe> = { add: new Stripe("add"), mod: new Stripe("mod"), del: new Stripe("del") };

/**
 * gutter 要的是按位置排好的 RangeSet，从行号表现算。
 * 新标记到了就重建；只是文本变了就把旧的按改动平移（见文件头）。
 */
const stripes = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(set, tr) {
    for (const e of tr.effects) {
      if (e.is(setChangeMarks)) return build(e.value, tr.state.doc);
    }
    return tr.docChanged ? set.map(tr.changes) : set;
  },
});

function build(marks: Map<number, MarkKind>, doc: { lines: number; line(n: number): { from: number } }) {
  const b = new RangeSetBuilder<GutterMarker>();
  // Map 的顺序是插入顺序，del 可能晚于它后面的行插进来 —— builder 要求升序
  for (const n of [...marks.keys()].sort((x, y) => x - y)) {
    if (n < 1 || n > doc.lines) continue;
    const from = doc.line(n).from;
    b.add(from, from, STRIPES[marks.get(n)!]);
  }
  return b.finish();
}

const theme = EditorView.baseTheme({
  ".cm-changeGutter": { width: "3px" },
  ".cm-changeGutter .cm-gutterElement": { padding: "0" },
  ".cm-changeStripe": { width: "3px", height: "100%" },
  ".cm-changeStripe.add": { background: "#63b76c" },
  ".cm-changeStripe.mod": { background: "#4f9ee3" },
  // 删除是「这里少了东西」：不是整行，画一个小楔子挂在行的上缘
  ".cm-changeStripe.del": {
    background: "transparent",
    height: "0",
    borderLeft: "3px solid #d1707a",
    borderTop: "3px solid transparent",
    borderBottom: "3px solid transparent",
    marginTop: "-3px",
  },
});

export function changeMarks() {
  return [
    marksField,
    stripes,
    gutter({
      class: "cm-changeGutter",
      markers: (view) => view.state.field(stripes),
    }),
    theme,
  ];
}
