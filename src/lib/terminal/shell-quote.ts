/**
 * 把一个路径写成 shell 里能原样粘贴的样子 —— 从 Finder 拖文件进终端时插入的就是它。
 *
 * 照 Terminal.app 的做法：对 shell 有特殊含义的字符前面加反斜杠（`My\ File\ (1).txt`），
 * 而不是整段套单引号 —— 拖进来之后人常常还要在路径后面接着敲、或者改一截，
 * 反斜杠的写法改起来和平常敲路径一样。
 */
const SPECIAL = /[\s'"\\$`!&*()?;<>|{}[\]#~^=%,]/g;

export function shellQuote(path: string): string {
  return path.replace(SPECIAL, (c) => `\\${c}`);
}

/** 拖进来的一批：空格隔开，末尾留一个空格好接着敲（Terminal.app 同样） */
export function dropText(paths: string[]): string {
  return paths.map(shellQuote).join(" ") + " ";
}
