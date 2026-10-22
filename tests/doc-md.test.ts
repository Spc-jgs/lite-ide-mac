/**
 * 快速文档的 Markdown → 安全的几块（`editor/doc-md.ts`）：jdtls 转出来的那几种结构认得；链接只留字；认不出的照原文当字
 */
import { docBlocks, spansOf } from "../src/lib/editor/doc-md.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const eq = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

// jdtls 对一个带 Javadoc 的方法回的样子（hover_markdown 拼好之后）
const md = "```java\nlong com.demo.PriceRule.apply(long cents)\n```\n\n按规则算出**折后价**（分），见 [Money](jdt://contents/x.class)。\n第二行接着同一段。\n\n* **Parameters:**\n  * **cents** 原价\n* **Returns:**\n  * 折后价";
const b = docBlocks(md);
ok(b[0].kind === "code" && b[0].text === "long com.demo.PriceRule.apply(long cents)", "代码块：签名原样");
ok(
  b[1].kind === "para" && eq(b[1].spans, [{ t: "按规则算出" }, { t: "折后价", bold: true }, { t: "（分），见 Money。 第二行接着同一段。" }]),
  `段落：粗体认出来、链接只留字、相邻两行并成一段：${JSON.stringify(b[1])}`,
);
ok(b[2].kind === "item" && b[2].depth === 0 && eq(b[2].spans, [{ t: "Parameters:", bold: true }]), "列表项：Parameters");
ok(b[3].kind === "item" && b[3].depth === 1 && eq(b[3].spans, [{ t: "cents", bold: true }, { t: " 原价" }]), `嵌套一层的列表项：${JSON.stringify(b[3])}`);
ok(b.length === 6, `一共 6 块（代码、段落、四个列表项）：${b.length}`);

ok(eq(spansOf("用 `List<String>` 装"), [{ t: "用 " }, { t: "List<String>", code: true }, { t: " 装" }]), "行内代码");
ok(eq(spansOf("这是 *斜体* 字"), [{ t: "这是 " }, { t: "斜体" }, { t: " 字" }]), `斜体：去掉星号、不加样子：${JSON.stringify(spansOf("这是 *斜体* 字"))}`);
ok(spansOf("a \\* b").map((s) => s.t).join("") === "a * b", "转义的星号还原成字");
ok(spansOf("<script>alert(1)</script>").map((s) => s.t).join("") === "<script>alert(1)</script>", "像 HTML 的照原文当字（画的时候走文本插值）");
ok(eq(docBlocks(""), []), "空的");
ok(eq(docBlocks("```\n没闭合的代码块"), [{ kind: "code", text: "没闭合的代码块" }]), "没闭合的代码块：到末尾为止，不丢字");

console.log(`${fail === 0 ? "✅" : "❌"} 快速文档的 Markdown：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
