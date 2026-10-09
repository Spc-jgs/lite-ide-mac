// 没打开的文件的符号（src/lib/editor/outline.ts 的 outlineOfText，issue #43 的 `文件@符号`）。
// 验收第 4 条「Python、TS、Java 各试一次 @」在这里：符号怎么认是 outlineOf 那一套，这里验的是「不经过编辑器也解析得出来」
import { java } from "@codemirror/lang-java";
import { python } from "@codemirror/lang-python";
import { javascript } from "@codemirror/lang-javascript";
import { outlineOfText } from "../src/lib/editor/outline.ts";

let pass = 0,
  fail = 0;
const eq = (a: unknown, b: unknown, m: string) => {
  if (JSON.stringify(a) === JSON.stringify(b)) pass++;
  else {
    fail++;
    console.error(`  ✗ ${m}：${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
  }
};
const names = (syms: { name: string; line: number }[]) => syms.map((s) => `${s.name}:${s.line}`);

const JAVA = `package com.demo.api;

public class AdminController {
    private final OrderClient orderClient;

    public String status() {
        return "ok";
    }
}
`;
eq(names(outlineOfText(JAVA, java())), ["AdminController:3", "orderClient:4", "status:6"], "Java：类、字段、方法，行号从 1 起");

const PY = `import os

class Repo:
    def find(self, key):
        return key

def main():
    pass
`;
eq(names(outlineOfText(PY, python())), ["Repo:3", "find:4", "main:7"], "Python：类、方法、函数");

const TS = `export interface Order { id: string }
export function listMember(o: Order) { return o.id; }
class Cart {
  total() { return 0; }
}
`;
eq(
  names(outlineOfText(TS, javascript({ typescript: true }))),
  ["Order:1", "listMember:2", "Cart:3", "total:4"],
  "TS：接口、函数、类、方法 —— 方法只列一次，接口里的属性名不算符号",
);
eq(names(outlineOfText("class A {\n  x = 1;\n  run() {}\n}\n", javascript())), ["A:1", "x:2", "run:3"], "JS：类字段算「属性」，方法只列一次");

eq(outlineOfText("x".repeat(2_000_001), java()), [], "超过上限的不解析");

// 量一下：5000 行的 Java 解析 + 抠符号要多久（file-symbols 在 `@` 后面每个字都可能问一次，命中缓存之前就是这个数）
const big = "public class Big {\n" + Array.from({ length: 1250 }, (_, i) => `    public int m${i}(int a) {\n        return a + ${i};\n    }\n`).join("") + "}\n";
const t0 = performance.now();
const syms = outlineOfText(big, java());
const ms = performance.now() - t0;
eq(syms.length, 1251, "5000 行：1 个类 + 1250 个方法，一个不少（没被 300ms 的解析上限截断）");
console.log(`  （5000 行 Java：${ms.toFixed(1)}ms，${syms.length} 个符号）`);

console.log(`${fail === 0 ? "✅" : "❌"} 没打开的文件的符号：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
