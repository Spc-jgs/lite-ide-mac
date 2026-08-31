import {
  parseDiff,
  toSideBySide,
  changeBlocks,
  blankRuns,
  FLAT_BLANK_RUN,
  type DiffLine,
} from "../src/lib/git/diff.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// 1:1 改动 —— 左右对齐，不留空格
const a = toSideBySide(parseDiff(`diff --git a/x b/x
@@ -1,3 +1,3 @@
 keep
-old1
-old2
+new1
+new2
 tail
`)[0].lines);
ok(a.length === 5, `1 hunk + 1 ctx + 2 change + 1 ctx = 5 行，实得 ${a.length}`);
ok(a[0].kind === "hunk", "第一行是 hunk 头");
ok(a[1].left === a[1].right, "上下文行左右是同一条");
ok(a[2].kind === "change" && a[2].left?.text === "old1" && a[2].right?.text === "new1", "第一处改动配对");
ok(a[3].left?.text === "old2" && a[3].right?.text === "new2", "第二处改动配对");

// 删多增少 —— 多出来的删除行右边留空
const b = toSideBySide(parseDiff(`diff --git a/y b/y
@@ -1,3 +1,1 @@
-a
-b
-c
+z
`)[0].lines);
ok(b.length === 4, `1 hunk + 3 change = 4，实得 ${b.length}`);
ok(b[1].right?.text === "z" && b[1].left?.text === "a", "首行配上");
ok(b[2].right === null && b[2].left?.text === "b", "多出的删除行右侧留空");
ok(b[3].right === null && b[3].left?.text === "c", "多出的删除行右侧留空");

// 纯新增 —— 左边整段留空
const c = toSideBySide(parseDiff(`diff --git a/z b/z
new file mode 100644
@@ -0,0 +1,2 @@
+p
+q
`)[0].lines);
ok(c.filter(r => r.kind === "change").every(r => r.left === null), "纯新增左侧全空");
ok(c.filter(r => r.kind === "change").length === 2, "两行新增");

// 行内高亮要能穿过双栏转换活下来
ok(a[2].left?.span !== undefined && a[2].right?.span !== undefined, "行内区间在双栏里仍在");

// 变更块：连续的算一块
const blocks = changeBlocks(a);
ok(blocks.length === 1, `连续两行改动应算一块，实得 ${blocks.length}`);
const d = toSideBySide(parseDiff(`diff --git a/w b/w
@@ -1,6 +1,6 @@
 c1
-x
+y
 c2
 c3
-m
+n
 c4
`)[0].lines);
ok(changeBlocks(d).length === 2, `隔开的两处改动应算两块，实得 ${changeBlocks(d).length}`);

// 空 diff
ok(toSideBySide([]).length === 0, "空输入");

// ── 长空白块标记（斜纹换纯色的依据）──
{
  const rows = (spec: string) =>
    [...spec].map((c) => ({
      kind: c === "c" ? ("ctx" as const) : ("change" as const),
      left: c === "c" || c === "l" ? ({ kind: "ctx", text: "" } as DiffLine) : null,
      right: c === "c" || c === "r" ? ({ kind: "ctx", text: "" } as DiffLine) : null,
    }));
  // r = 只有右边（左边空），c = 两边都有
  const j = (b: boolean[]) => b.map((x) => (x ? "1" : "0")).join("");

  ok(j(blankRuns(rows("crrc")).left) === "0000", `2 行空白不算长块，实得 ${j(blankRuns(rows("crrc")).left)}`);
  ok(j(blankRuns(rows("crrrc")).left) === "00000", `恰好 3 行还不算（FLAT_BLANK_RUN=${FLAT_BLANK_RUN}）`);
  ok(j(blankRuns(rows("crrrrc")).left) === "011110", `4 行算长块，实得 ${j(blankRuns(rows("crrrrc")).left)}`);
  // 结尾那一段也要被结算 —— 循环少跑一格就会漏掉它
  ok(j(blankRuns(rows("crrrr")).left) === "01111", `块顶到结尾也要标出来，实得 ${j(blankRuns(rows("crrrr")).left)}`);
  ok(j(blankRuns(rows("rrrrc")).left) === "11110", "块从开头起也要标出来");
  // 左右两侧各算各的
  ok(j(blankRuns(rows("cllllc")).right) === "011110", "右侧的空白块同样要标");
  ok(j(blankRuns(rows("cllllc")).left) === "000000", "左侧有内容就不该被标");
  // hunk 行把上下两段隔开，不该被连成一块
  {
    const split = [
      ...rows("rr"),
      { kind: "hunk" as const, left: null, right: null, text: "@@" },
      ...rows("rr"),
    ];
    ok(j(blankRuns(split).left) === "00000", `hunk 行要把两段 2 行的空白隔开，实得 ${j(blankRuns(split).left)}`);
  }
  ok(blankRuns([]).left.length === 0, "空输入");
}

console.log(`\n${fail === 0 ? "✅" : "❌"} 双栏对照：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
