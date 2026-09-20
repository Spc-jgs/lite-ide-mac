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

/**
 * 从一块里只挑几行，拼成一份 git 收的 patch 块（issue #38 按行暂存）。
 *
 * `keep` 是块**正文**（`@@` 那行之后）里要的行的下标（0-based，上下文行也占下标，
 * 但选不选它都一样）。规则由「patch 的基线是谁」决定：
 *
 * - 正向（暂存，基线是暂存区 = 旧侧）：没选的 `+` 行**扔掉**（它在基线里本来就不存在），
 *   没选的 `-` 行**变成上下文**（它在基线里还在，只是这次不删）。
 * - 反向（`apply -R` 取消暂存，基线是暂存区 = **新**侧）：角色对调 —— 没选的 `+` 行变上下文
 *   （暂存区里有它），没选的 `-` 行扔掉（暂存区里没有它）。
 *
 * `\ No newline at end of file` 跟着它前一行走：前一行扔了它也扔。前一行要**转成上下文**时
 * 不能简单转：上下文行隐含「带换行」，而基线里那一行正是没换行的，git 对不上（review
 * 2026-09-20 在真仓库撞的：`patch does not apply`）。正确写法是「删掉没换行的它、再加回
 * 带换行的它」—— 原行 + 标记 + 反号的同一行；反向时符号对调，标记留在基线那一侧。
 * `@@` 头原样留着，行数交给 `git apply --recount` 重数 —— 自己算头等于把 git 已经做对的
 * 事再做一遍。一行都没选到给空串，调用方别拿它去 apply。
 */
export function pickLines(hunk: string, keep: ReadonlySet<number>, reverse = false): string {
  const lines = hunk.split("\n");
  if (!lines[0]?.startsWith("@@")) return "";
  // 末尾的空串是尾随换行 split 出来的，不是正文
  if (lines[lines.length - 1] === "") lines.pop();
  const out: string[] = [lines[0]];
  let picked = 0;
  /** 前一行原样留下了 —— 只有这种情况它后面的 `\ No newline` 才跟着留 */
  let prevKept = false;
  /** 「转上下文」撞上无换行标记时，标记之后要补的那一行（带换行的同一行，反号） */
  let afterMark: string | null = null;
  for (let i = 1; i < lines.length; i++) {
    const l = lines[i];
    const idx = i - 1;
    const c = l[0];
    if (c === "\\") {
      if (prevKept) out.push(l);
      if (afterMark !== null) out.push(afterMark);
      afterMark = null;
      continue;
    }
    const isChange = c === "+" || c === "-";
    if (!isChange || keep.has(idx)) {
      out.push(l);
      if (isChange) picked++;
      prevKept = true;
      continue;
    }
    // 没选的改动行：在基线里存在的变上下文，不存在的扔掉
    const inBase = reverse ? c === "+" : c === "-";
    if (inBase) {
      if (lines[i + 1]?.[0] === "\\") {
        // 基线里这行没换行：原样留下（连同标记），标记之后再加回带换行的它
        out.push(l);
        afterMark = (c === "-" ? "+" : "-") + l.slice(1);
        prevKept = true;
        continue;
      }
      out.push(` ${l.slice(1)}`);
    }
    prevKept = false;
  }
  return picked === 0 ? "" : out.join("\n") + "\n";
}
