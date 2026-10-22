/**
 * Java 代码模板的纯的那半（`editor/java-templates.ts`，docs/TAKEOVER.md G21）：光标前那个整词是不是模板；后缀补全前面那个表达式从哪儿开始
 */
import { POSTFIX, templateAt, exprStart } from "../src/lib/editor/java-templates.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

ok(templateAt("        sout")?.tpl.key === "sout" && templateAt("        sout")?.from === 8, "行首缩进后的 sout：是，从第 8 个字开始");
ok(templateAt("x = 1; fori")?.tpl.key === "fori", "分号后面的也算");
ok(templateAt("order.sout") === null, "点后面的是后缀补全，不是模板");
ok(templateAt("xsout") === null && templateAt("sout1") === null, "只认整个词");
ok(templateAt("        String s") === null, "不是模板的词");

const e = (t: string) => {
  const i = exprStart(t);
  return i < 0 ? null : t.slice(i);
};
ok(e("        order") === "order", "一个变量");
ok(e("    x = order.items().get(0)") === "order.items().get(0)", `点、调用、括号：${e("    x = order.items().get(0)")}`);
ok(e('    log(map.get("a)b")') === 'map.get("a)b")', `括号里字符串里的括号不算：${e('    log(map.get("a)b")')}`);
ok(e("    foo(a, b") === "b", "没配对的左括号：停在它后面");
ok(e("    ") === null && e("    x + ") === null, "点前面是空白：认不出");
ok(e("return list[i]") === "list[i]", "方括号");

const sout = POSTFIX.find((p) => p.key === "sout")!;
const soutv = POSTFIX.find((p) => p.key === "soutv")!;
ok(sout.wrap("order.total()") === "System.out.println(order.total());${}", "expr.sout");
ok(soutv.wrap('m.get("k")') === 'System.out.println("m.get(\\"k\\") = " + m.get("k"));${}', `soutv 的说明里引号转义：${soutv.wrap('m.get("k")')}`);

console.log(`${fail === 0 ? "✅" : "❌"} Java 代码模板：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
