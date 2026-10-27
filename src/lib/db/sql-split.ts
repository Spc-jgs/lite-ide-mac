/**
 * SQL 控制台里「光标所在的那条」是哪一段（docs/DATABASE.md 第 5 节）：按 `;` 和空行切，照 IDEA 的控制台。
 *
 * 字符串、带引号的名字、注释、PostgreSQL 的 `$$ … $$`（函数体）里面的 `;` 和空行不算 —— 切错的后果是把半条语句发给数据库，
 * 报一个和你写的东西对不上的语法错，或者更糟：`DELETE FROM t WHERE …` 在 WHERE 前面被切开。
 * 不碰编辑器、不碰 IPC：在裸 node 里测（tests/sql-split.test.ts）。
 */

export interface Stmt {
  /** 在全文里的起止（去掉了两头的空白），高亮和「跑的是哪一段」都按它 */
  from: number;
  to: number;
  sql: string;
}

/** 全文切成语句。只有注释和空白的段不算一条 */
export function splitSql(text: string): Stmt[] {
  const out: Stmt[] = [];
  let start = 0;
  let meaningful = false;
  const push = (end: number) => {
    if (meaningful) {
      let a = start;
      let b = end;
      while (a < b && /\s/.test(text[a])) a++;
      while (b > a && /\s/.test(text[b - 1])) b--;
      out.push({ from: a, to: b, sql: text.slice(a, b) });
    }
    meaningful = false;
  };
  let i = 0;
  const n = text.length;
  while (i < n) {
    const c = text[i];
    const next = text[i + 1];
    if (c === "-" && next === "-") {
      const nl = text.indexOf("\n", i);
      i = nl < 0 ? n : nl;
      continue;
    }
    if (c === "/" && next === "*") {
      const end = text.indexOf("*/", i + 2);
      i = end < 0 ? n : end + 2;
      continue;
    }
    if (c === "'" || c === '"' || c === "`") {
      meaningful = true;
      i = skipQuoted(text, i, c);
      continue;
    }
    if (c === "$") {
      // PostgreSQL 的美元引号：$$ 或 $标签$，到同样的那一串为止
      const m = /^\$([A-Za-z_][A-Za-z0-9_]*)?\$/.exec(text.slice(i, i + 64));
      if (m) {
        meaningful = true;
        const end = text.indexOf(m[0], i + m[0].length);
        i = end < 0 ? n : end + m[0].length;
        continue;
      }
    }
    if (c === ";") {
      push(i);
      start = i + 1;
      i++;
      continue;
    }
    if (c === "\n") {
      // 空行（只有空白的一行）也是分隔：照 IDEA，没写分号的几条之间空一行就能分开跑
      let j = i + 1;
      while (j < n && (text[j] === " " || text[j] === "\t" || text[j] === "\r")) j++;
      if (j < n && text[j] === "\n") {
        push(i);
        start = j;
        i = j;
        continue;
      }
      i++;
      continue;
    }
    if (!/\s/.test(c)) meaningful = true;
    i++;
  }
  push(n);
  return out;
}

/** 引号里：同一种引号连写两个是转义（SQL 标准）；单引号里反斜杠转义下一个字（MySQL 默认如此）。返回引号之后的位置 */
function skipQuoted(text: string, i: number, q: string): number {
  let j = i + 1;
  while (j < text.length) {
    const c = text[j];
    if (q === "'" && c === "\\") {
      j += 2;
      continue;
    }
    if (c === q) {
      if (text[j + 1] === q) {
        j += 2;
        continue;
      }
      return j + 1;
    }
    j++;
  }
  return text.length;
}

/**
 * 光标所在的那条。光标在两条之间的空白里：同一行上前面有一条（`select 1;|`）就是它，否则是下一条（同 IDEA）；
 * 都没有是 null
 */
export function statementAt(text: string, pos: number): Stmt | null {
  const all = splitSql(text);
  const hit = all.find((s) => pos >= s.from && pos <= s.to);
  if (hit) return hit;
  const lineStart = text.lastIndexOf("\n", pos - 1) + 1;
  const before = [...all].reverse().find((s) => s.to <= pos);
  if (before && before.to >= lineStart) return before;
  const lineEnd = text.indexOf("\n", pos);
  const after = all.find((s) => s.from >= pos);
  if (after && (lineEnd < 0 || after.from <= lineEnd)) return after;
  return before ?? after ?? null;
}

/** 选中了一段：选中的里面切出来的每一条（位置换算回全文） */
export function statementsIn(text: string, from: number, to: number): Stmt[] {
  return splitSql(text.slice(from, to)).map((s) => ({ from: s.from + from, to: s.to + from, sql: s.sql }));
}
