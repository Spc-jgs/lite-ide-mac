/**
 * Java 的代码模板（docs/TAKEOVER.md G21，IDEA 的 Live Templates / Postfix Completion）：敲 `sout` 按 Tab 展开成 `System.out.println();`。
 *
 * **不重造模板引擎**：jdtls 自己有两套 —— 代码片段（`sysout`、`foreach`、`fori`、`ifnull`…，按上下文猜变量）和后缀补全
 * （`.var`、`.for`、`.fori`、`.null`、`.nnull`、`.not`、`.par`、`.throw`…，默认开，读 1.61 的字节码看到的），补全框里本来就有。
 * 缺的是 **IDEA 的名字和手感**：IDEA 里敲的是 `sout` / `psvm` / `iter` / `ifn`，按的是 Tab；后缀是 `.sout`（jdtls 叫 `.sysout`，`sout` 不是它的前缀，匹配不上）。
 * 所以这里只有一张 IDEA 缩写表，展开成 CM6 的 snippet（`${名字}` 是要填的格子、同名的一起改，`${}` 是最后光标停的地方）；
 * `.nn` 不用补：它是 jdtls `.nnull` 的前缀，补全框里本来就出得来。
 *
 * 纯的部分（哪个词是模板、后缀前面那个表达式从哪儿开始）在这里、单独测；挂到编辑器上的在 java-lsp.ts
 */
export interface Tpl {
  key: string;
  /** 补全框右边那一格（IDEA 的模板说明） */
  desc: string;
  /** CM6 snippet：`${名字}` 是格子（同名的一起改），`${}` 是最后停的地方；`\t` 换成编辑器的缩进 */
  body: string;
}

/** IDEA 默认那几个最常用的（Java 后端天天敲的），名字照 IDEA */
export const TEMPLATES: Tpl[] = [
  { key: "sout", desc: "System.out.println", body: "System.out.println(${});" },
  { key: "serr", desc: "System.err.println", body: "System.err.println(${});" },
  { key: "souf", desc: "System.out.printf", body: 'System.out.printf("${}%n");' },
  { key: "soutv", desc: "打印一个变量：名字 = 值", body: 'System.out.println("${name} = " + ${name});${}' },
  { key: "psvm", desc: "main 方法", body: "public static void main(String[] args) {\n\t${}\n}" },
  { key: "main", desc: "main 方法", body: "public static void main(String[] args) {\n\t${}\n}" },
  { key: "fori", desc: "for (int i = 0; i < n; i++)", body: "for (int ${i} = 0; ${i} < ${n}; ${i}++) {\n\t${}\n}" },
  { key: "itar", desc: "遍历数组（下标）", body: "for (int ${i} = 0; ${i} < ${array}.length; ${i}++) {\n\t${}\n}" },
  { key: "iter", desc: "for (T item : items)", body: "for (${var} ${item} : ${items}) {\n\t${}\n}" },
  { key: "ifn", desc: "if (x == null)", body: "if (${x} == null) {\n\t${}\n}" },
  { key: "inn", desc: "if (x != null)", body: "if (${x} != null) {\n\t${}\n}" },
  { key: "thr", desc: "throw new", body: "throw new ${}" },
  { key: "psf", desc: "public static final", body: "public static final ${}" },
  { key: "psfs", desc: "public static final String", body: "public static final String ${}" },
  { key: "psfi", desc: "public static final int", body: "public static final int ${}" },
  { key: "prsf", desc: "private static final", body: "private static final ${}" },
];

const byKey = new Map(TEMPLATES.map((t) => [t.key, t]));

/** 后缀补全里 jdtls 没有、IDEA 有的那两个：`expr.sout` → `System.out.println(expr);` */
export const POSTFIX: { key: string; desc: string; wrap: (expr: string) => string }[] = [
  { key: "sout", desc: "System.out.println(expr)", wrap: (e) => `System.out.println(${e});\${}` },
  { key: "soutv", desc: '打印：expr = 值', wrap: (e) => `System.out.println("${e.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/[{}$]/g, "")} = " + ${e});\${}` },
];

/**
 * 光标前面那个词是不是一个模板、从哪儿开始：只认整个词（前面不是字母数字、不是 `.`）—— `.sout` 那种是后缀补全，`xsout` 不是模板。
 * `text` 是这一行光标前面的部分。不是就 null
 */
export function templateAt(text: string): { tpl: Tpl; from: number } | null {
  const m = /(^|[^\p{L}\p{N}_$.])([\p{L}_$][\p{L}\p{N}_$]*)$/u.exec(text);
  if (!m) return null;
  const tpl = byKey.get(m[2]);
  return tpl ? { tpl, from: text.length - m[2].length } : null;
}

/**
 * 后缀补全前面那个表达式从第几个字开始：`text` 是这一行到那个 `.` 为止（不含点）。往回认标识符、点、配对的括号（`order.items().get(0)`），
 * 碰到空白、运算符、没配对的括号就停。一个字都认不出（`.` 前面是空白）回 -1
 */
export function exprStart(text: string): number {
  let i = text.length;
  let depth = 0;
  while (i > 0) {
    const c = text[i - 1];
    if (c === ")" || c === "]") depth++;
    else if (c === "(" || c === "[") {
      if (depth === 0) break;
      depth--;
    } else if (depth === 0 && !/[\p{L}\p{N}_$.]/u.test(c)) break;
    else if (depth > 0 && c === '"') {
      // 括号里的字符串原样跳过（`get("a)b")`）
      const j = text.lastIndexOf('"', i - 2);
      if (j < 0) return -1;
      i = j;
      continue;
    }
    i--;
  }
  if (depth !== 0 || i === text.length) return -1;
  // 认出来的不能是以点开头的半截（`  .foo`）
  return /^[\p{L}_$"(]/u.test(text.slice(i)) ? i : -1;
}
