/**
 * 编辑 settings.json 时的补全和行内标注（issue #44 第 4 步）。CM6 的 EditorState 在裸 node 里跑得起来（不需要 DOM），
 * 补全的键名走 `mock/settings.ts` 的 settings_schema。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { EditorState } = await import("@codemirror/state");
const { CompletionContext } = await import("@codemirror/autocomplete");
const { keySource, keyInsert, decorate } = await import("../src/lib/editor/settings-file");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

/** 在 `|` 处问一次补全 */
async function ask(textWithCursor: string) {
  const pos = textWithCursor.indexOf("|");
  const doc = textWithCursor.replace("|", "");
  const st = EditorState.create({ doc });
  return { doc, pos, r: await keySource(new CompletionContext(st, pos, true)) };
}

// ── 补全：只在行首写键的位置给 ──
{
  const { pos, r } = await ask('{\n  "edi|\n}');
  ok(r !== null && r.options.some((o) => o.label === '"editor.fontSize"'), "行首敲 \"edi：列出设置键");
  ok(r?.from === pos - 4, `连同开头的引号一起替换（from=${r?.from}，光标 ${pos}）`);
  ok((await ask('{\n  "editor.fontSize": 1|\n}')).r === null, "值的位置：不给（默认值插键时已经带上了）");
  ok((await ask('{\n  // "edi|\n}')).r === null, "注释里：不给");
  ok((await ask("{\n  |\n}")).r === null, "还没敲引号：不弹（不然每次回车都弹一个框）");
}

// ── 选了之后插什么 ──
{
  const d = { key: "editor.fontSize", default: "13" };
  // 括号自动配对补了收尾的引号：`"edi|"`
  const doc = '{\n  "edi"\n}';
  const from = doc.indexOf('"edi');
  const to = from + 4;
  const c = keyInsert(doc, from, to, d);
  const out = doc.slice(0, c.from) + c.insert + doc.slice(c.to);
  ok(out === '{\n  "editor.fontSize": 13\n}', `自动配对的那个收尾引号一起吃掉：${JSON.stringify(out)}`);
  const doc2 = '{\n  "edi\n}';
  const c2 = keyInsert(doc2, doc2.indexOf('"edi'), doc2.indexOf('"edi') + 4, d);
  const out2 = doc2.slice(0, c2.from) + c2.insert + doc2.slice(c2.to);
  ok(out2 === '{\n  "editor.fontSize": 13\n}', `没有自动配对时照样对：${JSON.stringify(out2)}`);
}

// ── 问题标在行上 ──
{
  const st = EditorState.create({ doc: '{\n  "editor.fontsize": 1\n}' });
  const deco = decorate(st.doc, [
    { text: "不认识的设置：editor.fontsize", line: 2, col: 3, fatal: false },
    { text: "最外层要是 {}", line: null, col: null, fatal: true },
    { text: "按存盘那份算的，编辑器里已经删了几行", line: 99, col: 1, fatal: false },
  ]);
  const lines: number[] = [];
  deco.between(0, st.doc.length, (from, _to, v) => {
    if (v.spec.class === "cm-setting-bad") lines.push(st.doc.lineAt(from).number);
  });
  ok(lines.join() === "1,2,3", `标在第 2 行；没有行号的标第 1 行；越界的夹到最后一行：${lines.join()}`);
}

console.log(`${fail === 0 ? "✅" : "❌"} 设置文件的补全和标注：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
