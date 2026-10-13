// 草稿本恢复时哪些标签不再摆回标签栏（src/lib/state/scratchbook.ts）：只挑「三天没改过的草稿」，没保存的、钉住的、不是草稿的、拿不准的一律留着
import { keepInScratchbook, SCRATCHBOOK_KEEP_DAYS } from "../src/lib/state/scratchbook.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const now = Date.parse("2026-10-13T10:00:00+08:00");
const day = 24 * 3600 * 1000;
const isScratch = (p: string) => p.startsWith("/s/");
/** 盘上修改时间表：路径 → 多久以前改的 */
const m = (ago: number | undefined) => () => (ago === undefined ? undefined : now - ago);

ok(!keepInScratchbook({ path: "/s/a.md"  }, now, isScratch, m(4 * day)), "四天没改的草稿：收起");
ok(keepInScratchbook({ path: "/s/a.md"  }, now, isScratch, m(2 * day)), "两天前改过：留着");
ok(!keepInScratchbook({ path: "/s/a.md"  }, now, isScratch, m(SCRATCHBOOK_KEEP_DAYS * day)), "正好三天：收起（「三天没改过」）");
ok(keepInScratchbook({ path: "/s/a.md", draft: "没存的字"  }, now, isScratch, m(9 * day)), "有没保存的内容：再旧也留着");
ok(keepInScratchbook({ path: "/s/a.md", pinned: true  }, now, isScratch, m(9 * day)), "钉住的留着");
ok(keepInScratchbook({ path: "/logs/app.log"  }, now, isScratch, m(9 * day)), "不是草稿（草稿本里开着的日志）留着");
ok(keepInScratchbook({ path: "/s/a.md" }, now, isScratch, m(undefined)), "列表里没有它：拿不准就留着");
ok(keepInScratchbook({ path: "/s/a.md" }, now, isScratch, () => 0), "取不到修改时间（0）：留着");

console.log(`${fail === 0 ? "✅" : "❌"} 草稿本收起旧标签：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
