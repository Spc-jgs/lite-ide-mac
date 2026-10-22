/**
 * 断点和执行点（docs/IDEA.md 第 18 节），照 IDEA 新 UI 的默认样子：**断点画在行号上**（点行号下 / 取消，红点盖住那个数字），
 * 停下来时那一整行标成执行点（停在这儿的那一帧亮、选了下面的帧淡一档）。⌘F8 在光标那行下 / 取消。
 *
 * 断点跟着行走：存成 RangeSet 挂在每行开头，改动时 CM6 替我们映射（上面插了三行，断点跟着下移三行）；映射完写回 `debug/breakpoints.ts`
 * （去抖 400ms —— 那边每次变了都要过一次 IPC 告诉 Rust）。
 *
 * 不靠语言服务器：下在哪一行就是哪一行。「这一行能不能停」（空行、注释）要等调试器挂上、按编译出来的行号表才知道，这一版不画「没对上」的样子。
 *
 * **条件断点**（IDEA 的 Condition）：右键行号上的断点，填一个 Java 表达式，为真才停（`orderId == 42`）。条件挂在标记上、跟着行一起走；
 * 有条件的红点里多一个「?」（IDEA 是同一个意思的小记号）。条件由 java-debug 在目标 JVM 里算。
 *
 * **右键是断点属性窗**（2026-10-22，docs/TAKEOVER.md G14，照 IDEA 右键断点那个小窗）：启用、条件、命中次数（第几次走到才停）、
 * 日志断点（不停，打一句进运行窗的输出）、删除。停用的画成空心（IDEA 同样），日志断点是橙点（IDEA 不挂起的断点是黄的）
 */
