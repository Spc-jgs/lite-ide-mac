/**
 * 行内变量值（IDEA 停下时在代码行尾灰字写 `cents = 12345`；docs/TAKEOVER.md G14）：哪几行写什么。
 *
 * 范围是「执行点所在的那个方法，从它开头到执行点这一行」（调用方用语法树找方法开头）：往下的行还没跑到，写上值是在骗人；
 * 别的方法里的同名变量不是这一个。一行里提到了哪几个局部变量就写哪几个（按在这一行里出现的次序），只认整词、跳过点号后面的
 * （`order.cents` 里的 `cents` 是字段，不是局部变量 `cents`）、跳过字符串和 `//` 注释里的。值太长截断（行尾不是看大对象的地方）
 */
export interface Val {
  name: string;
  value: string;
}

/** 一个值最多显示多少字 */
export const VAL_MAX = 60;

const short = (v: string) => (v.length > VAL_MAX ? `${v.slice(0, VAL_MAX - 1)}…` : v);

/** 去掉字符串字面量和行尾注释（换成空格，下标不动） */
function code(line: string): string {
  let out = "";
  let q: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (q) {
      if (c === "\\") {
        out += "  ";
        i++;
        continue;
      }
      if (c === q) q = null;
      out += " ";
      continue;
    }
    if (c === '"' || c === "'") {
      q = c;
      out += " ";
      continue;
    }
    if (c === "/" && line[i + 1] === "/") return out + " ".repeat(line.length - i);
    out += c;
  }
  return out;
}

/**
 * `lines`：整份文本按行（下标 0 是第 1 行）；`from` / `to`：从第几行到第几行（从 1 数，含两头）。
 * 回：行号（从 1 数）→ 那一行尾巴上写的字（`cents = 12345, s = "CNY"`）。`this` 不写（它在变量面板里，写在每一行上是噪音）
 */
export function inlineVals(lines: string[], from: number, to: number, vals: Val[]): Map<number, string> {
  const by = new Map(vals.filter((v) => v.name !== "this" && /^[\p{L}_$][\p{L}\p{N}_$]*$/u.test(v.name)).map((v) => [v.name, v.value]));
  const out = new Map<number, string>();
  if (!by.size) return out;
  for (let n = Math.max(1, from); n <= Math.min(to, lines.length); n++) {
    const c = code(lines[n - 1] ?? "");
    const seen: string[] = [];
    for (const m of c.matchAll(/[\p{L}_$][\p{L}\p{N}_$]*/gu)) {
      const w = m[0];
      if (!by.has(w) || seen.includes(w)) continue;
      // 点号后面的是字段 / 方法（`order.cents`），不是这个局部变量
      if (c.slice(0, m.index).trimEnd().endsWith(".")) continue;
      seen.push(w);
    }
    if (seen.length) out.set(n, seen.map((w) => `${w} = ${short(by.get(w)!)}`).join(", "));
  }
  return out;
}
