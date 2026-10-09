// 跨文件替换浮层的纯函数（src/lib/search/replace-model.ts，#42 第 3 步）
import { applyEdits, buildPicks, counts, fileState, flatten, hitKey, inlineDiff, toggleFile, visibleRange } from "../src/lib/search/replace-model.ts";

let pass = 0,
  fail = 0;
const eq = (a: unknown, b: unknown, m: string) => {
  if (JSON.stringify(a) === JSON.stringify(b)) pass++;
  else {
    fail++;
    console.error(`  ✗ ${m}：${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
  }
};

const hit = (line: number, col: number) => ({ line, col, text: "", spans: [] as [number, number][], lines: 1, block: null as string | null });
const scan = {
  files: [
    { rel: "A.java", path: "/p/A.java", editor: false, hits: [hit(1, 1), hit(2, 1)] },
    { rel: "B.java", path: "/p/B.java", editor: false, hits: [hit(1, 1), hit(2, 5)] },
    { rel: "C.java", path: "/p/C.java", editor: true, hits: [hit(3, 2)] },
  ],
  skipped: [],
  binary: 0,
  total: 5,
  truncated: false,
  indexTruncated: false,
};

// ── 勾选 → picks（验收 1：六处里取消一处，改五处）──
eq(buildPicks(scan, new Set()), [
  { rel: "A.java", hits: [0, 1] },
  { rel: "B.java", hits: [0, 1] },
  { rel: "C.java", hits: [0] },
], "全勾：每个文件每一处");
const off = new Set([hitKey("B.java", hit(1, 1))]);
eq(buildPicks(scan, off), [
  { rel: "A.java", hits: [0, 1] },
  { rel: "B.java", hits: [1] },
  { rel: "C.java", hits: [0] },
], "B 的第一处取消了：只带第二处，下标是原来的 1");
eq(counts(scan, off), { hits: 4, files: 3 }, "按钮上的数");
eq(buildPicks(scan, new Set([hitKey("C.java", hit(3, 2))])).map((p) => p.rel), ["A.java", "B.java"], "一处都没勾的文件不带");

// ── 文件那一行的复选框 ──
eq(fileState(scan.files[1], off), "some", "勾了一部分：半勾");
eq(fileState(scan.files[0], off), "all", "全勾");
const t1 = toggleFile(scan.files[1], off);
eq(fileState(scan.files[1], t1), "all", "半勾点一下：全勾");
const t2 = toggleFile(scan.files[1], t1);
eq(fileState(scan.files[1], t2), "none", "全勾点一下：全不勾");
eq(t2.has(hitKey("A.java", hit(1, 1))), false, "别的文件不受影响");

// ── 编辑落到文本上：从后往前，下标不歪 ──
eq(applyEdits("OrderClient a; OrderClient b;", [
  { from: 0, to: 11, insert: "OrderGateway" },
  { from: 15, to: 26, insert: "OrderGateway" },
]), "OrderGateway a; OrderGateway b;", "两处都换对（从前往后改的话第二处会歪一个字）");
eq(applyEdits("😀 OLD", [{ from: 3, to: 6, insert: "NEW" }]), "😀 NEW", "UTF-16 下标：😀 是两个单位");
eq(applyEdits("abc", []), "abc", "没有编辑：原样");

// ── 摊平 + 虚拟滚动 ──
eq(flatten(scan, new Set()).length, 3 + 5, "三个文件头 + 五处");
eq(flatten(scan, new Set(["B.java"])).map((r) => r.kind).join(","), "file,hit,hit,file,file,hit", "折叠的文件只剩文件头");
// 跨行的命中：点开之后拆成改前 / 改后两块，一行一行（行高固定，虚拟滚动才能用乘法定位）
{
  const ml = { ...scan, files: [{ rel: "M.java", path: "/p/M.java", editor: false, hits: [{ ...hit(2, 5), lines: 2, block: "    @Autowired\n    private Repo r;" }] }] };
  const after = [[{ text: "", spans: [] as [number, number][], block: "    private final Repo r;" }]];
  eq(flatten(ml, new Set(), new Set(), after).length, 2, "没点开：文件头 + 一行");
  const open = flatten(ml, new Set(), new Set([hitKey("M.java", hit(2, 5))]), after);
  eq(
    open.map((r) => (r.kind === "block" ? `${r.side}${r.text.trim()}` : r.kind)),
    ["file", "hit", "-@Autowired", "-private Repo r;", "+private final Repo r;"],
    "点开：改前两行、改后一行",
  );
}
eq(visibleRange(1000, 24, 0, 240), [0, 18], "顶上：可见 10 行 + 下面多画 8 行");
eq(visibleRange(1000, 24, 2400, 240), [92, 118], "滚到第 100 行：上下各多画 8 行");
eq(visibleRange(5, 24, 0, 240), [0, 5], "不够一屏：全画");

// ── 一行里的「改前 → 改后」──
eq(
  inlineDiff({ text: "    private OrderClient c;", spans: [[12, 23]] }, { text: "    private OrderGateway c;", spans: [[12, 24]] }),
  { pre: "private ", old: "OrderClient", neu: "OrderGateway", post: " c;" },
  "缩进去掉，旧的和新的各一段，后文取改后那一行的",
);
eq(inlineDiff({ text: "a OLD b", spans: [[2, 5]] }, undefined).neu, null, "改后还没到：不画新的");
const longPre = inlineDiff({ text: "x".repeat(60) + "OLD", spans: [[60, 63]] }, { text: "x".repeat(60) + "NEW", spans: [[60, 63]] });
eq([longPre.pre.length, longPre.pre[0]], [29, "…"], "前文太长收成 … + 28 个字");
eq(inlineDiff({ text: "OLD", spans: [[0, 3]] }, { text: "", spans: [[0, 0]] }), { pre: "", old: "OLD", neu: "", post: "" }, "换成空串：新的是空的");

console.log(`${fail === 0 ? "✅" : "❌"} 替换浮层的纯函数：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
