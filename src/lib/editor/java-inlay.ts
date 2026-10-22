/**
 * 内嵌提示（docs/TAKEOVER.md G11，IDEA 的 Parameter Name Hints）：`rule.apply(100)` 画成 `rule.apply(cents: 100)`，`cents:` 是灰的、不是正文。
 *
 * 给哪些是 jdtls 定的（默认只给字面量参数，同 IDEA；java.rs `inlay_hints` 头上写了出处），这里只管什么时候问、画在哪：
 * - **只问看得见的那一截**（上下各多 20 行）：一个几千行的文件全问一遍是白算，滚过去再问
 * - **打字时不重问，跟着改动挪**（`deco.map`）：问要等文档同步先到 jdtls（250 ms 去抖），这中间提示不该闪没；停下来 600 ms 再问
 * - **jdtls 还在导入时问到的不全**（类型还没解析出来的调用没有提示）：隔 3 秒再问，到它说导入完为止
 *
 * 提示是 widget，**光标不进去**：画在参数前面、`side: 1` —— 光标停在参数开头时画在提示前面（同 IDEA：`(|cents: 100`），
 * 方向键一下跳过整个提示。提示里的字走 `textContent`，不插 HTML（no-html-sinks）
 */
import { StateEffect, StateField, type Extension, type Text } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import type { JavaInlay } from "../ipc/commands";

export type Hint = { pos: number; label: string; param: boolean };

/** jdtls 的行列（从 0 数、列按 UTF-16）→ 文档偏移。越界的丢掉；按位置排好（`Decoration.set` 要） */
export function hintsOf(doc: Text, hs: JavaInlay[]): Hint[] {
  const out: Hint[] = [];
  for (const h of hs) {
    if (h.line < 0 || h.line >= doc.lines) continue;
    const l = doc.line(h.line + 1);
    if (h.character > l.length) continue;
    out.push({ pos: l.from + h.character, label: h.label, param: h.param });
  }
  return out.sort((a, b) => a.pos - b.pos);
}

/**
 * 看得见的那一截，上下各多 `pad` 行：`[起始行, 终点行, 终点列]`，行列从 0 数。**终点是文件里真有的位置**：
 * 到了文件末尾就是最后一行的行尾，不写成「下一行第 0 列」—— jdtls 对不存在的位置整段回空（真 jdtls 上撞的，桩不管这个）
 */
export function linesAround(doc: Text, from: number, to: number, pad = 20): [number, number, number] {
  const end = doc.lineAt(to).number + pad; // 从 1 数的行号 = 下一行从 0 数的行号
  if (end < doc.lines) return [Math.max(0, doc.lineAt(from).number - 1 - pad), end, 0];
  return [Math.max(0, doc.lineAt(from).number - 1 - pad), doc.lines - 1, doc.line(doc.lines).length];
}

class HintWidget extends WidgetType {
  label: string;
  param: boolean;
  constructor(label: string, param: boolean) {
    super();
    this.label = label;
    this.param = param;
  }
  override eq(o: HintWidget) {
    return o.label === this.label && o.param === this.param;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-inlay";
    el.textContent = this.label;
    return el;
  }
}

const setHints = StateEffect.define<Hint[]>();
const field = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    for (const e of tr.effects) if (e.is(setHints)) return Decoration.set(e.value.map((h) => Decoration.widget({ widget: new HintWidget(h.label, h.param), side: 1 }).range(h.pos)));
    return tr.docChanged ? deco.map(tr.changes) : deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** 问从 `from` 行开头到 `(endLine, endChar)` 的：jdtls 能问了才给 Promise。`settled`：导入完了没有（没完的话问到的不全，过一会儿再问） */
type Ask = (view: EditorView, from: number, endLine: number, endChar: number) => { hints: Promise<JavaInlay[]>; settled: boolean } | null;

/** 普通字段、不写 `constructor(readonly …)`：测试在裸 node 里引这个文件（frontend.md） */
class Watch {
  #timer: ReturnType<typeof setTimeout> | null = null;
  view: EditorView;
  ask: Ask;
  constructor(view: EditorView, ask: Ask) {
    this.view = view;
    this.ask = ask;
    this.later(300);
  }

  later(ms: number) {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.fire(), ms);
  }

  update(u: ViewUpdate) {
    if (u.docChanged) this.later(600);
    else if (u.viewportChanged) this.later(150);
  }

  fire() {
    this.#timer = null;
    const { view } = this;
    const doc = view.state.doc;
    const [from, endLine, endChar] = linesAround(doc, view.viewport.from, view.viewport.to);
    const r = this.ask(view, from, endLine, endChar);
    // jdtls 还没起好：过一会儿再看（只查一张表，不发 IPC）
    if (!r) return this.later(2000);
    void r.hints
      .then((hs) => {
        // 问的这一会儿里又改过字：这份对不上了，改动那边已经排了下一次
        if (view.state.doc !== doc) return;
        view.dispatch({ effects: setHints.of(hintsOf(doc, hs)) });
        if (!r.settled) this.later(3000);
      })
      .catch(() => {});
  }

  destroy() {
    if (this.#timer) clearTimeout(this.#timer);
  }
}

export function inlayHints(ask: Ask): Extension {
  return [
    field,
    ViewPlugin.define((view) => new Watch(view, ask)),
    EditorView.theme({
      // 照 IDEA：比正文小一号、灰字、一块淡底；贴在一行字里的小块用 r-xs（ui.md 一之三）
      ".cm-inlay": {
        display: "inline-block",
        padding: "0 4px",
        margin: "0 3px 0 1px",
        borderRadius: "var(--r-xs)",
        background: "var(--inlay-bg)",
        color: "var(--text-faint)",
        fontFamily: "var(--ui-font)",
        fontSize: "0.85em",
        lineHeight: "1.35",
        verticalAlign: "baseline",
      },
    }),
  ];
}