import { Decoration, EditorView, GutterMarker, ViewPlugin, WidgetType, gutterLineClass, lineNumberMarkers, lineNumbers, type DecorationSet } from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { inlineVals, type Val } from "../debug/inline-vals";
import { Prec, RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { bpTitle, bpsOf, onBreakpoints, sameBp, setBp, setBps, toggle, type Bp } from "../debug/breakpoints";
import { imeKey } from "../shell/ime";
import { execPoint, onExecPoint } from "../debug/exec-point";
import { idKeymap } from "./id-keys";

class BpMarker extends GutterMarker {
  // 不写成 `constructor(readonly bp)`：那是要编译的 TS 语法，状态层测试在裸 node 里剥类型跑，剥不掉它。
  // 行号不放在标记里比：标记跟着行走，行号是读出来的（`bpsIn`）
  readonly bp: Omit<Bp, "line">;
  constructor(bp: Bp) {
    super();
    const { line: _l, ...rest } = bp;
    this.bp = rest;
  }
  override eq(o: BpMarker) {
    return sameBp({ line: 0, ...o.bp }, { line: 0, ...this.bp });
  }
  override toDOM() {
    const b = this.bp;
    const el = document.createElement("span");
    el.className = `cm-bp${b.cond || b.hit ? " cond" : ""}${b.log ? " log" : ""}${b.off ? " off" : ""}`;
    el.title = bpTitle(b);
    return el;
  }
}

/** 换成这些断点：外面（另一个分屏、调试开始前同步、改了条件）改了表 */
const resetBps = StateEffect.define<Bp[]>();

function bpSet(state: EditorState, list: Bp[]): RangeSet<BpMarker> {
  const doc = state.doc;
  return RangeSet.of(
    list.filter((b) => b.line <= doc.lines).map((b) => new BpMarker(b).range(doc.line(b.line).from)),
    true,
  );
}

/** 标记此刻在哪几行、各带什么条件（同一行两个的话后面的算数，`setBps` 会去重） */
function bpsIn(state: EditorState, set: RangeSet<BpMarker>): Bp[] {
  const out: Bp[] = [];
  for (const it = set.iter(); it.value; it.next()) {
    out.push({ ...it.value.bp, line: state.doc.lineAt(it.from).number });
  }
  return out;
}

const same = (a: Bp[], b: Bp[]) => a.length === b.length && a.every((x, i) => sameBp(x, b[i]));

/**
 * 断点属性窗（照 IDEA 右键断点那个小窗）：挂在鼠标那儿（body 上，按屏幕坐标摆 —— frontend.md「container-type 的元素里 fixed 不以窗口为参照」）。
 * ↵ / 点外面 = 完成（IDEA 也是点开就生效）、Esc 不改（`done(null)`）、「删除」= `done("delete")`。
 * 裸 DOM 拼（同查找条）：这个模块跟着 Java 的编辑器扩展走，为一个小窗引 Svelte 组件要多一层挂载
 */
export function editBp(x: number, y: number, line: number, cur: Bp | undefined, done: (bp: Bp | "delete" | null) => void) {
  const box = document.createElement("div");
  box.className = "popup cm-bp-props";
  box.style.cssText = `position:fixed;left:${Math.max(8, Math.min(x, innerWidth - 400))}px;top:${Math.min(y + 8, innerHeight - 300)}px;z-index:70;width:380px;padding:10px 12px`;
  const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text = "") => {
    const e = document.createElement(tag);
    e.className = cls;
    if (text) e.textContent = text;
    return e;
  };
  const field = (label: string, value: string, placeholder: string) => {
    const row = el("label", "row");
    const input = el("input", "in");
    input.value = value;
    input.placeholder = placeholder;
    input.spellcheck = false;
    row.append(el("span", "lb", label), input);
    return { row, input };
  };
  const check = (label: string, on: boolean) => {
    const row = el("label", "row chk");
    const input = el("input", "");
    input.type = "checkbox";
    input.checked = on;
    row.append(input, el("span", "", label));
    return { row, input };
  };
  const title = el("div", "t", `第 ${line} 行的断点`);
  const enabled = check("启用", !cur?.off);
  const cond = field("条件", cur?.cond ?? "", "Java 表达式，为真才停：orderId == 42");
  const hit = field("第几次", cur?.hit ?? "", "第几次走到才停，空 = 每次");
  hit.input.inputMode = "numeric";
  const logOn = check("只打一句、不停（日志断点）", !!cur?.log);
  const log = field("打什么", cur?.log ?? "", "进来了 orderId={orderId}");
  const hint = el("div", "h", "↵ 完成 · Esc 不改 · {表达式} 换成它的值，打进运行窗的输出");
  const actions = el("div", "acts");
  const del = el("button", "btn sm quiet", "删除");
  const ok = el("button", "btn sm primary", "完成");
  actions.append(del, ok);
  const sync = () => {
    log.row.style.display = logOn.input.checked ? "" : "none";
    hit.input.classList.toggle("bad", !!hit.input.value.trim() && !/^[1-9]\d{0,8}$/.test(hit.input.value.trim()));
  };
  logOn.input.addEventListener("change", sync);
  hit.input.addEventListener("input", sync);
  sync();
  box.append(title, enabled.row, cond.row, hit.row, logOn.row, log.row, hint, actions);
  document.body.append(box);
  let closed = false;
  const close = (result: Bp | "delete" | null) => {
    if (closed) return;
    closed = true;
    box.remove();
    document.removeEventListener("mousedown", outside, true);
    done(result);
  };
  const result = (): Bp => {
    const b: Bp = { line };
    if (cond.input.value.trim()) b.cond = cond.input.value.trim();
    if (/^[1-9]\d{0,8}$/.test(hit.input.value.trim())) b.hit = hit.input.value.trim();
    if (logOn.input.checked && log.input.value.trim()) b.log = log.input.value.trim();
    if (!enabled.input.checked) b.off = true;
    return b;
  };
  const outside = (e: MouseEvent) => {
    if (!box.contains(e.target as Node)) close(result());
  };
  box.addEventListener("keydown", (e) => {
    if (imeKey(e)) return;
    if (e.key === "Enter" && (e.target as HTMLElement).tagName !== "BUTTON") {
      e.preventDefault();
      close(result());
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close(null);
    }
  });
  del.addEventListener("click", () => close("delete"));
  ok.addEventListener("click", () => close(result()));
  setTimeout(() => document.addEventListener("mousedown", outside, true));
  cond.input.focus();
  cond.input.select();
}

