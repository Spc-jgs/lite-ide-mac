/**
 * 把一份单文件 unified diff 拆成「文件头 + 每个 hunk 的原文」，给按块暂存用
 * （issue #33 ⑫）。`git apply --cached` 吃的是一份合法的 patch：文件头 + 任意
 * 一个或几个 hunk 就是合法的 —— hunk 之间互相独立，行号是各自的。
 *
 * 只认 `git diff` 自己的输出（`parseDiff` 的输入），不做通用 patch 解析。
 * 多文件的 diff 只取第一个文件：差异标签本来就是单文件的。
 *
 * `index abc..def` 那行**扔掉**：`apply --cached` 不带 `--3way` 时用不着它，
 * 带着反而会在暂存区已经被别的块改过之后报「does not match index」。
 */
export interface HunkPatches {
  /** `diff --git` 到第一个 `@@` 之前的那几行（去掉 index 行），末尾带换行 */
  header: string;
  /** 每个 hunk 从 `@@` 到下一个 `@@`（或文件尾）的原文，末尾带换行 */
  hunks: string[];
}

export function splitHunks(raw: string): HunkPatches {
  const lines = raw.split("\n");
  const header: string[] = [];
  const hunks: string[] = [];
  let cur: string[] | null = null;
  let seenFile = false;
  for (const line of lines) {
    if (line.startsWith("diff --git ")) {
      if (seenFile) break; // 第二个文件：不要
      seenFile = true;
      header.push(line);
      continue;
    }
    if (!seenFile) continue;
    if (line.startsWith("@@")) {
      if (cur) hunks.push(cur.join("\n") + "\n");
      cur = [line];
      continue;
    }
    if (cur) {
      // 末尾的空串是 split 出来的，不是内容
      if (line === "" && lines[lines.length - 1] === "") continue;
      cur.push(line);
    } else if (!line.startsWith("index ")) {
      header.push(line);
    }
  }
  if (cur) hunks.push(cur.join("\n") + "\n");
  return { header: header.length ? header.join("\n") + "\n" : "", hunks };
}

/** 第 `i` 块单独成一份 patch；越界给空串 */
export function hunkPatch(p: HunkPatches, i: number): string {
  const h = p.hunks[i];
  return h === undefined || !p.header ? "" : p.header + h;
}
