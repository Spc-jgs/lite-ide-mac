import { gitMarks, decoOf, klass } from "../src/lib/shell/tree-git.ts";
import type { GitEntry, GitStatus } from "../src/lib/ipc/commands.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) {
    pass++;
  } else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：得到 ${JSON.stringify(a)}`);

const entry = (path: string, over: Partial<GitEntry> = {}): GitEntry =>
  ({ path, index: ".", work: "M", untracked: false, conflicted: false, ...over }) as GitEntry;
const status = (entries: GitEntry[], untrackedDirs: string[] = []): GitStatus =>
  ({ root: "/p", entries, untrackedDirs }) as unknown as GitStatus;
const row = (path: string, chain = [path]) => ({ path, chain });

// ── 一个条目归哪一档 ──
eq(klass(entry("a", { conflicted: true })), "conflict", "冲突压过一切");
eq(klass(entry("a", { untracked: true })), "untracked", "未跟踪");
eq(klass(entry("a", { index: "A", work: "." })), "added", "只在暂存区新增：看暂存区");
eq(klass(entry("a", { index: "A", work: "M" })), "modified", "工作区还改过：工作区优先（我现在看到的这个文件怎么了）");
eq(klass(entry("a", { index: "R", work: " " })), "renamed", "改名");

// ── 冒泡：目录收着也知道里面有动静，冒到仓库根为止 ──
{
  const g = gitMarks(status([entry("src/a/b.ts")]));
  eq(decoOf(row("/p/src/a/b.ts"), g), { cls: "modified", ch: "M" }, "文件自身的字母");
  eq(decoOf(row("/p/src/a"), g), { cls: "roll", ch: "" }, "父目录只有一个点");
  eq(decoOf(row("/p/src"), g), { cls: "roll", ch: "" }, "一路冒上去");
  ok(!g.roll.has("/p"), "仓库根自己不冒");
  eq(decoOf(row("/p/docs"), g), null, "没改的目录什么都不画");
}

// ── 整个未跟踪的目录：目录自己上色，里面的东西按前缀认 ──
{
  const g = gitMarks(status([], ["new/"]));
  eq(decoOf(row("/p/new"), g), { cls: "untracked", ch: "?" }, "目录自己是未跟踪");
  eq(decoOf(row("/p/new/deep/x.ts"), g), { cls: "untracked", ch: "?" }, "里面的文件按前缀认（条目被截断时兜底）");
}

// ── 合并行：不用特殊处理，链头被报成未跟踪时靠前缀认出来 ──
{
  // git 把整个未跟踪的目录报在最上面那一层（`?? src/main/java/com/`），而合并行的 path 是最深那层
  const J = "/p/src/main/java";
  const g = gitMarks(status([], ["src/main/java/com/"]));
  const merged = row(`${J}/com/demo/order`, [`${J}/com`, `${J}/com/demo`, `${J}/com/demo/order`]);
  eq(decoOf(merged, g), { cls: "untracked", ch: "?" }, "链头被报成未跟踪，整行就是未跟踪（最深那层以链头为前缀）");

  const g2 = gitMarks(status([entry("src/main/java/com/demo/order/A.java")]));
  eq(decoOf(merged, g2), { cls: "roll", ch: "" }, "里面有文件改了：合并行一个点");
}

console.log(`${fail === 0 ? "✅" : "❌"} tree-git：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
