/**
 * 日志视图里的「JSON 文档」格式（src/lib/logview/parse.ts，#77）：`.http` 的响应体排好缩进后，键、字符串、数字、true/false/null 各一色。
 * 形状：把一行一条的 JSON 日志也认成文档（那种要按级别着色、要筛）；键当成字符串；字符串里像堆栈的被当成堆栈续行。
 */
import { detectFormat, parse } from "../src/lib/logview/parse.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

const doc = JSON.stringify({ id: 7, name: "订单 \"A\"", paid: true, note: null, items: [{ qty: -2.5e3 }], err: "Caused by: x" }, null, 2).split("\n");
eq(detectFormat(doc), "jsondoc", "排好缩进的 JSON 文档");
eq(detectFormat(JSON.stringify([1, 2], null, 2).split("\n")), "jsondoc", "数组开头的也算");
eq(detectFormat(['{"time":"2026-10-28T01:02:03Z","level":"error","msg":"boom"}', '{"time":"2026-10-28T01:02:04Z","level":"info","msg":"ok"}']), "json", "一行一条的 JSON 日志：还是日志");
eq(detectFormat(["{", "这是一段说明", "}"]), "plain", "开头一个 { 但下面不是 JSON 的：不认");

const cls = (line: string) => parse(line, "jsondoc").parts.map((p) => `${p.cls}:${p.text}`);
eq(cls('  "id": 7,'), ["msg:  ", 'jkey:"id"', "msg:: ", "jnum:7", "msg:,"], "键 / 数字 / 标点");
eq(cls('  "name": "订单 \\"A\\"",'), ["msg:  ", 'jkey:"name"', "msg:: ", 'jstr:"订单 \\"A\\""', "msg:,"], "带转义引号的字符串是一段");
eq(cls('  "paid": true,').slice(-2), ["jlit:true", "msg:,"], "true");
eq(cls('      "qty": -2.5e3').slice(-1), ["jnum:-2.5e3"], "负数、小数、指数");
const err = parse('  "err": "Caused by: x"', "jsondoc");
ok(!err.stack && err.parts.some((p) => p.cls === "jstr" && p.text === '"Caused by: x"'), "值里写着 Caused by: 的字符串：照样是一段字符串");
eq(parse("  {", "jsondoc").parts.map((p) => p.text).join(""), "  {", "原文一个字都不丢（搜索高亮按原文对位）");
for (const l of doc) ok(parse(l, "jsondoc").parts.map((p) => p.text).join("") === l, `拼回去和原文一样：${l}`);

console.log(`${fail === 0 ? "✅" : "❌"} JSON 文档着色：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
