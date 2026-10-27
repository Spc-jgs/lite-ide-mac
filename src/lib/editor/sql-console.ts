/**
 * SQL 控制台（docs/DATABASE.md 第 5 节 DB-2）：每个连接一份 `.sql`，在应用数据目录 `db/console/<连接 id>/` 底下，就是普通的编辑器标签 ——
 * 语法高亮、多光标、撤销、本地历史全是现成的。这个扩展只加三样：
 * - ⌘↵ 执行光标所在的那条（按 `;` 和空行切，`db/sql-split.ts`），选中了就执行选中的那几条；执行的那段闪一下底色，看得出跑的是哪段
 * - 顶上一条：连的是哪个库、「执行」「历史」「结果」
 * - 历史（`db/HistoryPopup.svelte`）选一条，插到光标处
 *
 * 认文件靠路径（`project.dbConsole`：应用数据目录里的），不靠扩展名：项目里的 `.sql` 文件不是控制台、不知道该连哪个库。
 * 存盘同草稿，自己存（`project.autosaves`）
 */
import { Decoration, EditorView, showPanel, type DecorationSet, type Panel } from "@codemirror/view";
import { Prec, StateEffect, StateField, type Extension } from "@codemirror/state";
import { mount, unmount } from "svelte";
import { idKeymap } from "./id-keys";
import { statementAt, statementsIn } from "../db/sql-split";
import { dbList } from "../ipc/db";
import { layout } from "../state/layout.svelte";
import { project } from "../state/project.svelte";
import { notify } from "../state/notify.svelte";

const setFlash = StateEffect.define<{ from: number; to: number } | null>();
const flashMark = Decoration.mark({ class: "cm-sql-running" });
const flashField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(v, tr) {
    v = v.map(tr.changes);
    for (const e of tr.effects) if (e.is(setFlash)) v = e.value && e.value.to > e.value.from ? Decoration.set([flashMark.range(e.value.from, e.value.to)]) : Decoration.none;
    return v;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** 连接名（结果标签、历史上要写）：配置里查，查不到用 id */
async function nameOf(root: string, conn: string): Promise<string> {
  const list = await dbList(root).catch(() => []);
  return list.find((c) => c.id === conn)?.name ?? conn;
}

async function run(root: string, conn: string, view: EditorView) {
  const sel = view.state.selection.main;
  const text = view.state.doc.toString();
  const stmts = sel.empty ? [statementAt(text, sel.head)].filter((s) => s !== null) : statementsIn(text, sel.from, sel.to);
  if (!stmts.length) {
    notify.hint("光标处没有可执行的语句");
    return;
  }
  view.dispatch({ effects: setFlash.of({ from: stmts[0].from, to: stmts[stmts.length - 1].to }) });
  setTimeout(() => view.dispatch({ effects: setFlash.of(null) }), 700);
  const [name, { results }] = await Promise.all([nameOf(root, conn), import("../db/results.svelte")]);
  await results.run(root, conn, name, stmts.map((s) => s.sql));
}

function history(root: string, conn: string, view: EditorView) {
  void Promise.all([nameOf(root, conn), import("../db/HistoryPopup.svelte")]).then(([name, m]) => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const close = () => {
      unmount(app);
      host.remove();
      view.focus();
    };
    const app = mount(m.default, {
      target: host,
      props: {
        conn,
        connName: name,
        onclose: close,
        // 选中了一段：换掉它。没选中、光标那行是空的：插在光标处。那行有字：插在那行后面、隔一个空行 ——
        // 直接插在光标处会和那行原来的语句粘成一条（`select 1select 2`），⌘↵ 发出去的是两条拼起来的错 SQL。
        // 光标落在插进去的那段末尾：接着 ⌘↵ 执行的就是它
        onpick: (sql: string) => {
          const r = view.state.selection.main;
          const line = view.state.doc.lineAt(r.head);
          const glue = r.empty && line.text.trim() !== "";
          const at = glue ? line.to : r.from;
          const insert = glue ? `\n\n${sql}` : sql;
          view.dispatch({ changes: { from: at, to: glue ? at : r.to, insert }, selection: { anchor: at + insert.length }, scrollIntoView: true });
        },
      },
    });
  });
}

/** 顶上那一条：连的哪个库、执行、历史、结果（同 `.http` 的运行环境条，岛里的工具条 34 高） */
function bar(root: string, conn: string) {
  return (view: EditorView): Panel => {
    const dom = document.createElement("div");
    dom.className = "cm-sql-bar";
    const who = document.createElement("span");
    who.className = "who";
    who.textContent = "数据库控制台";
    void nameOf(root, conn).then((n) => (who.textContent = n));
    const btn = (label: string, title: string, f: () => void) => {
      const b = document.createElement("button");
      b.className = "btn sm quiet";
      b.textContent = label;
      b.title = title;
      b.onclick = f;
      return b;
    };
    const keys = document.createElement("span");
    keys.className = "keys";
    keys.textContent = "⌘↵ 执行光标所在的语句";
    dom.append(
      who,
      btn("执行", "执行光标所在的语句，选中时执行所选语句（⌘↵）", () => void run(root, conn, view)),
      btn("历史", "此连接执行过的语句", () => history(root, conn, view)),
      btn("结果", "显示查询结果工具窗", () => {
        layout.panelView = "db";
        layout.panel = true;
      }),
      keys,
    );
    return { dom, top: true };
  };
}

export function sqlConsole(root: string | null, path: string): Extension {
  const conn = project.dbConsole(path);
  if (!root || !conn) return [];
  return [
    flashField,
    showPanel.of(bar(root, conn)),
    Prec.high(
      idKeymap([
        {
          id: "db-run",
          preventDefault: true,
          run: (view) => {
            void run(root, conn, view);
            return true;
          },
        },
      ]),
    ),
    EditorView.baseTheme({
      ".cm-sql-bar": { display: "flex", alignItems: "center", gap: "4px", height: "34px", padding: "0 10px", borderBottom: "1px solid var(--border-soft)", fontFamily: "var(--ui-font)", fontSize: "var(--fs-sm)", color: "var(--text-dim)" },
      ".cm-sql-bar .who": { marginRight: "6px", color: "var(--text)", fontWeight: "600", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
      ".cm-sql-bar .keys": { marginLeft: "auto", color: "var(--text-faint)", fontSize: "var(--fs-xs)", whiteSpace: "nowrap" },
      ".cm-sql-running": { background: "color-mix(in srgb, var(--accent) 22%, transparent)" },
    }),
  ];
}
