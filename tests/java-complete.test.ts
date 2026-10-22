/**
 * Java 补全里不碰编辑器的那一半（src/lib/editor/java-complete.ts）：snippet 语法、种类、行列换偏移、排序加分。
 */
import { Text } from "@codemirror/state";
import { boostOf, kindType, lspSnippet, offsetOf, splitEdits } from "../src/lib/editor/java-complete.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

// jdtls 实测的两种（guessMethodArguments 关着 / 开着）原样能用
eq(lspSnippet("add(${0});"), "add(${0});", "带花括号的 ${0} 不动");
eq(lspSnippet("add(${1:name}, ${2:count});"), "add(${1:name}, ${2:count});", "带名字的占位不动");
// 省掉花括号的：CM6 不认，原样会把 $0 插进代码
eq(lspSnippet("println($1)$0"), "println(${1})${0}", "$1 / $0 补上花括号");
eq(lspSnippet("for (int $1 = 0; $1 < $2; $1++) {\n\t$0\n}"), "for (int ${1} = 0; ${1} < ${2}; ${1}++) {\n\t${0}\n}", "同一个占位出现几次都换");
// 选择项取第一个
eq(lspSnippet("${1|public,private|} void"), "${1:public} void", "选择项取第一个当默认");
// 转义：\$ 是字面的 $，不是占位
eq(lspSnippet("cost \\$1"), "cost $1", "\\$1 是字面量，去掉反斜杠、不变成占位");
eq(lspSnippet("plain"), "plain", "没有占位的原样");
eq(lspSnippet("$1$0"), "${1}${0}", "挨着的两个占位都换（按「前面一个字符不是反斜杠」去匹配会吃掉第二个前面那个字，漏换）");

eq([2, 4, 5, 7, 8, 13, 14, 21, 99].map(kindType), ["method", "method", "property", "class", "interface", "enum", "keyword", "constant", "text"], "种类换成图标");

const doc = Text.of(["package demo;", "", "class 甲 {", "}"]);
eq(offsetOf(doc, 0, 13), 13, "第 0 行第 13 列");
eq(offsetOf(doc, 2, 7), doc.line(3).from + 7, "中文按 UTF-16 数（JS 下标同一套）");
eq(offsetOf(doc, 0, 99), 13, "列越界夹到行尾");
eq(offsetOf(doc, 9, 0), doc.length, "行越界夹到文末");

ok(boostOf(0, 50) === 20 && boostOf(49, 50) === -20 && boostOf(0, 1) === 0, `加分在 ±20 之间、第一名最高：${boostOf(0, 50)} / ${boostOf(49, 50)}`);
ok(boostOf(3, 50) > boostOf(4, 50) || boostOf(3, 50) === boostOf(4, 50), "名次靠前的不比后面的低");

// resolve 回来的改动：import 在上面先做；后缀补全那笔删除（从表达式开头到光标）和补全范围重叠，并进去一起换
const ed = (line: number, character: number, endLine: number, endCharacter: number, text: string) => ({ line, character, endLine, endCharacter, text });
const imp = ed(1, 0, 1, 0, "import java.util.List;\n");
// `        name.length().var`：var 在 22..25，表达式从 8 开始
const del = ed(5, 8, 5, 25, "");
eq(splitEdits([imp, del], 5, 22, 25), { before: [imp], span: [8, 25] }, "import 先做；后缀补全的删除并进补全范围（从表达式开头换到 var 末尾）");
// 光标在词中间（`va|r`）：补全范围只到光标，jdtls 那笔删到词尾 —— 照删得多的那头换，不留下半截 `r`
eq(splitEdits([del], 5, 22, 24), { before: [], span: [8, 25] }, "光标在词中间：并进来的范围取到删除的末尾");
eq(splitEdits([ed(5, 8, 5, 22, "")], 5, 22, 25), { before: [ed(5, 8, 5, 22, "")], span: null }, "同一行、在补全起点之前结束的：先做，不并");
eq(splitEdits([ed(5, 8, 5, 25, "x")], 5, 22, 25), { before: [], span: null }, "重叠但不是删除的：不做（不知道该怎么合）");
eq(splitEdits([ed(5, 26, 5, 30, "")], 5, 22, 25), { before: [], span: null }, "补全后面的：不做");
eq(splitEdits([ed(6, 0, 6, 3, "")], 5, 22, 25), { before: [], span: null }, "下面的行：不做");

console.log(`${fail === 0 ? "✅" : "❌"} Java 补全的转换：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
