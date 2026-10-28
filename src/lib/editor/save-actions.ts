/**
 * 存盘时照 `.editorconfig` 做的两件事（`trim_trailing_whitespace` / `insert_final_newline`）：算出要改哪几处，由编辑器作为**一笔修改**落进去 ——
 * ⌘Z 撤得回、光标跟着挪，不是写盘时偷偷改（那样盘上和编辑器里不一样，存完立刻又是「未保存」）。
 *
 * - 光标所在的那几行不去行尾空格（IDEA 的默认「保留光标所在行的行尾空格」）：正在打 `foo ` 准备接着写时按了 ⌘S，空格不该被吃掉
 * - `insert_final_newline = false` 是「末尾不要换行」（规范如此），不是「不管」
 * 纯函数、零 import：tests/ 里裸 node 跑
 */

export interface SaveRules {
  trim: boolean | null;
  finalNewline: boolean | null;
}

export interface Change {
  from: number;
  to: number;
  insert: string;
}

/** `keep`：光标所在的行号（从 1 数），这些行不去行尾空格 */
export function saveChanges(text: string, rules: SaveRules, keep: ReadonlySet<number> = new Set()): Change[] {
  const out: Change[] = [];
  if (rules.trim) {
    let start = 0;
    let line = 1;
    while (start <= text.length) {
      let end = text.indexOf("\n", start);
      if (end < 0) end = text.length;
      // CRLF 的行：`\r` 不算行尾空白
      const body = text.charCodeAt(end - 1) === 13 && end > start ? end - 1 : end;
      let i = body;
      while (i > start && (text[i - 1] === " " || text[i - 1] === "\t")) i--;
      if (i < body && !keep.has(line)) out.push({ from: i, to: body, insert: "" });
      if (end === text.length) break;
      start = end + 1;
      line++;
    }
  }
  if (rules.finalNewline !== null && /\S/.test(text)) {
    /*
     * 文件末尾那一段（最后一个非空白字符之后的全部）当一个整体算：去行尾空格之后，原来不在末尾的换行可能成了末尾
     * （`a\n   \n` 去掉空白是 `a\n\n`）。分开算的话「末尾不要换行」只删得到最后那一个，第一版就红在这儿
     */
    let tail = text.length;
    while (tail > 0 && /\s/.test(text[tail - 1])) tail--;
    const old = text.slice(tail);
    let want = rules.trim ? old.replace(/[ \t]+(?=\r?\n|$)/g, "") : old;
    // 光标在最后一行、正打着 `foo ` 时，那一行的空格同样留着
    const tailLine = text.slice(0, tail).split("\n").length;
    if (rules.trim && keep.has(tailLine)) {
      const lead = /^[ \t]*/.exec(old)![0];
      want = lead + old.slice(lead.length).replace(/[ \t]+(?=\r?\n|$)/g, "");
    }
    if (rules.finalNewline) {
      if (!want.endsWith("\n")) want += text.includes("\r\n") ? "\r\n" : "\n";
    } else {
      want = want.replace(/(\r?\n)+$/, "");
    }
    if (want !== old) {
      // 末尾那一段里去行尾空格的那几处已经算进 want 了，不能再各改一次（改动重叠 CM6 直接抛错）
      for (let i = out.length - 1; i >= 0 && out[i].to > tail; i--) {
        if (out[i].from >= tail) out.splice(i, 1);
        else out[i].to = tail;
      }
      out.push({ from: tail, to: text.length, insert: want });
    }
  }
  return out.filter((c) => c.from !== c.to || c.insert);
}

/** 把 `saveChanges` 的结果套到文本上（测试和不在编辑器里的那条路用） */
export function applyChanges(text: string, cs: Change[]): string {
  let out = "";
  let at = 0;
  for (const c of [...cs].sort((a, b) => a.from - b.from)) {
    out += text.slice(at, c.from) + c.insert;
    at = c.to;
  }
  return out + text.slice(at);
}
