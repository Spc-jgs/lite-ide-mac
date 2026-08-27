/**
 * 冲突标记解析。
 *
 * git 打起来的冲突文件长这样：
 *
 * ```
 * <<<<<<< HEAD
 * 我这边的内容
 * ||||||| merged common ancestors     ← 只有 diff3 风格才有
 * 共同祖先的内容
 * =======
 * 他们那边的内容
 * >>>>>>> feature/x
 * ```
 *
 * 为什么解析工作区文件、而不是去读 `git show :2:path` / `:3:path` 那三个暂存位：
 * 工作区文件是**用户此刻真正会提交的东西**。他可能已经手动改过一部分了 ——
 * 从暂存位重新构造会把那些手改悄悄抹掉。以盘上的实际内容为准，才不会骗人。
 */

export interface ConflictBlock {
  kind: "conflict";
  /** 我这边（HEAD / 当前分支） */
  ours: string[];
  /** 他们那边（被合并进来的分支） */
  theirs: string[];
  /** 共同祖先；只有 diff3 风格才有 */
  base: string[] | null;
  /** 标记行上的标签，如 `HEAD` / `feature/x` */
  oursLabel: string;
  theirsLabel: string;
  /** 用户选了哪边；null 表示还没决定 */
  pick: "ours" | "theirs" | "both" | "base" | null;
}

export interface PlainBlock {
  kind: "plain";
  lines: string[];
}

export type Block = PlainBlock | ConflictBlock;

const OURS = /^<{7}(?: (.*))?$/;
const BASE = /^\|{7}(?: (.*))?$/;
const SEP = /^={7}$/;
const THEIRS = /^>{7}(?: (.*))?$/;

/** 把带冲突标记的文本切成「普通段 / 冲突段」的序列 */
export function parseConflicts(text: string): Block[] {
  const lines = text.split("\n");
  const out: Block[] = [];
  let plain: string[] = [];
  let i = 0;

  const flush = () => {
    if (plain.length) {
      out.push({ kind: "plain", lines: plain });
      plain = [];
    }
  };

  while (i < lines.length) {
    const m = OURS.exec(lines[i]);
    if (!m) {
      plain.push(lines[i++]);
      continue;
    }
    // 进入一个冲突块
    const oursLabel = (m[1] ?? "").trim();
    let theirsLabel = "";
    const ours: string[] = [];
    let base: string[] | null = null;
    const theirs: string[] = [];
    let phase: "ours" | "base" | "theirs" = "ours";
    i++;
    let closed = false;

    while (i < lines.length) {
      const l = lines[i];
      if (BASE.test(l)) {
        base = [];
        phase = "base";
        i++;
        continue;
      }
      if (SEP.test(l)) {
        phase = "theirs";
        i++;
        continue;
      }
      const t = THEIRS.exec(l);
      if (t) {
        theirsLabel = (t[1] ?? "").trim();
        i++;
        closed = true;
        break;
      }
      if (phase === "ours") ours.push(l);
      else if (phase === "base") base!.push(l);
      else theirs.push(l);
      i++;
    }

    if (!closed) {
      /*
       * 开了 `<<<<<<<` 却没有配对的 `>>>>>>>`。这多半不是 git 写的冲突，
       * 而是文件里恰好有这么一行（比如一份讲冲突解决的文档）。
       * 那就把它当普通文本还回去，别把好文件解析成半个冲突块。
       */
      plain.push(m[0], ...ours);
      if (base) plain.push(...base);
      plain.push(...theirs);
      continue;
    }

    flush();
    out.push({ kind: "conflict", ours, theirs, base, oursLabel, theirsLabel, pick: null });
  }
  flush();
  return out;
}

/** 还有没有没决定的冲突 */
export function unresolved(blocks: Block[]): number {
  return blocks.filter((b) => b.kind === "conflict" && b.pick === null).length;
}

/**
 * 按当前选择拼回完整文件。
 *
 * 没做决定的冲突块**原样保留标记** —— 半成品文件写回去时不该悄悄丢内容，
 * 用户可以先存一部分、回头再处理剩下的。
 */
export function compose(blocks: Block[]): string {
  const out: string[] = [];
  for (const b of blocks) {
    if (b.kind === "plain") {
      out.push(...b.lines);
      continue;
    }
    switch (b.pick) {
      case "ours":
        out.push(...b.ours);
        break;
      case "theirs":
        out.push(...b.theirs);
        break;
      case "base":
        if (b.base) out.push(...b.base);
        break;
      case "both":
        out.push(...b.ours, ...b.theirs);
        break;
      default:
        out.push(`<<<<<<< ${b.oursLabel}`.trimEnd(), ...b.ours);
        if (b.base) out.push(`||||||| ${"merged common ancestors"}`, ...b.base);
        out.push("=======", ...b.theirs, `>>>>>>> ${b.theirsLabel}`.trimEnd());
    }
  }
  return out.join("\n");
}
