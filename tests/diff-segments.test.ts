import { findDiffSegments, diffLineKind, linePieces } from "../src/lib/git/diff.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const D = [
  "diff --git a/src/a.ts b/src/a.ts",
  "index 1111111..2222222 100644",
  "--- a/src/a.ts",
  "+++ b/src/a.ts",
  "@@ -1,3 +1,3 @@ fn main",
  " const a = 1;",
  "-const b = 2;",
  "+const b = 3;",
];

const s1 = findDiffSegments(["笔记", ...D, "", "然后呢"]);
ok(s1.length === 1 && s1[0].from === 1 && s1[0].to === 8, `diff --git 开段、空行断段：${JSON.stringify(s1)}`);
ok(s1[0].kinds.join(",") === "meta,meta,meta,meta,hunk,ctx,del,add", `每行种类：${s1[0].kinds}`);

const s2 = findDiffSegments(["@@ -10,2 +10,2 @@", "-old", "+new", "prose"]);
ok(s2.length === 1 && s2[0].to === 2, `光有 @@ 也开段（git diff 复制一半的情形）：${JSON.stringify(s2)}`);

ok(findDiffSegments(["--- a/x", "+++ b/x", "-old", "+new"]).length === 0, "`--- ` 开头不开段（正文里太常见）");
ok(findDiffSegments(["diff --git a/x b/x", "index 1..2", "Binary files differ"]).length === 0, "没有 hunk 的不算");
ok(findDiffSegments(["- 列表项", "- 又一项", "+ 加号"]).length === 0, "markdown 列表不是 diff");
ok(findDiffSegments([]).length === 0, "空输入");

// 两段
const s3 = findDiffSegments([...D, "", "@@ -5 +5 @@", "-x", "+y"]);
ok(s3.length === 2 && s3[1].from === D.length + 1, `空行隔开的是两段：${JSON.stringify(s3.map((s) => [s.from, s.to]))}`);

ok(diffLineKind("\\ No newline at end of file") === "meta", "No newline 是 meta");
ok(diffLineKind("") === null, "空行不是 diff 行");


// ── linePieces：语法段 × 行内改动区间 ──
{
  const l = { kind: "add" as const, text: "int timeout = 5000;", span: [14, 18] as [number, number] };
  const toks = [
    { t: "int", cls: "kw" },
    { t: " timeout = ", cls: "" },
    { t: "5000", cls: "num" },
    { t: ";", cls: "" },
  ];
  const p = linePieces(l, toks);
  ok(p.map((x) => x.t).join("") === l.text, "拼回来必须是原串");
  ok(p.filter((x) => x.hit).map((x) => x.t).join("") === "5000", `改动区间应该是 5000，实得 ${JSON.stringify(p.filter((x) => x.hit))}`);
  ok(p.find((x) => x.t === "5000")?.cls === "num", "改动区间里的语法类名要保住");
  ok(p.find((x) => x.t === "int")?.cls === "kw" && !p.find((x) => x.t === "int")?.hit, "区间外的段不带 hit");
  // 区间切在一个语法段中间：段被切开，两半类名一样、hit 不一样
  const q = linePieces({ kind: "del" as const, text: "foobar", span: [3, 6] }, [{ t: "foobar", cls: "id" }]);
  ok(q.length === 2 && q[0].t === "foo" && !q[0].hit && q[1].t === "bar" && q[1].hit && q[1].cls === "id", "区间切在段中间要切开");
  // 没有语法信息：退化成三段
  const r = linePieces({ kind: "add" as const, text: "ab😀cd", span: [2, 3] }, null);
  ok(r.filter((x) => x.hit).map((x) => x.t).join("") === "😀", "span 是 code point 计的，emoji 算一个字");
  ok(linePieces({ kind: "ctx" as const, text: "x" }, null).length === 1, "没有 span 就是一整段");
}

console.log(`${fail === 0 ? "✅" : "❌"} diff 段：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
