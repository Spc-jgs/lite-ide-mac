import { changedLines } from "../src/lib/git/diff.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// 改：删一行加一行
const a = changedLines(`diff --git a/x b/x
@@ -1,3 +1,3 @@
 c1
-old
+new
 c3
`);
ok(a.size === 1 && a.get(2) === "mod", `改动应标在第 2 行 mod，实得 ${JSON.stringify([...a])}`);

// 加：纯新增
const b = changedLines(`diff --git a/x b/x
@@ -1,2 +1,4 @@
 c1
+n1
+n2
 c2
`);
ok(b.get(2) === "add" && b.get(3) === "add" && b.size === 2, `纯新增：${JSON.stringify([...b])}`);

// 删：纯删除，标在缺口下一行
const c = changedLines(`diff --git a/x b/x
@@ -1,4 +1,2 @@
 c1
-gone1
-gone2
 c2
`);
ok(c.size === 1 && c.get(2) === "del", `纯删除应标在缺口处第 2 行，实得 ${JSON.stringify([...c])}`);

// 文件开头就删
const d = changedLines(`diff --git a/x b/x
@@ -1,3 +1,1 @@
-gone
-gone2
 keep
`);
ok(d.get(1) === "del", `开头删除标第 1 行，实得 ${JSON.stringify([...d])}`);

// 混合：多个 hunk
const e = changedLines(`diff --git a/x b/x
@@ -1,3 +1,3 @@
 c1
-a
+b
 c2
@@ -20,3 +20,4 @@
 d1
+added
 d2
`);
ok(e.get(2) === "mod" && e.get(21) === "add", `多 hunk：${JSON.stringify([...e])}`);

// 新文件：整份都是 add
const f = changedLines(`diff --git a/n b/n
new file mode 100644
@@ -0,0 +1,3 @@
+l1
+l2
+l3
`);
ok(f.size === 3 && [...f.values()].every(v => v === "add"), `新文件全 add：${JSON.stringify([...f])}`);

// 二进制不产生标记
ok(changedLines(`diff --git a/i.png b/i.png
Binary files a/i.png and b/i.png differ
`).size === 0, "二进制无标记");

ok(changedLines("").size === 0, "空 diff");

console.log(`\n${fail === 0 ? "✅" : "❌"} 改动行标记：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