const execEffect = StateEffect.define<{ line: number; top: boolean; vals?: Val[] } | null>();

/** 行内变量值：行尾一段灰字（IDEA 停下时的样子）。不进正文、光标进不去 */
class ValWidget extends WidgetType {
  text: string;
  constructor(text: string) {
    super();
    this.text = text;
  }
  override eq(o: ValWidget) {
    return o.text === this.text;
  }
  override toDOM() {
    const el = document.createElement("span");
    el.className = "cm-inline-val";
    el.textContent = this.text;
    return el;
  }
}

/** 执行点所在的方法从第几行开始（Lezer 的语法树往上找方法 / 构造器 / lambda）；找不到就往上 30 行 */
function methodStart(state: EditorState, line: number): number {
  const pos = state.doc.line(line).from;
  for (let n: ReturnType<ReturnType<typeof syntaxTree>["resolveInner"]> | null = syntaxTree(state).resolveInner(pos, 1); n; n = n.parent) {
    if (n.name === "MethodDeclaration" || n.name === "ConstructorDeclaration" || n.name === "LambdaExpression") return state.doc.lineAt(n.from).number;
  }
  return Math.max(1, line - 30);
}

/** 执行点 + 行内变量值的那一套装饰 */
function execDeco(state: EditorState, p: { line: number; top: boolean; vals?: Val[] } | null): DecorationSet {
  if (!p || p.line > state.doc.lines) return Decoration.none;
  const ranges = [(p.top ? execLine : execLineUnder).range(state.doc.line(p.line).from)];
  if (p.vals?.length) {
    const lines = state.doc.toString().split("\n");
    for (const [n, text] of inlineVals(lines, methodStart(state, p.line), p.line, p.vals)) {
      ranges.push(Decoration.widget({ widget: new ValWidget(text), side: 1 }).range(state.doc.line(n).to));
    }
  }
  return Decoration.set(ranges, true);
}

const execLine = Decoration.line({ class: "cm-exec-line" });
const execLineUnder = Decoration.line({ class: "cm-exec-line under" });
class ExecGutter extends GutterMarker {
  override elementClass = "cm-exec-gutter";
}

