import { findDiffSegments, diffLineKind } from "../src/lib/git/diff.ts";

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

console.log(`${fail === 0 ? "✅" : "❌"} diff 段：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
