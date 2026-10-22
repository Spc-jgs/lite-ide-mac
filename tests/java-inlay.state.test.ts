/**
 * 内嵌提示（`editor/java-inlay.ts`，docs/TAKEOVER.md G11）的纯的那半：jdtls 的行列 → 文档偏移（越界的丢掉、排好序）；问哪一截（看得见的上下各多 20 行，夹在文档里）
 */
import { Text } from "@codemirror/state";
import { hintsOf, linesAround } from "../src/lib/editor/java-inlay";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const doc = Text.of(["class A {", "  long f() { return rule.apply(100, \"VIP\"); }", "}"]);
const l2 = doc.line(2);
const h = hintsOf(doc, [
  { line: 1, character: 36, label: "tag:", param: true },
  { line: 1, character: 31, label: "cents:", param: true },
  { line: 7, character: 0, label: "x:", param: true },
  { line: 1, character: 999, label: "y:", param: true },
]);
ok(h.length === 2, `越界的行、越界的列丢掉：${h.length}`);
ok(h[0].label === "cents:" && h[0].pos === l2.from + 31 && doc.sliceString(h[0].pos, h[0].pos + 3) === "100", `按位置排好、落在参数开头：${JSON.stringify(h)}`);
ok(doc.sliceString(h[1].pos, h[1].pos + 5) === '"VIP"', "第二个参数开头");

const big = Text.of(Array.from({ length: 500 }, (_, i) => `line ${i}`));
ok(JSON.stringify(linesAround(big, big.line(101).from, big.line(140).to)) === "[80,160,0]", `看得见 101–140 行：问第 80 行开头到第 160 行开头：${JSON.stringify(linesAround(big, big.line(101).from, big.line(140).to))}`);
ok(JSON.stringify(linesAround(big, 0, big.line(10).to)) === "[0,30,0]", "顶上夹到 0");
// 到了末尾：终点是最后一行（从 0 数第 499 行）的行尾，不是「第 500 行第 0 列」—— 那一行不存在，jdtls 整段回空
ok(JSON.stringify(linesAround(big, big.line(490).from, big.length)) === "[469,499,8]", `底下：最后一行的行尾：${JSON.stringify(linesAround(big, big.line(490).from, big.length))}`);
const small = Text.of(["a", "bb", "ccc"]);
ok(JSON.stringify(linesAround(small, 0, small.length)) === "[0,2,3]", "整个文件都看得见（小文件，就是验收撞上的那种）");

console.log(`${fail === 0 ? "✅" : "❌"} 内嵌提示：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