export function javaDebugGutter(root: string, path: string): Extension {
  const bps = StateField.define<RangeSet<BpMarker>>({
    create: (s) => bpSet(s, bpsOf(root, path)),
    update(v, tr) {
      for (const e of tr.effects) if (e.is(resetBps)) return bpSet(tr.state, e.value);
      return tr.docChanged ? v.map(tr.changes) : v;
    },
    provide: (f) => lineNumberMarkers.from(f),
  });

  const exec = StateField.define<DecorationSet>({
    create(s) {
      const p = execPoint();
      return execDeco(s, p && p.path === path ? p : null);
    },
    update(v, tr) {
      for (const e of tr.effects) if (e.is(execEffect)) return execDeco(tr.state, e.value);
      return tr.docChanged ? v.map(tr.changes) : v;
    },
    provide: (f) => [
      EditorView.decorations.from(f),
      // 行号那一格也染上（同 IDEA：执行点那一行连行号底下都是蓝的）
      gutterLineClass.from(f, (d) => {
        const marks: ReturnType<GutterMarker["range"]>[] = [];
        const it = d.iter();
        for (; it.value; it.next()) marks.push(new ExecGutter().range(it.from));
        return RangeSet.of(marks);
      }),
    ],
  });

  const atCursor = (view: EditorView) => {
    toggle(root, path, view.state.doc.lineAt(view.state.selection.main.head).number);
    return true;
  };

  return [
    bps,
    exec,
    lineNumbers({
      domEventHandlers: {
        // 点行号：下 / 取消断点。按下就算（不等抬起），同 IDEA；拖选正文从行号上开始的那种不受影响 —— 那是在正文里按下的
        mousedown(view, line, ev) {
          if ((ev as MouseEvent).button !== 0) return false;
          toggle(root, path, view.state.doc.lineAt(line.from).number);
          return true;
        },
        // 右键：这一行断点的属性窗（没有断点的行：填了什么才新下一个）。同 IDEA：右键断点出属性框
        contextmenu(view, line, ev) {
          const n = view.state.doc.lineAt(line.from).number;
          const cur = bpsOf(root, path).find((b) => b.line === n);
          const e = ev as MouseEvent;
          e.preventDefault();
          editBp(e.clientX, e.clientY, n, cur, (bp) => {
            if (bp === "delete") {
              if (cur) setBps(root, path, bpsOf(root, path).filter((b) => b.line !== n));
            } else if (bp && (cur || bp.cond || bp.hit || bp.log)) {
              // 没断点的行、又什么都没填：什么都不做（右键一下不该凭空多一个断点）
              setBp(root, path, bp);
            }
            // 焦点还给编辑器：小框一摘，焦点落在 body 上，接着打的字没地方去（浏览器里验时撞见的）
            view.focus();
          });
          return true;
        },
      },
    }),
    ViewPlugin.define((view) => {
      let timer: ReturnType<typeof setTimeout> | null = null;
      const flush = () => {
        timer = null;
        setBps(root, path, bpsIn(view.state, view.state.field(bps)));
      };
      // 表变了（点了行号、⌘F8、另一个分屏里改的）：照表重画。自己映射完写回去的那一次，表和画面本来就一样，重画一次无妨
      const offBps = onBreakpoints((p) => {
        if (p !== path) return;
        const want = bpsOf(root, path);
        if (!same(want, bpsIn(view.state, view.state.field(bps)))) view.dispatch({ effects: resetBps.of(want) });
      });
      const offExec = onExecPoint(() => {
        const p = execPoint();
        view.dispatch({ effects: execEffect.of(p && p.path === path ? { line: p.line, top: p.top, vals: p.vals } : null) });
      });
      return {
        update(u) {
          if (!u.docChanged || u.startState.field(bps) === u.state.field(bps)) return;
          if (timer) clearTimeout(timer);
          timer = setTimeout(flush, 400);
        },
        destroy() {
          if (timer) {
            clearTimeout(timer);
            flush();
          }
          offBps();
          offExec();
        },
      };
    }),
    Prec.high(idKeymap([{ id: "toggle-breakpoint", preventDefault: true, run: atCursor }])),
    EditorView.baseTheme({
      // 行号格里的红点：14 的图标居中，盖住数字（IDEA 新 UI 的「断点画在行号上」）
      ".cm-lineNumbers .cm-gutterElement": { cursor: "pointer" },
      ".cm-bp": { display: "inline-block", width: "14px", height: "14px", verticalAlign: "middle", background: "url(/icons/breakpoint.svg) center / 14px 14px no-repeat" },
      // 日志断点：橙点（IDEA 不挂起的断点是黄的）；停用：空心（IDEA 同样）
      ".cm-bp.log": { filter: "hue-rotate(32deg) saturate(1.3)" },
      ".cm-bp.off": { background: "none", boxSizing: "border-box", width: "11px", height: "11px", margin: "1.5px", borderRadius: "99px", border: "1.5px solid #e5534b" },
      ".cm-bp.off.log": { borderColor: "#e8a33d", filter: "none" },
      // 条件断点、带命中次数的：红点里一个白色的「?」
      ".cm-bp.cond": { position: "relative" },
      ".cm-bp.off.cond::after": { color: "#e5534b" },
      ".cm-bp.cond::after": { content: '"?"', position: "absolute", inset: "0", textAlign: "center", lineHeight: "14px", fontSize: "10px", fontWeight: "700", color: "#fff", fontFamily: "var(--ui-font)" },
      // 执行点：整行淡蓝（IDEA 的 Execution point），选了下面的帧时更淡
      ".cm-exec-line": { backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent) !important" },
      ".cm-exec-line.under": { backgroundColor: "color-mix(in srgb, var(--accent) 14%, transparent) !important" },
      // 行内变量值：行尾灰字（IDEA 的样子），和正文隔开一段
      ".cm-inline-val": { marginLeft: "2.5em", color: "var(--text-faint)", fontStyle: "italic", whiteSpace: "pre" },
      ".cm-gutterElement.cm-exec-gutter": { backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent)", color: "var(--text)" },
    }),
  ];
}
