/**
 * 覆盖率在编辑器里的样子（docs/TAKEOVER.md G15，照 IDEA）：行号右边一条细色条 —— 绿 = 跑到了、黄 = 跑到一部分（有分支没走到）、红 = 没跑到。
 * 数据是最近一次带覆盖率跑出来的报告（`state/coverage.ts`）；报告换了、清掉了，开着的编辑器当场重画。
 *
 * 用 `gutterLineClass` 给行号那一格加类、CSS 画边（同书签那样不盖住数字）。改了正文就跟着映射 —— 行号是报告那一刻的，
 * 改过的文件颜色可能对不上新代码，IDEA 也是这样（它在改过的文件上照样画，重跑一次就对了）
 */
import { EditorView, GutterMarker, ViewPlugin, gutterLineClass } from "@codemirror/view";
import { RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { coverageOf, onCoverage, type Lines } from "../state/coverage";

class CovMarker extends GutterMarker {
  override elementClass: string;
  constructor(cls: string) {
    super();
    this.elementClass = cls;
  }
}
const HIT = new CovMarker("cm-cov cm-cov-hit");
const PART = new CovMarker("cm-cov cm-cov-part");
const MISS = new CovMarker("cm-cov cm-cov-miss");

const reset = StateEffect.define<Lines | undefined>();

function setOf(state: EditorState, l: Lines | undefined) {
  if (!l) return RangeSet.empty;
  const out = [];
  for (let n = 1; n <= state.doc.lines; n++) {
    const m = l.covered.has(n) ? HIT : l.partial.has(n) ? PART : l.missed.has(n) ? MISS : null;
    if (m) out.push(m.range(state.doc.line(n).from));
  }
  return RangeSet.of(out);
}

export function coverage(path: string): Extension {
  const field = StateField.define<RangeSet<CovMarker>>({
    create: (s) => setOf(s, coverageOf(path)),
    update(v, tr) {
      for (const e of tr.effects) if (e.is(reset)) return setOf(tr.state, e.value);
      return tr.docChanged ? v.map(tr.changes) : v;
    },
    provide: (f) => gutterLineClass.from(f),
  });
  const follow = ViewPlugin.fromClass(
    class {
      off: () => void;
      constructor(view: EditorView) {
        this.off = onCoverage(() => view.dispatch({ effects: reset.of(coverageOf(path)) }));
      }
      destroy() {
        this.off();
      }
    },
  );
  return [
    field,
    follow,
    EditorView.baseTheme({
      // 行号格子的右边一条 3px 的色条（IDEA 的位置）。内阴影不占宽度，不把正文推开
      ".cm-lineNumbers .cm-gutterElement.cm-cov-hit": { boxShadow: "inset -3px 0 0 var(--cov-hit, #5fb865)" },
      ".cm-lineNumbers .cm-gutterElement.cm-cov-part": { boxShadow: "inset -3px 0 0 var(--cov-part, #e5c07b)" },
      ".cm-lineNumbers .cm-gutterElement.cm-cov-miss": { boxShadow: "inset -3px 0 0 var(--cov-miss, #e06c75)" },
    }),
  ];
}
