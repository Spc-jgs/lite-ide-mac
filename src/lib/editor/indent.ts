/**
 * 从文件内容猜缩进与换行符（issue #33 ③）。状态栏显示，编辑器照它缩进。
 *
 * IDEA 和 VS Code（`detectIndentation`）都这么做：不问用户、不读配置，看文件
 * 自己是怎么写的。判据全在这个文件里，纯函数，`tests/indent.test.ts` 卡着。
 *
 * # 只看开头一段
 *
 * 文件可以到 64MB（`read_text` 的上限），逐行扫一遍不值：缩进风格在头几百行里
 * 就定了，换行符看第一个就够（混用的文件本身就是坏的，说「混用」比猜一个强）。
 */

export type Eol = "LF" | "CRLF" | "CR" | "混用";

/** `tab` = 制表符；数字 = 几个空格。`null` = 文件里没有缩进过的行，看不出来 */
export type Indent = "tab" | number | null;

/** 只扫这么多行 */
const MAX_LINES = 400;
/** 只扫这么多字节 —— 一行几百 KB 的压缩产物按行数算也会扫很久 */
const MAX_CHARS = 200_000;

export function detectEol(text: string): Eol {
  const head = text.slice(0, MAX_CHARS);
  let lf = 0, crlf = 0, cr = 0;
  for (let i = 0; i < head.length; i++) {
    const c = head.charCodeAt(i);
    if (c === 13) {
      if (head.charCodeAt(i + 1) === 10) {
        crlf++;
        i++;
      } else cr++;
    } else if (c === 10) lf++;
  }
  const kinds = (lf ? 1 : 0) + (crlf ? 1 : 0) + (cr ? 1 : 0);
  if (kinds > 1) return "混用";
  if (crlf) return "CRLF";
  if (cr) return "CR";
  return "LF"; // 没有换行的单行文件也报 LF：写回去时就是这个
}

/**
 * 缩进：数**相邻两行缩进差**，不数每行的绝对缩进。
 *
 * 绝对缩进会被深层嵌套骗：一份 2 空格的文件里，8 空格的行可能比 2 空格的还多
 * （函数体里的东西总比顶层多）。而相邻行的差只会是一个缩进单位（或它的小倍数），
 * 众数就是答案 —— VS Code 的 `guessIndentation` 也是这条思路。
 *
 * 制表符和空格分开数：哪个多算哪个。制表符文件里偶尔混几行空格（对齐用）是常态。
 */
export function detectIndent(text: string): Indent {
  const head = text.slice(0, MAX_CHARS);
  const lines = head.split(/\r\n|\r|\n/, MAX_LINES);
  let tabLines = 0, spaceLines = 0;
  const deltas = new Map<number, number>();
  let prev = 0;
  for (const line of lines) {
    if (line.trim() === "") continue; // 空行不算：它的缩进是随手的
    let i = 0;
    while (i < line.length && (line[i] === " " || line[i] === "\t")) i++;
    if (i === 0) {
      prev = 0;
      continue;
    }
    if (line[0] === "\t") {
      tabLines++;
      prev = 0;
      continue;
    }
    spaceLines++;
    const d = Math.abs(i - prev);
    // 差 1 多半是对齐（`.then(` 之类），不是缩进单位；差太大也不是
    if (d >= 2 && d <= 8) deltas.set(d, (deltas.get(d) ?? 0) + 1);
    prev = i;
  }
  if (tabLines === 0 && spaceLines === 0) return null;
  if (tabLines > spaceLines) return "tab";
  let best = 0, bestN = 0;
  for (const [d, n] of deltas) {
    // 并列时取小的：4 空格的文件里差 8 的行（连跳两级）不该赢过差 4 的
    if (n > bestN || (n === bestN && d < best)) {
      best = d;
      bestN = n;
    }
  }
  // 全是同一级缩进（没有相邻差）：按第一行缩进过的行的宽度算，封顶 8
  if (best === 0) {
    for (const line of lines) {
      let i = 0;
      while (i < line.length && line[i] === " ") i++;
      if (i > 0 && line.trim() !== "") return Math.min(i, 8);
    }
    return null;
  }
  return best;
}

/** 状态栏那格的文字：`4 空格` / `Tab` / `—` */
export function indentLabel(ind: Indent): string {
  if (ind === null) return "—";
  return ind === "tab" ? "Tab" : `${ind} 空格`;
}
