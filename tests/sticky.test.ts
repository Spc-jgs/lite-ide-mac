/**
 * 粘性行要钉哪几行（src/lib/editor/sticky-scope.ts，docs/TAKEOVER.md G19）：用真的 Java 语法树（@codemirror/lang-java）。
 */
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree } from "@codemirror/language";
import { java } from "@codemirror/lang-java";
import { stickyLines } from "../src/lib/editor/sticky-scope.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

const src = [
  "package a;", //                                   1
  "public class Order {", //                         2
  "    private int total;", //                       3
  "    @Override", //                                4
  "    public String toString() {", //               5
  "        try {", //                                6
  "            if (total > 0) {", //                 7
  "                return \"x\";", //                8
  "            }", //                                9
  "        } catch (Exception e) {", //              10
  "            return \"y\";", //                    11
  "        }", //                                    12
  "        return \"z\";", //                        13
  "    }", //                                        14
  "}", //                                            15
].join("\n");
const st = EditorState.create({ doc: src, extensions: [java()] });
ensureSyntaxTree(st, st.doc.length, 5000);

eq(stickyLines(st, 3), [2], "字段那行：钉类那行");
eq(stickyLines(st, 8), [2, 5, 6, 7], "最里面：类、方法签名（不是 @Override 那行）、try、if");
eq(stickyLines(st, 11), [2, 5, 10], "catch 里：try 那层已经过去了，钉的是 catch 那行");
eq(stickyLines(st, 6), [2, 5], "头就是这一行的（try 自己露着）不钉");
eq(stickyLines(st, 8, 2), [6, 7], "超过上限留里面的");
eq(stickyLines(st, 1), [], "第一行上面没东西");
eq(stickyLines(st, 15), [2], "类的最后一行（只剩 `}`）：头在上面、身子到这一行为止 —— 照钉（IDEA 也是）");

// else 那一支里钉的是 `} else {` 那行
const ifs = EditorState.create({ doc: "class A {\n  void f() {\n    if (a) {\n      x();\n    } else {\n      y();\n      z();\n    }\n  }\n}", extensions: [java()] });
ensureSyntaxTree(ifs, ifs.doc.length, 5000);
eq(stickyLines(ifs, 4), [1, 2, 3], "if 那一支：钉 if 那行");
eq(stickyLines(ifs, 7), [1, 2, 5], "else 那一支：钉 `} else {` 那行");

// 别的东西（纯文本）没有语法树：什么都不钉
const plain = EditorState.create({ doc: "a\nb\nc" });
eq(stickyLines(plain, 3), [], "没有语法树");

console.log(fail === 0 ? `✅ 粘性行：${pass} 通过，0 失败` : `❌ 粘性行：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
