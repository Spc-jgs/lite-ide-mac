/**
 * 合并视图的纯函数（`src/lib/logview/merge-path.ts`）：标签路径的编解、短名、偏移的读写、点图例。
 */
import {
  mergePath,
  mergePaths,
  shortNames,
  mergeTitle,
  parseOffset,
  fmtOffset,
  toggleSource,
  allSources,
  clockNote,
} from "../src/lib/logview/merge-path.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：得到 ${JSON.stringify(a)}，要 ${JSON.stringify(b)}`);

// 路径：来回一致；同样几份是同一个；路径里有换行的不让合（会和分隔符混）
{
  const ps = ["/srv/a.log", "/srv/b.log"];
  const p = mergePath(ps)!;
  eq(mergePaths(p), ps, "来回一致");
  ok(p.startsWith("merge:"), "前缀是 tabflow 认的那个");
  eq(mergePath(["/a\nb.log", "/c.log"]), null, "路径带换行不合");
  eq(mergePaths("/srv/a.log"), null, "普通路径不是合并路径");
  eq(mergePaths("merge:\n/only.log"), null, "只有一份不算");
}

// 短名：去后缀；同名的带父目录（同一个服务的几台机器）
eq(shortNames(["/x/order-service.log", "/x/gateway.log"]), ["order-service", "gateway"], "去掉 .log");
eq(shortNames(["/x/app.log.1", "/x/gc.out"]), ["app", "gc"], "轮转后缀、.out");
eq(shortNames(["/dl/10.0.0.1/app.log", "/dl/10.0.0.2/app.log", "/dl/gw.log"]), ["10.0.0.1/app", "10.0.0.2/app", "gw"], "同名带父目录");
eq(shortNames(["/x/README"]), ["README"], "没后缀原样");

eq(mergeTitle(["a", "b"]), "a + b", "两份");
eq(mergeTitle(["a", "b", "c", "d", "e"]), "a + b + 另 3 份", "多了折起来");

// 偏移：读
eq(parseOffset("+8h"), 8 * 3_600_000, "+8h");
eq(parseOffset("-350ms"), -350, "-350ms");
eq(parseOffset("−350ms"), -350, "U+2212 的负号也认（fmtOffset 自己写出去的就是它）");
eq(parseOffset("1m30s"), 90_000, "不带正负号是正的、多段相加");
eq(parseOffset(" +1h 30m "), 5_400_000, "中间有空格");
eq(parseOffset("0"), 0, "0");
eq(parseOffset(""), 0, "空串当 0");
eq(parseOffset("8"), null, "没单位不猜");
eq(parseOffset("abc"), null, "读不懂");
eq(parseOffset("+8hx"), null, "尾巴上有别的");
// 写：和读来回一致
for (const ms of [8 * 3_600_000, -350, 90_000, -5_400_250, 1]) eq(parseOffset(fmtOffset(ms)), ms, `来回 ${ms}`);
eq(fmtOffset(0), "", "0 不印");
eq(fmtOffset(-350), "−350ms", "负号是 U+2212");

// 点图例：同级别胶囊的手感
{
  const n = 3, all = allSources(n);
  eq(all, 0b111, "三份全看");
  eq(toggleSource(all, 1, n), 0b010, "全亮时点一份 = 只看它");
  eq(toggleSource(0b010, 1, n), all, "只剩它亮着再点 = 复原");
  eq(toggleSource(0b010, 0, n), 0b011, "否则翻转那一份");
  eq(toggleSource(0b011, 0, n), 0b010, "翻掉一份");
  eq(allSources(32), 0xffff_ffff, "32 份不溢出成负数");
  eq(toggleSource(allSources(32), 31, 32), 0x8000_0000, "第 32 份的位是正数");
}

eq(clockNote("timeOnly", "2026-08-24"), "只有时分秒，按 2026-08-24 对齐", "只有时分秒");
eq(clockNote("noYear", "2026-08-24"), "没有年份，按 2026 年对齐", "没有年");
eq(clockNote("full", "2026-08-24"), "", "全的不说");

console.log(`${fail === 0 ? "✅" : "❌"} 合并视图的纯函数：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
