/**
 * 文件树的 git 着色：git 状态 → 每一行画什么标记。纯函数，零运行时 import。
 *
 * 从 FileTree.svelte 搬出来（2026-09-24，整理那一轮）：它原来是组件里的一个 `$derived` 加三个函数，
 * 本质上是「输入一份状态、输出几张查找表」，和组件的响应式没有关系 —— 搬出来就能在裸 node 里测。
 */
import type { GitEntry, GitStatus } from "../ipc/commands";

/**
 * git 状态 → 绝对路径查找表。三样东西一起算，因为都要遍历同一份 entries：
 *
 * - `own`  —— 文件/目录**自身**的状态
 * - `roll` —— 祖先目录的「里面有东西改了」冒泡标记。IDE 里最有用的那个提示：
 *   目录收着也知道里面有动静
 * - `utDirs` —— 整个未跟踪的目录。里面的文件 Rust 侧已经摊开进 entries 了，
 *   这份名单是给**目录自己**上色用的：少了它，一个全新的目录只剩
 *   「里面有东西改了」的冒泡标记，和一个改了一行的老目录长得一样。
 *   前缀匹配那半边留着兜底 —— 条目撞上 5000 条上限被截断时，
 *   里面的文件可能一条都没进来
 */
export interface GitMarks {
  own: Map<string, string>;
  roll: Set<string>;
  utDirs: string[];
}

export function gitMarks(st: GitStatus | null): GitMarks {
  const own = new Map<string, string>();
  const roll = new Set<string>();
  const utDirs: string[] = [];
  if (!st) return { own, roll, utDirs };

  // 一路冒泡到仓库根为止
  const bubble = (abs: string) => {
    let p = abs;
    for (;;) {
      const i = p.lastIndexOf("/");
      if (i < 0) break;
      p = p.slice(0, i);
      if (p.length <= st.root.length) break;
      roll.add(p);
    }
  };

  for (const e of st.entries) {
    const abs = `${st.root}/${e.path}`;
    own.set(abs, klass(e));
    bubble(abs);
  }
  // 目录名带着末尾的斜杠，去掉它才是目录自己的路径
  for (const d of st.untrackedDirs ?? []) {
    const abs = `${st.root}/${d.slice(0, -1)}`;
    own.set(abs, "untracked");
    utDirs.push(`${abs}/`);
    bubble(abs);
  }
  return { own, roll, utDirs };
}

export function klass(e: GitEntry): string {
  if (e.conflicted) return "conflict";
  if (e.untracked) return "untracked";
  // 工作区的状态更贴近「我现在看到的这个文件怎么了」，优先它
  const c = e.work !== "." && e.work !== " " ? e.work : e.index;
  switch (c) {
    case "A": return "added";
    case "D": return "deleted";
    case "R":
    case "C": return "renamed";
    default: return "modified";
  }
}

const LETTER: Record<string, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  untracked: "?",
  renamed: "R",
  conflict: "!",
};

/**
 * 一行显示什么装饰：自身状态优先，其次未跟踪目录前缀，最后才是冒泡点。
 *
 * **合并行不用特殊处理**（2026-09-24 验红时查清的）：git 给目录报的只有一种状态 ——「整个目录未跟踪」
 * （`?? com/`），而它正好落在 `utDirs` 的前缀兜底里：合并行的 `path` 是最深那层 `com/demo/order`，
 * 以 `com/` 开头，照样认成未跟踪。合并包名那一轮给这里加过「链上每一层都查」，理由写的是
 * 「只查最深那层就漏了」—— 那是错的：把那段改回只查 `path` 测试照样绿，说明它从没起过作用。删了。
 */
export function decoOf(row: { path: string }, git: GitMarks): { cls: string; ch: string } | null {
  const own = git.own.get(row.path);
  if (own) return { cls: own, ch: LETTER[own] ?? "·" };
  for (const d of git.utDirs) {
    if (row.path.startsWith(d)) return { cls: "untracked", ch: "?" };
  }
  if (git.roll.has(row.path)) return { cls: "roll", ch: "" };
  return null;
}
