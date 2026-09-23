// 编辑器键位照 IDEA（src/lib/editor/idea-keys.ts）。测的是命令做了 IDEA 那件事，
// 以及「CM6 默认的那个意外」不会再发生（⌥⇧↓ 复制行、⌥↑ 挪行、⌘[ 减缩进）。
import { EditorSelection, EditorState, type Transaction } from "@codemirror/state";
import { javaLanguage } from "@codemirror/lang-java";
import {
  duplicate,
  extendSelection,
  shrinkSelection,
  selectAllOccurrences,
  toggleCase,
  ideaKeymap,
  editorDefaults,
  ideaKeysState,
} from "../src/lib/editor/idea-keys.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

type Cmd = (t: { state: EditorState; dispatch: (tr: Transaction) => void }) => boolean;
/** 跑一条命令，返回之后的 state */
function run(cmd: Cmd, st: EditorState): EditorState {
  let out = st;
  cmd({ state: st, dispatch: (tr) => (out = tr.state) });
  return out;
}
const mk = (doc: string, anchor: number, head = anchor) =>
  EditorState.create({
    doc,
    selection: EditorSelection.single(anchor, head),
    extensions: [javaLanguage, ideaKeysState, EditorState.allowMultipleSelections.of(true)],
  });
const selText = (st: EditorState) => st.sliceDoc(st.selection.main.from, st.selection.main.to);

// ── ⌘D ──
{
  const st = run(duplicate, mk("a\nbcd\ne", 3));
  ok(st.doc.toString() === "a\nbcd\nbcd\ne", `没有选区复制当前行：${JSON.stringify(st.doc.toString())}`);
  ok(st.doc.lineAt(st.selection.main.head).number === 3, "光标落到下面那份");
  const s2 = run(duplicate, mk("foo(bar)", 4, 7));
  ok(s2.doc.toString() === "foo(barbar)", `有选区复制选区：${s2.doc.toString()}`);
  ok(selText(s2) === "bar" && s2.selection.main.from === 7, "选中新贴的那份");
}

// ── ⌥↑ / ⌥↓ ──
{
  const doc = "class A { void m() { call(order.id); } }";
  let st = mk(doc, doc.indexOf("order") + 2);
  st = run(extendSelection, st);
  ok(selText(st) === "order", `第一下选中光标下的词：${selText(st)}`);
  st = run(extendSelection, st);
  ok(selText(st) === "order.id", `再扩一层是语法树的父节点：${selText(st)}`);
  st = run(extendSelection, st);
  ok(selText(st).startsWith("(") && selText(st).includes("order.id"), `再扩：${selText(st)}`);
  st = run(shrinkSelection, st);
  ok(selText(st) === "order.id", `缩小回到上一步：${selText(st)}`);
  st = run(shrinkSelection, st);
  st = run(shrinkSelection, st);
  ok(st.selection.main.empty && st.selection.main.head === doc.indexOf("order") + 2, "缩到底回到最初的光标");
  const before = st.doc.toString();
  const again = run(shrinkSelection, st);
  ok(again.doc.toString() === before, "栈空了再按 ⌥↓ 什么都不做（不能落到默认的挪行）");
  // 命令都返回 true：返回 false 会让 CM6 继续往下找，落到 defaultKeymap 的 ⌥↑ = 挪行
  ok(shrinkSelection({ state: st, dispatch: () => {} }) === true, "⌥↓ 总是吃掉这一下");
  // 光标自己动了，栈作废
  let s3 = run(extendSelection, mk(doc, doc.indexOf("call") + 1));
  s3 = s3.update({ selection: EditorSelection.cursor(0) }).state;
  const s4 = run(shrinkSelection, s3);
  ok(s4.selection.main.head === 0, "光标挪过之后，⌥↓ 不跳回旧选区");
}

// ── ⌃⌘G ──
{
  const st = run(selectAllOccurrences, mk("id + id + idx + id", 1));
  ok(st.selection.ranges.length === 3, `没有选区时按光标下的词选全部（整词，不含 idx）：${st.selection.ranges.length}`);
}

// ── ⇧⌘U ──
{
  const a = run(toggleCase, mk("hello World", 0, 11));
  ok(a.doc.toString() === "HELLO WORLD", `有小写 → 全大写：${a.doc.toString()}`);
  const b = run(toggleCase, mk("HELLO", 0, 5));
  ok(b.doc.toString() === "hello", "全大写 → 全小写");
  const c = run(toggleCase, mk("max_retry = 1", 2));
  ok(c.doc.toString() === "MAX_RETRY = 1" && c.selection.main.empty && c.selection.main.head === 2, `没有选区作用于光标下的词、光标不动：${c.doc.toString()}`);
}

// ── 键位表本身 ──
{
  const keys = ideaKeymap.map((b) => b.key);
  for (const k of ["Mod-d", "Mod-Backspace", "Shift-Alt-ArrowUp", "Shift-Alt-ArrowDown", "Alt-ArrowUp", "Alt-ArrowDown", "Ctrl-g", "Ctrl-Mod-g", "Alt-Mod-/", "Shift-Mod-u"]) {
    ok(keys.includes(k), `IDEA 键在表里：${k}`);
  }
  // 默认键位里原来的意外：⌥⇧↓ = copyLineDown。IDEA 那条必须排在它前面才压得住 —— 这里验的是
  // 默认表里确实有这条（否则这份测试是在防一个不存在的东西），以及 ⌘[ ⌘] 被摘掉了
  ok(editorDefaults.some((b) => b.key === "Shift-Alt-ArrowDown"), "默认表里确实有 ⌥⇧↓（复制行），所以 IDEA 那条必须排在前面");
  ok(!editorDefaults.some((b) => b.key === "Mod-[" || b.key === "Mod-]"), "⌘[ ⌘] 从默认键位里摘掉了，让给回退 / 前进");
}

console.log(`IDEA 编辑键：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
