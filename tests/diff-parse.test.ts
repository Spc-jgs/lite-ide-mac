import { parseDiff, segs } from "../src/lib/git/diff.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) { pass++; } else { fail++; console.error("  ✗ " + m); } };

// 1. 普通改动：行号与增删计数
const d1 = parseDiff(`diff --git a/src/a.rs b/src/a.rs
index 1234567..89abcde 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,7 +10,8 @@ fn main() {
 let x = 1;
-    println!("hello");
+    println!("world");
+    println!("extra");
 let y = 2;
`);
ok(d1.length === 1, "应该是一个文件");
ok(d1[0].path === "src/a.rs", "路径 " + d1[0].path);
ok(d1[0].adds === 2 && d1[0].dels === 1, `增删 ${d1[0].adds}/${d1[0].dels}`);
const hunk = d1[0].lines.find(l => l.kind === "hunk")!;
ok(hunk.text === "fn main() {", "hunk 上下文函数名: " + hunk.text);
const first = d1[0].lines.find(l => l.kind === "ctx")!;
ok(first.oldNo === 10 && first.newNo === 10, `首行号 ${first.oldNo}/${first.newNo}`);
const del = d1[0].lines.find(l => l.kind === "del")!;
ok(del.oldNo === 11, "删除行的旧行号 " + del.oldNo);
const adds = d1[0].lines.filter(l => l.kind === "add");
ok(adds[0].newNo === 11 && adds[1].newNo === 12, `新增行号 ${adds[0].newNo}/${adds[1].newNo}`);
const last = d1[0].lines.filter(l => l.kind === "ctx").pop()!;
ok(last.oldNo === 12 && last.newNo === 13, `尾行号 ${last.oldNo}/${last.newNo}（删1增2后新旧要错开）`);

// 2. 行内高亮：1删1增，只标真正变的那截
const d2 = parseDiff(`diff --git a/x b/x
--- a/x
+++ b/x
@@ -1 +1 @@
-const timeout = 300;
+const timeout = 5000;
`);
const dl = d2[0].lines.find(l => l.kind === "del")!;
const al = d2[0].lines.find(l => l.kind === "add")!;
ok(!!dl.span && !!al.span, "应该配对出行内区间");
ok(segs(dl)[1] === "300", "删除侧高亮 = " + JSON.stringify(segs(dl)[1]));
ok(segs(al)[1] === "5000", "新增侧高亮 = " + JSON.stringify(segs(al)[1]));
ok(segs(dl)[0] === "const timeout = ", "公共前缀 = " + JSON.stringify(segs(dl)[0]));
ok(segs(dl)[2] === ";", "公共后缀 = " + JSON.stringify(segs(dl)[2]));

// 3. 数量不等就不猜
const d3 = parseDiff(`diff --git a/y b/y
@@ -1,2 +1,1 @@
-aaa
-bbb
+ccc
`);
ok(!d3[0].lines.some(l => l.span), "2删1增不该配对");

// 4. 整行都变了也不标（标了等于没标）
const d4 = parseDiff(`diff --git a/z b/z
@@ -1 +1 @@
-abc
+xyz
`);
ok(!d4[0].lines.some(l => l.span), "毫无公共部分时不标");

// 5. 新文件 / 删除文件 / 二进制 / 改名
const d5 = parseDiff(`diff --git a/n b/n
new file mode 100644
--- /dev/null
+++ b/n
@@ -0,0 +1 @@
+hi
diff --git a/old b/del
deleted file mode 100644
diff --git a/img.png b/img.png
index aaa..bbb 100644
Binary files a/img.png and b/img.png differ
diff --git a/from.txt b/to.txt
similarity index 92%
rename from from.txt
rename to to.txt
`);
ok(d5.length === 4, "四个文件，实得 " + d5.length);
ok(d5[0].isNew, "新文件标记");
ok(d5[1].isDeleted, "删除标记");
ok(d5[2].binary, "二进制标记");
ok(d5[3].path === "to.txt" && d5[3].oldPath === "from.txt", `改名 ${d5[3].oldPath}→${d5[3].path}`);

// 6. 带空格的路径
const d6 = parseDiff(`diff --git a/my dir/a b.md b/my dir/a b.md
@@ -1 +1 @@
-x
+y
`);
ok(d6[0].path === "my dir/a b.md", "带空格路径 = " + JSON.stringify(d6[0].path));

// 7. 无换行结尾的标记不能占行号
const d7 = parseDiff(`diff --git a/q b/q
@@ -1,2 +1,2 @@
 keep
-old
\\ No newline at end of file
+new
\\ No newline at end of file
`);
ok(d7[0].adds === 1 && d7[0].dels === 1, `\\ 标记不该算进增删：${d7[0].adds}/${d7[0].dels}`);

// 8. 空输入
ok(parseDiff("").length === 0 && parseDiff("   \n").length === 0, "空输入返回空数组");

console.log(`\n${fail === 0 ? "✅" : "❌"} diff 解析：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
