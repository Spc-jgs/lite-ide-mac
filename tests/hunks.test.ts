import { splitHunks, hunkPatch } from "../src/lib/git/hunks.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const raw = `diff --git a/x.txt b/x.txt
index 1a2b3c4..5d6e7f8 100644
--- a/x.txt
+++ b/x.txt
@@ -1,3 +1,3 @@
 c1
-old
+new
 c3
@@ -10,2 +10,3 @@ fn foo
 c10
+added
 c11
`;
{
  const p = splitHunks(raw);
  ok(p.hunks.length === 2, `两块，实得 ${p.hunks.length}`);
  ok(!p.header.includes("index "), "index 行要扔掉");
  ok(p.header === "diff --git a/x.txt b/x.txt\n--- a/x.txt\n+++ b/x.txt\n", `文件头：${JSON.stringify(p.header)}`);
  ok(p.hunks[0] === "@@ -1,3 +1,3 @@\n c1\n-old\n+new\n c3\n", `第一块原文：${JSON.stringify(p.hunks[0])}`);
  ok(p.hunks[1].startsWith("@@ -10,2 +10,3 @@ fn foo\n") && p.hunks[1].endsWith(" c11\n"), `第二块：${JSON.stringify(p.hunks[1])}`);
  ok(hunkPatch(p, 1) === p.header + p.hunks[1], "单块 patch = 文件头 + 那一块");
  ok(hunkPatch(p, 5) === "", "越界给空串");
}
// 多文件只取第一个
{
  const two = raw + "diff --git a/y.txt b/y.txt\n--- a/y.txt\n+++ b/y.txt\n@@ -1 +1 @@\n-a\n+b\n";
  const p = splitHunks(two);
  ok(p.hunks.length === 2 && !p.header.includes("y.txt"), "第二个文件不要");
}
ok(splitHunks("").hunks.length === 0 && hunkPatch(splitHunks(""), 0) === "", "空输入");
// 没有 diff --git 头（比如 --no-index 的怪输出）：没有文件头就不给 patch
ok(hunkPatch(splitHunks("@@ -1 +1 @@\n-a\n+b\n"), 0) === "", "没有文件头不给 patch");

console.log(`${fail === 0 ? "✅" : "❌"} 按块拆 patch：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
