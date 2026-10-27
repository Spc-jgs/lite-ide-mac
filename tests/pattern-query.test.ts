/**
 * 日志归类：点一类 → 过滤条件（`src/lib/logview/pattern-query.ts`）。条件要用过滤框自己的切词（`query.ts`）切回来对得上，
 * 而且拿模板生成的那几行去匹配要全中 —— 不然点了一类、筛出来是空的。
 */
import { templateQuery, templateParts, templateShown, changeLabel } from "../src/lib/logview/pattern-query.ts";
import { parseQuery, matchLine } from "../src/lib/logview/query.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：得到 ${JSON.stringify(a)}，要 ${JSON.stringify(b)}`);

const T = "<*>-<*>-<*> <*>:<*>:<*>.<*> INFO [http-nio-<*>-exec-<*>] c.d.order.OrderController - 下单 userId=<*> sku=A-<*> qty=<*> traceId=<*>";
const q = templateQuery(T);
eq(q, '"[http-nio-" "-exec-" "c.d.order.OrderController" "userId=" "sku=A-" "traceId="', "最长的六个常量词加引号、按原先后");
// 生成这一类的真实行：都要命中（**原样的行**，级别补齐成五格的那两个空格不许替它抹掉）；换一个类的行不命中
const line = "2026-08-24 10:15:02.130 INFO  [http-nio-8080-exec-2] c.d.order.OrderController - 下单 userId=1024 sku=A-77 qty=2 traceId=7f3a9c";
ok(matchLine(line, parseQuery(q), false), "这一类的行命中（原行里 INFO 后面是两个空格）");
ok(!matchLine("2026-08-24 10:15:02.155 DEBUG [http-nio-8080-exec-2] c.d.order.OrderService - 预占库存", parseQuery(q), false), "别的类不命中");
eq(parseQuery(q).bad ?? false, false, "过滤框认这个语法");

// 以 - 开头、以 / 开头的段：加了引号就不会变成「排除」和「正则」
const q2 = templateQuery("<*> -verbose /api/orders <*>");
const pq = parseQuery(q2);
ok(pq.include.length === 2 && pq.exclude.length === 0, `-verbose 不能变成排除：${q2} → ${JSON.stringify(pq)}`);
ok(matchLine("x -verbose /api/orders y", pq, false), "命中原行");

// 段里有引号：拆开、带引号的词不要（过滤框的引号里不能再有引号）
eq(templateQuery('msg="boom" code=<*> retry later'), '"code=" "retry" "later"', "带引号的词丢掉");
// 全是变量 / 只有标点：没有条件（界面说「这一类全是变量，筛不出来」）
eq(templateQuery("<*> <*>:<*> <*>"), "", "全是变量");
// 太多段：留最长的六段，先后不变
const many = Array.from({ length: 10 }, (_, i) => `key${"x".repeat(i)}=<*>`).join(" ");
const qm = templateQuery(many);
eq(parseQuery(qm).include.length, 6, "最多六个词");
ok(qm.indexOf("keyxxxxxxxxx=") > qm.indexOf("keyxxxx="), "按原来的先后");

eq(templateParts("a <*> b<*>"), [{ text: "a ", wild: false }, { text: "<*>", wild: true }, { text: " b", wild: false }, { text: "<*>", wild: true }], "切成常量 / 变量");

eq(templateShown(T), "INFO [http-nio-<*>-exec-<*>] c.d.order.OrderController - 下单 userId=<*> sku=A-<*> qty=<*> traceId=<*>", "列表里去掉开头的时间戳形状");
eq(templateShown("<*> <*>"), "<*>", "全是没字的词：至少留一个");
eq(templateShown("[<*>] boom"), "boom", "方括号包着的序号也算没字");

// 对比的说法：按频率不按次数（今天的日志长了一倍，次数翻倍不算涨）
eq(changeLabel(3, 0, 100, 100), { text: "新出现", kind: "new" }, "基准没有");
eq(changeLabel(0, 5, 100, 100), { text: "已消失", kind: "gone" }, "这边没有");
eq(changeLabel(200, 100, 2000, 1000), { text: "持平", kind: "flat" }, "次数翻倍、总行数也翻倍：持平");
eq(changeLabel(80, 10, 283, 115), { text: "×3.3", kind: "up" }, "涨了三倍多");
eq(changeLabel(100, 100, 400, 200), { text: "÷2.0", kind: "down" }, "次数没动、总行数翻倍：频率降一半");
eq(changeLabel(900, 10, 1000, 1000), { text: "×90", kind: "up" }, "十倍以上不带小数");

console.log(`${fail === 0 ? "✅" : "❌"} 归类 → 过滤条件：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
