// Java 补色：注解（标签层，java-highlight.ts）+ 声明（ViewPlugin，java-semantic.ts）。
// 注解断言的是**最终颜色**而不是标签：标签打对了但主题里没有对应规则，照样是白的 ——
// 2026-09-23 之前就是反过来死的（主题有注解色，语法包不产出 annotation）。
import { javaLanguage } from "@codemirror/lang-java";
import { highlightTree, tags as t } from "@lezer/highlight";
import { javaStyleTags } from "../src/lib/editor/java-highlight.ts";
import { javaMarks } from "../src/lib/editor/java-scope.ts";
import { minimapHighlighter, HIGHLIGHT_SPEC } from "../src/lib/editor/theme-idea-dark.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const YELLOW = "#b3ae60";

const src = `package com.a;
@Service
@RequestMapping("/orders")
@org.springframework.stereotype.Component
public class OrderService {
    private static final int MAX_RETRY = 3;
    private OrderRepository repo, other;
    public OrderService(OrderRepository repo) { this.repo = repo; }
    @Override
    public Order persist(Order order) {
        int n = order.count;
        for (Item it : order.items) {}
        return repo.findById(order.id);
    }
    enum Color { RED, GREEN }
    record Pt(int x, int y) {}
    interface Api { int LIMIT = 1; void go(); }
}`;

const parser = javaLanguage.configure({ props: [javaStyleTags] }).parser;
const tree = parser.parse(src);

// ── 注解：标签层 ──
const spans: { text: string; color: string }[] = [];
highlightTree(tree, minimapHighlighter, (from, to, color) => spans.push({ text: src.slice(from, to), color }));
const color = (w: string) => spans.find((s) => s.text === w)?.color;

ok(color("@Service") === YELLOW, `注解连 @ 一起黄：${JSON.stringify(spans.filter((s) => s.text.includes("Service")))}`);
ok(color("@RequestMapping") === YELLOW, `带参数的注解名黄：${JSON.stringify(spans.slice(0, 6))}`);
ok(color('"/orders"') !== YELLOW, "注解参数里的字符串保持字符串色");
ok(
  color("@org.springframework.stereotype.Component") === YELLOW,
  `带包名的注解整段黄：${JSON.stringify(spans.filter((s) => s.text.includes("Component")))}`,
);
ok(color("@Override") === YELLOW, "方法上的注解也黄");

// ── 声明（第 1 步）；使用处的细节在 java-scope.test.ts ──
const marks = javaMarks(tree, (a, z) => src.slice(a, z), 0, src.length);
const kinds = (w: string) => marks.filter((m) => src.slice(m.from, m.to) === w).map((m) => m.kind);
const posOf = (w: string, nth = 0) => {
  let i = -1;
  for (let k = 0; k <= nth; k++) i = src.indexOf(w, i + 1);
  return i;
};
const kindAt = (w: string, nth: number) => marks.find((m) => m.from === posOf(w, nth))?.kind;

ok(kinds("persist").join() === "fnDecl", `方法声明：${kinds("persist")}`);
ok(kindAt("OrderService", 0) === undefined, "类名不上色");
ok(kindAt("OrderService", 1) === "fnDecl", `构造器声明同方法：${kindAt("OrderService", 1)}`);
ok(kindAt("repo", 0) === "field" && kinds("other").join() === "field", `字段声明（一行两个）：${kindAt("repo", 0)} ${kinds("other")}`);
ok(kinds("MAX_RETRY").join() === "staticField", `static final 字段：${kinds("MAX_RETRY")}`);
ok(kinds("RED").join() === "staticField" && kinds("GREEN").join() === "staticField", `枚举常量：${kinds("RED")}`);
ok(kinds("LIMIT").join() === "staticField", `接口常量天生 static：${kinds("LIMIT")}`);
ok(kinds("go").join() === "fnDecl", `接口方法声明：${kinds("go")}`);
ok(kinds("order").length === 0, `参数不上色：${kinds("order")}`);
ok(kinds("n").length === 0 && kinds("it").length === 0, "局部变量 / for 变量不上色");
ok(kinds("Pt").length === 0, `record 名不当方法（@lezer/java 把它解析成方法）：${kinds("Pt")}`);
// 构造器参数 repo 遮住同名字段：参数声明和 `= repo` 都不紫，`this.repo` 紫
ok(kindAt("repo", 1) === undefined && kindAt("repo", 3) === undefined, `构造器参数不紫：${kindAt("repo", 1)} ${kindAt("repo", 3)}`);
ok(kindAt("repo", 2) === "field", `this.repo 紫：${kindAt("repo", 2)}`);
ok(kindAt("repo", 4) === "field", `方法里的 repo 是字段：${kindAt("repo", 4)}`);

// 只走给定区间：第 10 行之后的 enum 不该出现在前半段的结果里
const half = javaMarks(tree, (a, z) => src.slice(a, z), 0, posOf("persist"));
ok(half.every((m) => m.from < posOf("persist")) && !half.some((m) => src.slice(m.from, m.to) === "RED"), "只返回区间里的");

// ── 主题：三种声明用的标签都有颜色规则 ──
const has = (tag: unknown) => HIGHLIGHT_SPEC.some((r) => ([] as unknown[]).concat(r.tag).includes(tag));
ok(has(t.constant(t.propertyName)), "static 字段有自己的规则");
const constRule = HIGHLIGHT_SPEC.find((r) => ([] as unknown[]).concat(r.tag).includes(t.constant(t.propertyName)));
ok(constRule?.fontStyle === "italic", "static 字段是斜体");
ok(has(t.definition(t.function(t.variableName))), "方法声明有规则");

console.log(`Java 补色：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
