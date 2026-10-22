/**
 * 参数提示（Java，jdtls 的 signatureHelp）：敲 `(` 或 `,` 时在光标上方列出这个方法的几个重载，当前那个参数加粗 —— 同 IDEA 的 Parameter Info
 * （IDEA 敲括号后自动弹、⌘P 手动弹）。⌘P 原来是按文件名打开，2026-10-20 用户定了照 IDEA 换（找文件挪到 ⇧⌘O，keymap.ts）。
 *
 * 开着的时候每次改动 / 挪光标都重新问一次（去抖 150ms，jdtls 热的时候几十毫秒）：出了括号、换了一个调用，它回空的，就收起 ——
 * 不自己去数括号判断「还在不在调用里」，那要认字符串、注释、泛型的尖括号，jdtls 本来就知道。
 */
import { EditorView, keymap, showTooltip, ViewPlugin, type Tooltip, type ViewUpdate } from "@codemirror/view";
import { Prec, StateEffect, StateField, type Extension } from "@codemirror/state";
import type { JavaSignatures } from "../ipc/commands";
import { idKeymap } from "./id-keys";

const setSig = StateEffect.define<{ pos: number; data: JavaSignatures } | null>();

/** 一次最多画几个重载：再多就是一堵墙，接着敲字让 jdtls 筛 */
const SHOW_MAX = 8;

function render(data: JavaSignatures): HTMLElement {
  const dom = document.createElement("div");
  dom.className = "cm-java-sig";
  const list = data.signatures;
  // 当前那个排第一（IDEA 是原顺序 + 高亮；超过 SHOW_MAX 时当前那个可能被截掉，所以挪到最前）
  const order = [data.active, ...list.map((_, i) => i).filter((i) => i !== data.active)].slice(0, SHOW_MAX);
  for (const i of order) {
    const s = list[i];
    if (!s) continue;
    const row = document.createElement("div");
    row.className = i === data.active ? "row on" : "row";
    const p = s.params[data.param];
    const chars = [...s.label];
    const pieces: [string, boolean][] = p && data.param < s.params.length ? [[chars.slice(0, p.start).join(""), false], [chars.slice(p.start, p.end).join(""), true], [chars.slice(p.end).join(""), false]] : [[s.label, false]];
    for (const [text, cur] of pieces) {
      if (!text) continue;
      const span = document.createElement(cur ? "b" : "span");
      span.textContent = text;
      row.append(span);
    }
    dom.append(row);
  }
  const more = data.total - order.length;
  if (more > 0) {
    const m = document.createElement("div");
    m.className = "more";
    m.textContent = `还有 ${more} 个重载，接着敲参数会筛`;
    dom.append(m);
  }
  return dom;
}

const sigField = StateField.define<{ pos: number; data: JavaSignatures } | null>({
  create: () => null,
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setSig)) return e.value;
    return v && tr.docChanged ? { ...v, pos: tr.changes.mapPos(v.pos) } : v;
  },
  provide: (f) =>
    showTooltip.from(f, (v): Tooltip | null => (v && v.data.signatures.length ? { pos: v.pos, above: true, strictSide: false, arrow: false, create: () => ({ dom: render(v.data) }) } : null)),
});

type Ask = (text: string, line: number, character: number) => Promise<JavaSignatures>;

function plugin(ask: Ask) {
  return ViewPlugin.fromClass(
    class {
      timer: ReturnType<typeof setTimeout> | null = null;
      seq = 0;
      constructor(readonly view: EditorView) {}
      update(u: ViewUpdate) {
        const open = u.state.field(sigField) !== null;
        const typed = u.transactions.some((tr) => tr.isUserEvent("input"));
        const head = u.state.selection.main.head;
        // 跳过空白再看：`foo(a, ` 一次贴进来（或者打完逗号接着打空格）也算
        const prev = u.state.sliceDoc(Math.max(0, head - 40), head).trimEnd().slice(-1);
        // 敲了 ( 或 ,（补全选中一个方法、光标停进括号里也算：插进来的文字里带 ( ）→ 问；开着时改动 / 挪光标 → 再问一次
        if ((typed && u.docChanged && (prev === "(" || prev === ",")) || (open && (u.docChanged || u.selectionSet))) this.schedule(typed ? 100 : 150);
      }
      schedule(ms: number) {
        if (this.timer) clearTimeout(this.timer);
        this.timer = setTimeout(() => void this.query(), ms);
      }
      async query() {
        this.timer = null;
        const v = this.view;
        const head = v.state.selection.main.head;
        const line = v.state.doc.lineAt(head);
        const my = ++this.seq;
        let data: JavaSignatures | null = null;
        try {
          data = await ask(v.state.doc.toString(), line.number - 1, head - line.from);
        } catch {
          /* jdtls 没起好 / 退了：不弹就是了，不为一个提示糊一句红字（同补全） */
        }
        // 问的时候又改了（后一次的回答更新）：这次的作废
        if (my !== this.seq || !v.dom.isConnected) return;
        v.dispatch({ effects: setSig.of(data && data.signatures.length ? { pos: head, data } : null) });
      }
      destroy() {
        if (this.timer) clearTimeout(this.timer);
      }
    },
  );
}

export function signatureHelp(ask: Ask): Extension {
  const p = plugin(ask);
  return [
    sigField,
    p,
    // 手动弹：⌘P（IDEA 的 Parameter Info）。按 id 挂，键位你改得了（docs/TAKEOVER.md R3）
    Prec.high(idKeymap([{ id: "param-info", preventDefault: true, run: (v) => (v.plugin(p)?.schedule(0), true) }])),
    Prec.high(
      keymap.of([
        // 开着时 Esc 只收起它（不然 Esc 往外冒，关掉的是别的东西）
        { key: "Escape", run: (v) => (v.state.field(sigField) ? (v.dispatch({ effects: setSig.of(null) }), true) : false) },
      ]),
    ),
    EditorView.baseTheme({
      ".cm-java-sig": { padding: "4px 0", maxWidth: "640px", fontFamily: "var(--code-font)", fontSize: "var(--fs-sm)", color: "var(--text-dim)" },
      ".cm-java-sig .row": { padding: "2px 10px", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
      ".cm-java-sig .row.on": { color: "var(--text)" },
      // 当前参数：加粗 + 强调色（IDEA 是加粗）；不在当前那个重载里的也标出位置，淡一些
      ".cm-java-sig b": { fontWeight: "600", color: "var(--accent)" },
      ".cm-java-sig .row:not(.on) b": { color: "var(--text)" },
      ".cm-java-sig .more": { padding: "2px 10px 0", color: "var(--text-faint)", fontFamily: "var(--ui-font)", fontSize: "var(--fs-xs)" },
    }),
  ];
}
