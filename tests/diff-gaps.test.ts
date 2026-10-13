/**
 * 差异视图「展开 N 行未改动」（`gapLines` / `withGaps`，2026-10-13）：从新侧全文取那几行、行号两边都对；
 * 全文和差异对不上（拉差异之后文件又改了）就不展开，不能插错的行进来。
 */
import { parseDiff, gapLines, withGaps } from "../src/lib/git/diff.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// 新侧全文：第 n 行就是 "L{n}"；第 10、11 行被改过（旧侧是 old10）
const full = Array.from({ length: 30 }, (_, i) => `L${i + 1}`);
const raw = `diff --git a/x b/x
--- a/x
+++ b/x
@@ -8,4 +8,5 @@ fn a() {
 L8
 L9
-old10
+L10
+L11
 L12
@@ -20,2 +21,3 @@ fn b() {
 L21
+L22
 L23
`;
const lines = parseDiff(raw)[0].lines;
const hs = lines.filter((l) => l.kind === "hunk");
ok(hs[0].hidden === 7 && hs[1].hidden === 8, `前提：两块上面各折着 7 / 8 行，实际 ${hs[0].hidden} / ${hs[1].hidden}`);

// 第一块上面：新 1..7、旧 1..7
const g0 = gapLines(lines, 0, full)!;
ok(g0.length === 7 && g0[0].text === "L1" && g0[6].text === "L7", "第一段：L1..L7");
ok(g0[0].newNo === 1 && g0[0].oldNo === 1 && g0[6].newNo === 7 && g0[6].oldNo === 7, "第一段行号：新旧都是 1..7");
ok(g0.every((l) => l.kind === "ctx"), "补进来的都是上下文行（不能被选去暂存）");

// 第二块上面：新 13..20，旧 12..19（第一块里多了一行）
const g1 = gapLines(lines, 1, full)!;
ok(g1.length === 8 && g1[0].text === "L13" && g1[7].text === "L20", `第二段：L13..L20，实际 ${g1[0]?.text}..${g1[7]?.text}`);
ok(g1[0].newNo === 13 && g1[0].oldNo === 12, `第二段第一行：新 13 / 旧 12，实际 ${g1[0].newNo} / ${g1[0].oldNo}`);

// 对不上：文件在拉差异之后又改了（第 21 行不是 L21 了）→ 不展开
const changed = [...full];
changed[20] = "改过了";
ok(gapLines(lines, 1, changed) === null, "全文和差异对不上：不展开（null），不插错的行");
ok(gapLines(lines, 0, full.slice(0, 5)) === null, "全文比要的短：不展开");
ok(gapLines(lines, 0, full.map((s) => s + "\r"))?.[0].text === "L1", "CRLF 的全文：行尾 \\r 不算差别、也不带进来");

// 插回去：块头前面多出那几行，块头不再说「N 行未改动」；没展开的原样
const merged = withGaps(lines, new Map([[1, g1]]));
const h1 = merged.findIndex((l) => l.kind === "hunk" && l.text === "fn b() {");
ok(merged[h1 - 1].text === "L20" && merged[h1 - 8].text === "L13", "第二段插在第二个块头前面");
ok(merged[h1].hidden === 0, "展开了的块头 hidden 归零");
ok(merged.filter((l) => l.kind === "hunk")[0].hidden === 7, "没展开的块头照旧");
ok(merged.includes(lines[2]), "原来的行是同一个对象（按行暂存靠对象认行，展开不能把它们换掉）");
ok(withGaps(lines, new Map()) === lines, "没展开：原样返回");

console.log(`${fail === 0 ? "✅" : "❌"} 展开未改动的行：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
