/**
 * 日志过滤框的语法（`logview/query.ts`）。
 *
 * **这组用例和 `logengine/src/query.rs` 的测试是同一份**（切词那几条一一对应）：
 * 两边各实现一遍同一套语法，改了一边要改另一边，用例对不上就是漂了。
 */
import { parseQuery, matchLine, highlightQuery, isEmptyQuery, type Query } from "../src/lib/logview/query.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const show = (q: Query) =>
  [...q.include.map((t) => (t.kind === "re" ? `/${t.source}/` : t.text)), ...q.exclude.map((t) => "-" + (t.kind === "re" ? `/${t.source}/` : t.text))].join(" ");

// ── 切词（和 query.rs 一一对应）──

ok(show(parseQuery("a b")) === "a b" && parseQuery("a b").include.length === 2, "空格是 AND");
ok(show(parseQuery("  a   b  ")) === "a b", "多余空白不算");
ok(show(parseQuery("a -b")) === "a -b" && parseQuery("a -b").exclude.length === 1, "-b 是排除");
ok(show(parseQuery("-")) === "-", "单独一个 - 是字面量（日志里 - 分隔符多得是）");
ok(show(parseQuery('"x y" z')) === "x y z" && parseQuery('"x y" z').include[0].text === "x y", "引号里的空格不拆");
ok(parseQuery('"x y').include[0].text === "x y", "没闭合的引号：吃到末尾（正在打）");
ok(show(parseQuery("/a.b/ c")) === "/a.b/ c" && parseQuery("/a.b/ c").include[0].kind === "re", "斜杠是正则");
ok(show(parseQuery("-/a|b/")) === "-/a|b/", "-/re/ 是排除正则");
ok(show(parseQuery('"/x/"')) === "/x/" && parseQuery('"/x/"').include[0].kind === "lit", "引号包着的斜杠是字面量");
ok(parseQuery("/a b/").include[0].kind === "re" && (parseQuery("/a b/").include[0] as { source: string }).source === "a b", "正则里的空格不拆");
ok(isEmptyQuery(parseQuery("")) && isEmptyQuery(parseQuery("   ")) && isEmptyQuery(parseQuery('""')) && isEmptyQuery(parseQuery("//")), "空的几种写法都是空");
ok(parseQuery("/(/").bad && parseQuery("/(/").include.length === 0, "坏正则：标 bad，那一项不进条件");
ok(parseQuery("/(/ ok").bad && parseQuery("/(/ ok").include.length === 1, "坏正则不连累别的条件");
ok(parseQuery("/a/").include[0].kind === "re", "/a/ 是正则（最短的合法写法）");
ok(parseQuery("/").include[0].kind === "lit" && (parseQuery("/").include[0] as { text: string }).text === "/", "单独一个斜杠是字面量");

// ── 匹配 ──

const q = (s: string) => parseQuery(s);
ok(matchLine("ERROR order 8001 timeout", q("order timeout"), false), "AND：两个都在");
ok(!matchLine("ERROR order 8001", q("order timeout"), false), "AND：少一个就不中");
ok(!matchLine("INFO healthcheck ok timeout", q("timeout -healthcheck"), false), "排除：有就不中");
ok(matchLine("ERROR db timeout", q("timeout -healthcheck"), false), "排除：没有才中");
ok(matchLine("Order 1", q("order"), false) && !matchLine("Order 1", q("order"), true), "大小写跟着开关，正则也一样");
ok(matchLine("id=12345", q("/id=\\d+/"), false) && !matchLine("id=abc", q("/id=\\d+/"), false), "正则");
ok(matchLine("Boom", q("/b.o/"), false) && !matchLine("Boom", q("/b.o/"), true), "正则也跟大小写开关");
ok(matchLine("anything", q("/(/"), false), "坏正则被跳过，等于没这条");

// ── 高亮：奇数段是命中，和老的 highlight 同一个约定 ──

ok(JSON.stringify(highlightQuery("a order b order", q("order"), false)) === JSON.stringify(["a ", "order", " b ", "order", ""]), "两处命中");
ok(JSON.stringify(highlightQuery("x ORDER y", q("order"), false)) === JSON.stringify(["x ", "ORDER", " y"]), "不分大小写时原文保留");
ok(JSON.stringify(highlightQuery("id=12 ok", q("/\\d+/ ok"), false)) === JSON.stringify(["id=", "12", " ", "ok", ""]), "正则和字面量一起画");
ok(JSON.stringify(highlightQuery("abcd", q("ab bcd"), false)) === JSON.stringify(["", "abcd", ""]), "重叠的命中合成一段");
ok(JSON.stringify(highlightQuery("plain", q("-x"), false)) === JSON.stringify(["plain"]), "只有排除条件：不画");
ok(JSON.stringify(highlightQuery("baaa", q("/a*/"), false)) === JSON.stringify(["b", "aaa", ""]), "能匹配空串的正则：空匹配跳过，实匹配照画，不死循环");
ok(JSON.stringify(highlightQuery("xyz", q("/a*/"), false)) === JSON.stringify(["xyz"]), "全是空匹配就什么都不画");
ok(JSON.stringify(highlightQuery("a.b", q("."), false)) === JSON.stringify(["a", ".", "b"]), "字面量里的 . 不是通配");

console.log(`${fail === 0 ? "✅" : "❌"} 日志过滤语法：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
