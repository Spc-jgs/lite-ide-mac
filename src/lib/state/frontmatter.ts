/**
 * 草稿文件头的锚点（M10）。和 Rust 侧 `fsservice::frontmatter` 是**同一套判据**，
 * 两边都要认：Rust 在列表里读，前端在标签名 / 编辑器里读。
 *
 * 只认最窄的一种形状：第一行恰好是 `---`，接着若干 `key: value`，再一行 `---`。
 * 正文里自己写的 `---` 分隔线不在第一行，不会被误认。头最多 32 行。
 */
export interface Anchor {
  project: string;
  branch: string;
  head: string;
  /** `相对路径:行` */
  at: string;
}

/** `(锚点, 正文起点的字符偏移)`。没有头就是 `[null, 0]` */
export function splitFrontmatter(text: string): [Anchor | null, number] {
  if (!text.startsWith("---\n") && !text.startsWith("---\r\n")) return [null, 0];
  let off = text.indexOf("\n") + 1;
  const a: Anchor = { project: "", branch: "", head: "", at: "" };
  for (let i = 0; i < 32; i++) {
    const nl = text.indexOf("\n", off);
    const line = nl < 0 ? text.slice(off) : text.slice(off, nl);
    off = nl < 0 ? text.length : nl + 1;
    const t = line.replace(/\r$/, "");
    if (t === "---") {
      // 头后面紧跟的那个空行也算头的一部分
      if (text[off] === "\n") off += 1;
      return [a, off];
    }
    const c = t.indexOf(":");
    if (c > 0) {
      const k = t.slice(0, c).trim() as keyof Anchor;
      if (k in a) a[k] = t.slice(c + 1).trim();
    }
    if (nl < 0) break;
  }
  return [null, 0];
}

/** `src/lib/x.svelte:347` → `{ path, line }`；没有 `:行` 就 line 为 undefined */
export function parseAt(at: string): { path: string; line?: number } | null {
  if (!at) return null;
  const m = /^(.*?)(?::(\d+))?$/.exec(at);
  if (!m || !m[1]) return null;
  return m[2] ? { path: m[1], line: Number(m[2]) } : { path: m[1] };
}

/**
 * 草稿的标签名：正文第一行有字的内容（跳过文件头的锚点），截到 30 个字符；
 * 一个字没有就回落到文件名。从 `tab.ts` 搬过来（M10）—— 它要读文件头，
 * 而 `tab.ts` 是 node 直接跑测试的模块，不能有带路径的值导入。
 */
export function scratchTitle(text: string): string | undefined {
  const [, body] = splitFrontmatter(text);
  for (const raw of text.slice(body).split("\n", 40)) {
    const line = raw.trim().replace(/^#+\s*/, "").trim();
    if (line) return [...line].slice(0, 30).join("");
  }
  return undefined;
}
