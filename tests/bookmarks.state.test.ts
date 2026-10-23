/**
 * 书签（docs/TAKEOVER.md G18）：表的增删改、跨重启读回来、文件 / 目录改名时跟过去；编辑器里上面插了几行，书签跟着往下走。
 */
const { EditorState } = await import("@codemirror/state");
const { gutterLineClass } = await import("@codemirror/view");
const bm = await import("../src/lib/state/bookmarks");
const { bookmarks } = await import("../src/lib/editor/bookmarks-ext");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const lines = (root: string | null, path: string) => bm.marksOf(root, path).map((m) => m.line).join(",");

const R = "/p";
ok(bm.toggle(R, "/p/A.java", 5, "    int total = 0;   ") === true, "没有就打上");
bm.toggle(R, "/p/A.java", 2, "class A {");
bm.toggle(R, "/p/B.java", 9, "x");
ok(lines(R, "/p/A.java") === "2,5", `按行排：${lines(R, "/p/A.java")}`);
ok(bm.marksOf(R, "/p/A.java")[1].text === "int total = 0;", "存的是那一行去掉两头空白的字");
ok(bm.toggle(R, "/p/A.java", 2, "") === false && lines(R, "/p/A.java") === "5", "再按一下取消");
ok(bm.allOf(R).map((e) => `${e.path.slice(3)}:${e.line}`).join(",") === "A.java:5,B.java:9", "全部按文件、行号排");
ok(bm.allOf(null).length === 0, "没项目的那一格是另一份");
bm.setMarks(R, "/p/A.java", [{ line: 7, text: "a" }, { line: 7, text: "b" }, { line: 3, text: "c" }]);
ok(lines(R, "/p/A.java") === "3,7", "同一行两个只留一个");

// 跨重启：丢掉缓存、从 localStorage 读回来
bm._reset();
ok(lines(R, "/p/A.java") === "3,7" && lines(R, "/p/B.java") === "9", "重读 localStorage 还在");

// 文件改名、目录挪走：书签跟过去，旧路径上没了
bm.rename(R, "/p/A.java", "/p/Renamed.java");
ok(lines(R, "/p/Renamed.java") === "3,7" && lines(R, "/p/A.java") === "", "文件改名跟过去");
bm.setMarks(R, "/p/dir/C.java", [{ line: 1, text: "" }]);
bm.setMarks(R, "/p/dirx/D.java", [{ line: 4, text: "" }]);
bm.rename(R, "/p/dir", "/p/moved");
ok(lines(R, "/p/moved/C.java") === "1" && lines(R, "/p/dir/C.java") === "", "目录挪走，底下的跟过去");
ok(lines(R, "/p/dirx/D.java") === "4", "名字只是前缀相同的 /p/dirx 不动");

// 编辑器里：上面插两行，第 3 行的书签跟到第 5 行
{
  bm.setMarks(R, "/p/E.txt", [{ line: 3, text: "c" }]);
  let st = EditorState.create({ doc: "a\nb\nc\nd\n", extensions: [bookmarks(R, "/p/E.txt")] });
  const marked = () => {
    const out: number[] = [];
    for (const set of st.facet(gutterLineClass)) for (const it = set.iter(); it.value; it.next()) out.push(st.doc.lineAt(it.from).number);
    return out.join(",");
  };
  ok(marked() === "3", `表里的书签画在第 3 行：${marked()}`);
  st = st.update({ changes: { from: 0, insert: "x\ny\n" } }).state;
  ok(marked() === "5", `上面插两行，跟到第 5 行：${marked()}`);
}

console.log(`${fail === 0 ? "✅" : "❌"} 书签：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
