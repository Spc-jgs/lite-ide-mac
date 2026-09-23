// 切标签接回撤销历史（src/lib/editor/undo-store.ts）。
// 「丢数据形状」：切走再切回，⌘Z 撤不回切走前打的字；以及反过来 —— 文档变了还硬接历史，撤到错位置。
import { EditorState, type Transaction } from "@codemirror/state";
import { history, undo } from "@codemirror/commands";
import { saveHistory, stateWithHistory } from "../src/lib/editor/undo-store.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const config = { extensions: [history()] };
const type = (st: EditorState, at: number, text: string) =>
  st.update({ changes: { from: at, insert: text }, selection: { anchor: at + text.length }, userEvent: "input.type" }).state;
const doUndo = (st: EditorState) => {
  let out = st;
  undo({ state: st, dispatch: (tr: Transaction) => (out = tr.state) });
  return out;
};

// 打字 → 切走（存）→ 切回（新 state，新扩展）→ ⌘Z 撤得回
{
  let st = EditorState.create({ ...config, doc: "hello" });
  st = type(st, 5, " world");
  saveHistory("/p/A.java", st);
  const back = stateWithHistory("/p/A.java", "hello world", config);
  ok(back !== st, "是新建的 state，不是把旧对象塞回去（旧扩展里的闭包指着已销毁的组件）");
  const undone = doUndo(back);
  ok(undone.doc.toString() === "hello", `切回来之后 ⌘Z 撤得回：${JSON.stringify(undone.doc.toString())}`);
  ok(back.selection.main.head === 11, `选区也接回来了：${back.selection.main.head}`);
}

// 切走期间文件被外部改了：不接历史
{
  let st = EditorState.create({ ...config, doc: "abc" });
  st = type(st, 3, "d");
  saveHistory("/p/B.java", st);
  const back = stateWithHistory("/p/B.java", "totally different", config);
  ok(back.doc.toString() === "totally different", "显示的是新的内容");
  ok(doUndo(back).doc.toString() === "totally different", "文档对不上 → 历史扔掉，⌘Z 什么都不撤");
}

// 取过就删：同一份历史不会被第二个编辑器再拿一次
{
  let st = EditorState.create({ ...config, doc: "x" });
  st = type(st, 1, "y");
  saveHistory("/p/C.java", st);
  stateWithHistory("/p/C.java", "xy", config);
  const second = stateWithHistory("/p/C.java", "xy", config);
  ok(doUndo(second).doc.toString() === "xy", "第二次拿不到（分屏里同一文件开两份时不串）");
}

// 别的文件的历史不串过来
{
  let st = EditorState.create({ ...config, doc: "same" });
  st = type(st, 4, "!");
  saveHistory("/p/D.java", st);
  const other = stateWithHistory("/p/E.java", "same!", config);
  ok(doUndo(other).doc.toString() === "same!", "按路径认，不按内容认");
}

console.log(`撤销历史接续：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
