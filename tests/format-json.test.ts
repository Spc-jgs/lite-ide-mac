/**
 * ⌥⌘L 格式化 JSON（`src/lib/editor/format-json.ts`）。最要紧的是「值一个字节都不动」：
 * 大整数（订单号、雪花 ID）和键的顺序 —— `JSON.stringify(JSON.parse(...))` 那种写法两样都会悄悄改掉。
 */
import { EditorState, EditorSelection } from "@codemirror/state";
import { prettyJson, jsonSpanAt, formatJsonChanges } from "../src/lib/editor/format-json.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

/** 在 `doc` 上（光标在 `|` 处，或者选中 `[` `]` 之间）跑一次，返回改完的全文 / 错误 / same */
function run(marked: string, unit = "  "): string {
  let doc = marked, sel = EditorSelection.single(0);
  const a = marked.indexOf("["), b = marked.indexOf("]", a);
  if (marked.includes("|")) {
    const p = marked.indexOf("|");
    doc = marked.slice(0, p) + marked.slice(p + 1);
    sel = EditorSelection.single(p);
  } else if (marked.startsWith("SEL:")) {
    doc = marked.slice(4).replace("[", "").replace("]", "");
    sel = EditorSelection.single(a - 4, b - 5);
  }
  const state = EditorState.create({ doc, selection: sel });
  const r = formatJsonChanges(state, unit);
  if ("error" in r) return "ERR:" + r.error;
  if ("same" in r) return "SAME";
  return state.update(r.spec).state.doc.toString();
}

// 值原样：大整数不丢精度、像整数的键不被挪到前面
{
  const src = '{"orderId":12345678901234567890,"b":1,"2":0,"amount":1.10}';
  const out = prettyJson(src);
  ok(out.includes("12345678901234567890"), `大整数原样：${out}`);
  ok(out.indexOf('"b"') < out.indexOf('"2"'), "键的顺序不动（像整数的键不该跑到前面）");
  ok(out.includes("1.10"), "1.10 不被改成 1.1");
}

// 形状：两格缩进、冒号后一个空格、空容器不展开、字符串里的括号逗号和转义不碰
{
  const out = prettyJson('{"a":[1,2],"e":{},"l":[],"s":"x,y:{z}\\"q","n":null}');
  const want = '{\n  "a": [\n    1,\n    2\n  ],\n  "e": {},\n  "l": [],\n  "s": "x,y:{z}\\"q",\n  "n": null\n}';
  ok(out === want, `形状对：\n${out}`);
  ok(prettyJson(out) === out, "再格式化一次不变");
}

// 草稿里「接口返回：{…}」这一行：从 { 起格式化，前缀留着
{
  const out = run('接口返回：{"code":0,"data":{"id":7}}|\n下一行');
  ok(out === '接口返回：{\n  "code": 0,\n  "data": {\n    "id": 7\n  }\n}\n下一行', `行内的 JSON：\n${out}`);
}

// 缩进了的行：后面的行跟着这一行的缩进
{
  const out = run('- 返回\n  {"a":1}|');
  ok(out === '- 返回\n  {\n    "a": 1\n  }', `跟着行首缩进：\n${out}`);
}

// ```json 围栏里，光标在哪一行都行
{
  const out = run('笔记\n```json\n{"a":1,|"b":[true]}\n```\n后面');
  ok(out === '笔记\n```json\n{\n  "a": 1,\n  "b": [\n    true\n  ]\n}\n```\n后面', `围栏里的：\n${out}`);
}

// 光标停在两条围栏线上也算：开头那行的行首、行中，收尾那行（review 2026-10-13：原来开头那行只认行尾）
{
  const want = '笔记\n```json\n{\n  "a": 1\n}\n```\n后面';
  ok(run('笔记\n|```json\n{"a":1}\n```\n后面') === want, "光标在 ```json 行首");
  ok(run('笔记\n```js|on\n{"a":1}\n```\n后面') === want, "光标在 ```json 行中");
  ok(run('笔记\n```json\n{"a":1}\n``|`\n后面') === want, "光标在收尾那行");
}

// 整份就是 JSON（.json 文件），用文件自己的缩进单位
{
  const out = run('|{"a":{"b":1}}\n', "    ");
  ok(out === '{\n    "a": {\n        "b": 1\n    }\n}\n', `整份，4 格：\n${out}`);
}

// 选区：只动选中的
{
  const out = run('SEL:x = [{"k":"v"}]; // 尾巴');
  ok(out === 'x = {\n  "k": "v"\n}; // 尾巴', `选中的那段：\n${out}`);
}

// 不猜：不合法的不改、说清楚；已经格式化好的说一声
{
  ok(run("SEL:[{'a':1}]").startsWith("ERR:选中的不是合法的 JSON"), "单引号不是 JSON");
  ok(run('{"a":1,}|').startsWith("ERR:光标这里没有 JSON"), "尾逗号不认");
  ok(run("普通的一行字|").startsWith("ERR:"), "没有 JSON 的行");
  ok(run('|{\n  "a": 1\n}') === "SAME", "已经是这个格式");
  ok(jsonSpanAt("[1, 2", 1) === null, "半截的数组不认");
}

console.log(`${fail === 0 ? "✅" : "❌"} 格式化 JSON：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
