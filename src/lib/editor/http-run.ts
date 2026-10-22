/**
 * `.http` 文件的编辑器扩展（docs/IDEA.md 第 19 节），照 IDEA 的 HTTP Client：
 * - 每个请求的请求行旁边一个 ▶，点了发它；⌃⇧R 发光标所在的那个（IDEA 里每个请求是一个临时运行配置，⌃⇧R 就是通用的「运行光标这儿的」，同测试文件）
 * - 编辑器顶上一条「运行环境」（IDEA 的 Run with）：环境名来自 `http-client.env.json` / `.private.`，按项目记住选的哪个
 *
 * 发的是编辑器里**此刻**的文本（没保存的也算）。切请求、替换变量在 `http/http-file.ts`，发在 `http/http-ops.ts`（动态引：没发过就不拉）。
 */
import { EditorView, GutterMarker, gutter, showPanel, type Panel } from "@codemirror/view";
import { Prec, RangeSet, StateField, type Extension } from "@codemirror/state";
import { parseHttp, type HttpReq } from "../http/http-file";
import { httpEnvs } from "../ipc/http";
import { idKeymap } from "./id-keys";

const reqField = StateField.define<HttpReq[]>({
  create: (s) => parseHttp(s.doc.toString()).requests,
  update: (v, tr) => (tr.docChanged ? parseHttp(tr.state.doc.toString()).requests : v),
});

class SendMarker extends GutterMarker {
  constructor(readonly r: HttpReq) {
    super();
  }
  override eq(o: SendMarker) {
    return o.r.line === this.r.line && o.r.method === this.r.method && o.r.url === this.r.url;
  }
  override toDOM() {
    const el = document.createElement("span");
    el.className = "cm-http-send";
    el.title = `发送 ${this.r.name ?? `${this.r.method} ${this.r.url}`}（⌃⇧R）`;
    return el;
  }
}

async function send(root: string, path: string, view: EditorView, line: number) {
  const { sendAt } = await import("../http/http-ops");
  await sendAt(root, path, view.state.doc.toString(), line);
}

/** 顶上那一条：运行环境（无 / dev / test …），右边一句键位提示 */
function envPanel(root: string, path: string) {
  return (): Panel => {
    const dom = document.createElement("div");
    dom.className = "cm-http-env";
    let names: string[] = [];
    let problem = "";
    let ops: typeof import("../http/http-ops") | null = null;
    let off: (() => void) | null = null;
    let dead = false;
    const render = () => {
      dom.replaceChildren();
      const label = document.createElement("span");
      label.className = "lbl";
      label.textContent = "运行环境";
      dom.append(label);
      // 选过的环境在环境文件里没有了（改了名、删了）：当没选
      const cur = ops?.envOf(root) ?? null;
      const sel = cur && names.includes(cur) ? cur : null;
      if (!names.length) {
        const none = document.createElement("span");
        none.className = "hint";
        none.textContent = problem || "没有环境文件（http-client.env.json，放在这个文件旁边或上层目录）";
        dom.append(none);
      } else {
        const segs = document.createElement("div");
        segs.className = "segs";
        for (const n of [null, ...names]) {
          const b = document.createElement("button");
          b.className = `btn sm${n === sel ? " on" : ""}`;
          b.textContent = n ?? "无";
          b.title = n ? `用环境「${n}」里的变量` : "不用环境（只用文件里 @名字 = 值 定义的）";
          b.onmousedown = (e) => e.preventDefault();
          b.onclick = () => ops?.setEnv(root, n);
          segs.append(b);
        }
        dom.append(segs);
      }
      const keys = document.createElement("span");
      keys.className = "keys";
      keys.textContent = "⌃⇧R 发光标所在的请求";
      dom.append(keys);
    };
    const load = () =>
      httpEnvs(root, path)
        .then((e) => {
          if (dead) return;
          names = e.names;
          problem = e.problems[0] ?? "";
          render();
        })
        .catch(() => {});
    render();
    void import("../http/http-ops").then((m) => {
      if (dead) return;
      ops = m;
      off = m.onEnv(render);
      render();
    });
    void load();
    return {
      dom,
      top: true,
      // 回到这个编辑器时重读一次环境文件（可能刚在旁边改过）
      update(u) {
        if (u.focusChanged && u.view.hasFocus) void load();
      },
      destroy() {
        dead = true;
        off?.();
      },
    };
  };
}

export function httpRun(root: string | null, path: string): Extension {
  if (!root || !path.startsWith(`${root}/`)) return [];
  return [
    reqField,
    gutter({
      class: "cm-httpGutter",
      markers: (view) => {
        const doc = view.state.doc;
        return RangeSet.of(view.state.field(reqField).filter((r) => r.line <= doc.lines).map((r) => new SendMarker(r).range(doc.line(r.line).from)));
      },
      lineMarkerChange: (u) => u.docChanged,
      domEventHandlers: {
        mousedown(view, line) {
          const n = view.state.doc.lineAt(line.from).number;
          if (!view.state.field(reqField).some((r) => r.line === n)) return false;
          void send(root, path, view, n);
          return true;
        },
      },
    }),
    showPanel.of(envPanel(root, path)),
    Prec.high(
      idKeymap([
        {
          id: "run-test",
          preventDefault: true,
          run: (view) => {
            void send(root, path, view, view.state.doc.lineAt(view.state.selection.main.head).number);
            return true;
          },
        },
      ]),
    ),
    EditorView.baseTheme({
      ".cm-httpGutter .cm-gutterElement": { padding: "0 2px", display: "flex", alignItems: "center", justifyContent: "center" },
      // IDEA 的 run 图标（绿三角，本来就带色）当背景图画，同测试文件的 ▶
      ".cm-http-send": { width: "14px", height: "14px", cursor: "pointer", background: "url(/icons/run.svg) center / 14px 14px no-repeat" },
      // 顶上那一条：岛里的工具条（34 高，底下一道分区线，ui.md 一之二 / 第二条）
      ".cm-http-env": { display: "flex", alignItems: "center", gap: "8px", height: "34px", padding: "0 10px", borderBottom: "1px solid var(--border-soft)", fontFamily: "var(--ui-font)", fontSize: "var(--fs-sm)", color: "var(--text-dim)" },
      ".cm-http-env .lbl": { color: "var(--text-faint)" },
      ".cm-http-env .hint": { color: "var(--text-faint)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
      ".cm-http-env .keys": { marginLeft: "auto", color: "var(--text-faint)", fontSize: "var(--fs-xs)", whiteSpace: "nowrap" },
    }),
  ];
}
