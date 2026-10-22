/**
 * 同名高亮（`editor/java-usage-hl.ts`，docs/TAKEOVER.md G11）的纯的那半：jdtls 的行列 → 文档偏移（列按 UTF-16、越界的丢掉、排好序）；
 * 光标还在已经标出来的某一处里就不重问；只在贴着标识符时才问
 */
import { Text } from "@codemirror/state";
import { inSpot, onWord, spotsOf } from "../src/lib/editor/java-usage-hl";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const hl = (line: number, character: number, endCharacter: number, write = false) => ({ line, character, endLine: line, endCharacter, write });

const doc = Text.of(["class A {", "  int count = 0;", "  void f() { count++; String s = \"😀\" + count; }", "}"]);
const s = spotsOf(doc, [hl(2, 40, 45), hl(1, 6, 11, true), hl(2, 13, 18)]);
ok(s.length === 3 && s[0].write && s.map((x) => x.from).every((f, i, a) => i === 0 || a[i - 1] < f), `按位置排好、写的那处带着：${JSON.stringify(s)}`);
ok(doc.sliceString(s[0].from, s[0].to) === "count", "第 2 行的 count");
// 😀 在 UTF-16 里占两个单位，jdtls 的列也是按 UTF-16 数的 —— 正好是 JS 字符串下标，不用换算
ok(doc.sliceString(s[2].from, s[2].to) === "count", `emoji 后面那个 count 没错位：${doc.sliceString(s[2].from, s[2].to)}`);
ok(spotsOf(doc, [hl(9, 0, 3), hl(1, 6, 99), hl(1, 6, 6)]).length === 0, "越界的行、越界的列、空的：都丢掉（问的时候文档可能已经变了）");

ok(inSpot(s, s[1].from) && inSpot(s, s[1].to), "名字的两头也算在里面（光标在名字末尾很常见）");
ok(!inSpot(s, s[1].to + 1), "出了名字就不算");

ok(onWord(doc, doc.line(2).from + 8), "名字中间");
ok(onWord(doc, doc.line(2).from + 11), "名字末尾（右边是空格，左边是字）");
ok(!onWord(doc, doc.line(2).from + 13), "两个空格之间、= 旁边：不问");

console.log(`${fail === 0 ? "✅" : "❌"} 同名高亮：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
