/**
 * 覆盖率在编辑器里（docs/TAKEOVER.md G15）：报告里这个文件的三种行各画一种颜色；报告换了 / 清掉了，开着的编辑器跟着变；总的行覆盖率算对。
 */
const { EditorState } = await import("@codemirror/state");
const { gutterLineClass } = await import("@codemirror/view");
const cov = await import("../src/lib/state/coverage");
const { coverage } = await import("../src/lib/editor/coverage-ext");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const P = "/p/src/main/java/Calc.java";
cov.setCoverage("m [cover Calc]", { files: [{ path: P, covered: [2, 3], partial: [4], missed: [5] }, { path: "/p/B.java", covered: [], partial: [], missed: [1, 2] }], total: 2 });
let st = EditorState.create({ doc: "a\nb\nc\nd\ne\nf\n", extensions: [coverage(P)] });
const marks = () => {
  const out: string[] = [];
  for (const set of st.facet(gutterLineClass)) for (const it = set.iter(); it.value; it.next()) out.push(`${st.doc.lineAt(it.from).number}:${(it.value as { elementClass: string }).elementClass.replace("cm-cov cm-cov-", "")}`);
  return out.join(",");
};
ok(marks() === "2:hit,3:hit,4:part,5:miss", `三种行三种颜色：${marks()}`);
st = st.update({ changes: { from: 0, insert: "new\n" } }).state;
ok(marks() === "3:hit,4:hit,5:part,6:miss", `上面插一行，颜色跟着往下：${marks()}`);

const t = cov.totalPct(cov.lastCoverage()!.files);
ok(t.hit === 3 && t.all === 6 && Math.abs(t.pct - 0.5) < 1e-9, `总的行覆盖率：${t.hit} / ${t.all}`);
ok(cov.filePct({ covered: [], partial: [], missed: [] }) === 1, "没有可执行行的文件算 100%");

// 清掉：开着的编辑器跟着收（编辑器里靠 ViewPlugin 订阅；这里直接看表）
cov.clearCoverage();
ok(cov.coverageOf(P) === undefined && cov.lastCoverage() === null, "清掉之后表里没了");

console.log(`${fail === 0 ? "✅" : "❌"} 覆盖率（编辑器）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
