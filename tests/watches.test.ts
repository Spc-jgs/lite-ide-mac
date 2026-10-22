/**
 * 监视的表达式（`debug/watches.ts`，docs/TAKEOVER.md G14）：去两头空白、去重、空的不要、最多 WATCH_MAX 个；存的不是数组（手改坏了）当成没有
 */
import { WATCH_MAX, clean } from "../src/lib/debug/watches.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

ok(JSON.stringify(clean([" cents ", "cents", "", "  ", "order.id"])) === '["cents","order.id"]', `去空白、去重、空的不要：${JSON.stringify(clean([" cents ", "cents", "", "  ", "order.id"]))}`);
ok(clean(Array.from({ length: 50 }, (_, i) => `x${i}`)).length === WATCH_MAX, "最多 WATCH_MAX 个（每停一次都要在 JVM 里全算一遍）");
ok(clean("cents").length === 0 && clean(null).length === 0 && clean([1, {}, "a"]).join() === "a", "不是字符串数组的（手改坏的）：认得的留下");

console.log(`${fail === 0 ? "✅" : "❌"} 监视：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
