/**
 * 同名高亮（docs/TAKEOVER.md G11，IDEA 的「Highlight usages of element at caret」）：光标停在一个名字上，
 * 这个文件里它出现的地方都铺一层淡底，写的那几处（赋值、带初值的声明）换一种颜色。
 *
 * 问的是 jdtls（`textDocument/documentHighlight`），是**语义的**：同名的另一个局部变量、别的类的同名字段不算。
 * 这正是它和 CM6 自带的 `highlightSelectionMatches`（选中了字才标、按字面找）不一样的地方，两个都留着。
 *
 * 三条节奏，都照 IDEA：
 * - **光标停下来才问**（180ms）—— 方向键一路按过去不该一路问；刚打过字等久一点（500ms），让去抖的文档同步（250ms）先到 jdtls
 * - **打字就收**：位置会变，而且新敲的字 jdtls 还没看到，留着就是标错的地方
 * - **在同一个名字里左右挪不重问**：光标还在已经标出来的某一处里，答案不会变
 */
import { StateEffect, StateField, type Extension, type Text } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import type { JavaHighlight } from "../ipc/commands";
import { settings } from "../state/settings.svelte";

export type Spot = { from: number; to: number; write: boolean };

/** jdtls 的行列（从 0 数、列按 UTF-16 —— 正好是 JS 字符串的下标）→ 文档偏移。越界的、空的丢掉；按位置排好（`Decoration.set` 要） */
export function spotsOf(doc: Text, hs: JavaHighlight[]): Spot[] {
  const at = (line: number, ch: number) => {
    if (line < 0 || line >= doc.lines) return -1;
    const l = doc.line(line + 1);
    return ch > l.length ? -1 : l.from + ch;
  };
  const out: Spot[] = [];
  for (const h of hs) {
    const from = at(h.line, h.character);
    const to = at(h.endLine, h.endCharacter);
    if (from < 0 || to <= from) continue;
    out.push({ from, to, write: h.write });
  }
  return out.sort((a, b) => a.from - b.from);
}

/** 光标还在已经标出来的某一处里（名字的两头也算） */
export const inSpot = (spots: Spot[], pos: number) => spots.some((s) => pos >= s.from && pos <= s.to);

/** 光标贴着一个标识符（左边或右边是 Java 标识符的字）。空白、符号上不问 */
export function onWord(doc: Text, pos: number): boolean {
  const l = doc.lineAt(pos);
  const id = /[\p{L}\p{N}_$]/u;
  const i = pos - l.from;
  return id.test(l.text[i] ?? "") || id.test(l.text[i - 1] ?? "");
}

const setSpots = StateEffect.define<Spot[]>();
const READ = Decoration.mark({ class: "cm-usage" });
const WRITE = Decoration.mark({ class: "cm-usage-w" });

const field = StateField.define<{ spots: Spot[]; deco: DecorationSet }>({
  create: () => ({ spots: [], deco: Decoration.none }),
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setSpots)) return { spots: e.value, deco: Decoration.set(e.value.map((s) => (s.write ? WRITE : READ).range(s.from, s.to))) };
    return tr.docChanged && v.spots.length ? { spots: [], deco: Decoration.none } : v;
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});

/** 问一次：jdtls 没起好就给 null（不问） */
type Ask = (view: EditorView, line: number, character: number) => Promise<JavaHighlight[]> | null;

/** 普通字段、不写 `constructor(readonly …)`：测试在裸 node 里引这个文件，那种写法剥不掉类型（frontend.md） */
class Watch {
  #timer: ReturnType<typeof setTimeout> | null = null;
  view: EditorView;
  ask: Ask;
  constructor(view: EditorView, ask: Ask) {
    this.view = view;
    this.ask = ask;
    // 设置里关掉：标着的当场收；打开：等下一次光标停下来
    this.#off = settings.subscribe((s) => {
      if (!s.editorHighlightUsages && this.view.state.field(field).spots.length) this.view.dispatch({ effects: setSpots.of([]) });
    });
  }
  #off: () => void;

  update(u: ViewUpdate) {
    if (!u.selectionSet && !u.docChanged) return;
    const sel = u.state.selection.main;
    const spots = u.state.field(field).spots;
    if (!u.docChanged && sel.empty && inSpot(spots, sel.head)) return;
    if (this.#timer) clearTimeout(this.#timer);
    // 挪出了标出来的那几处：当场收（update 里不能 dispatch，推到下一拍）
    if (spots.length && !u.docChanged) queueMicrotask(() => this.view.state.field(field).spots === spots && this.view.dispatch({ effects: setSpots.of([]) }));
    this.#timer = setTimeout(() => this.fire(), u.docChanged ? 500 : 180);
  }

  fire() {
    this.#timer = null;
    const { view } = this;
    // 设置里关了（editor.highlightUsages）：不问
    if (!settings.v.editorHighlightUsages) return;
    const sel = view.state.selection.main;
    if (!sel.empty || !onWord(view.state.doc, sel.head)) return;
    const doc = view.state.doc;
    const l = doc.lineAt(sel.head);
    const p = this.ask(view, l.number - 1, sel.head - l.from);
    if (!p) return;
    void p
      .then((hs) => {
        // 问的这一会儿里改过字、或者光标走了：这份答案对不上了
        if (view.state.doc !== doc || view.state.selection.main.head !== sel.head) return;
        view.dispatch({ effects: setSpots.of(spotsOf(doc, hs)) });
      })
      .catch(() => {});
  }

  destroy() {
    if (this.#timer) clearTimeout(this.#timer);
    this.#off();
  }
}

export function usageHighlight(ask: Ask): Extension {
  return [
    field,
    ViewPlugin.define((view) => new Watch(view, ask)),
    EditorView.theme({
      ".cm-usage": { backgroundColor: "var(--usage-read)" },
      ".cm-usage-w": { backgroundColor: "var(--usage-write)" },
    }),
  ];
}
