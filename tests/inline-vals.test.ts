/**
 * 行内变量值挑哪几行写什么（`debug/inline-vals.ts`，docs/TAKEOVER.md G14）：只在给的范围里；整词；点号后面的不算；字符串、注释里的不算；
 * 同一行同一个只写一次、按出现次序；this 不写；长的截断
 */
import { VAL_MAX, inlineVals } from "../src/lib/debug/inline-vals.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const src = [
  "public String show(long cents) {", // 1
  '    String s = String.format("cents=%d", cents / 100);', // 2
  "    // cents 在注释里", // 3
  "    long centsTotal = order.cents + cents;", // 4
  "    return s + s;", // 5
  "}", // 6
  "    s = cents;", // 7：范围外
];
const vals = [
  { name: "cents", value: "12345" },
  { name: "s", value: '"CNY 123.45"' },
  { name: "this", value: "Money@21" },
  { name: "centsTotal", value: "x".repeat(100) },
];
const m = inlineVals(src, 1, 5, vals);
ok(m.get(1) === "cents = 12345", `方法头：参数：${m.get(1)}`);
ok(m.get(2) === 's = "CNY 123.45", cents = 12345', `按出现次序、字符串里的 cents 不算：${m.get(2)}`);
ok(!m.has(3), "注释里的不算");
ok(m.get(4)?.startsWith("centsTotal = xxx") && m.get(4)!.includes(", cents = 12345") && m.get(4)!.split(", ")[0].length === "centsTotal = ".length + VAL_MAX, `整词（centsTotal 不当 cents）、order.cents 不算、长的截断：${m.get(4)}`);
ok(m.get(5) === 's = "CNY 123.45"', "同一行同一个只写一次");
ok(!m.has(7) && !m.has(6), "范围外的不写（还没跑到的行）");
ok(inlineVals(src, 1, 5, [{ name: "this", value: "x" }]).size === 0, "this 不写");
ok(inlineVals(["    long t = order.cents;"], 1, 1, vals).size === 0, "只在点号后面出现（order.cents 是字段）：不写");

console.log(`${fail === 0 ? "✅" : "❌"} 行内变量值：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
