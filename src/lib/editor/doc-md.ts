/**
 * 快速文档（F1，docs/TAKEOVER.md G3）的 Markdown → 几块可以安全画的东西。
 *
 * 不用 Markdown 库、不插 HTML：页面里的文字一律走 Svelte 的文本插值（`tests/no-html-sinks.test.ts` 卡着）——
 * Javadoc 是 jar 里别人写的，一旦能插 HTML 就等于能跑别人的脚本。jdtls 转出来的 Markdown 只有几种结构：
 * 代码块（签名）、段落、`*` 列表（Parameters / Returns）、`**粗**` `*斜*` `` `代码` ``、`[字](jdt://…)` 链接 —— 只认这几种，
 * 认不出的照原文当字显示，不丢东西
 */

export type Span = { t: string; code?: true; bold?: true };
export type Block = { kind: "code"; text: string } | { kind: "para"; spans: Span[] } | { kind: "item"; depth: number; spans: Span[] };

/** 一行里的行内标记 → 几段。链接只留字（`jdt://` 这种点了也去不了）；转义的 `\*` 还原成字 */
export function spansOf(line: string): Span[] {
  const s = line.replace(/\[([^\]]*)\]\([^)]*\)/g, "$1");
  const out: Span[] = [];
  const re = /`([^`]+)`|\*\*([^*]+)\*\*|__([^_]+)__|\*([^*\s][^*]*)\*/g;
  let last = 0;
  for (let m = re.exec(s); m; m = re.exec(s)) {
    if (m.index > last) out.push({ t: unescape(s.slice(last, m.index)) });
    if (m[1] !== undefined) out.push({ t: m[1], code: true });
    else out.push({ t: unescape(m[2] ?? m[3] ?? m[4]), bold: m[4] === undefined ? true : undefined });
    last = re.lastIndex;
  }
  if (last < s.length) out.push({ t: unescape(s.slice(last)) });
  // 斜体不加样子（只是去掉星号），bold 是 undefined 的那个键不留
  return out.map((x) => (x.bold === undefined ? (({ bold: _b, ...r }) => r)(x) : x)).filter((x) => x.t.length > 0);
}

const unescape = (s: string) => s.replace(/\\([\\`*_{}[\]()#+\-.!<>])/g, "$1");

export function docBlocks(md: string): Block[] {
  const out: Block[] = [];
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  let para: string[] = [];
  const flush = () => {
    if (para.length) out.push({ kind: "para", spans: spansOf(para.join(" ")) });
    para = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    const fence = /^\s*```/.exec(l);
    if (fence) {
      flush();
      const body: string[] = [];
      for (i++; i < lines.length && !/^\s*```/.test(lines[i]); i++) body.push(lines[i]);
      out.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    const item = /^(\s*)[*-]\s+(.*)$/.exec(l);
    if (item) {
      flush();
      out.push({ kind: "item", depth: Math.min(3, Math.floor(item[1].length / 2)), spans: spansOf(item[2]) });
      continue;
    }
    if (!l.trim()) flush();
    else para.push(l.trim());
  }
  flush();
  return out;
}
