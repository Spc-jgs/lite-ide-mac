/**
 * 桩：跨文件替换（#42），对应 `src-tauri/src/commands/replace.rs`。
 *
 * 照真实现的规则走（不然浏览器里试出来的和 .app 里不一样，桩就开始骗人）：
 * 命中由 `grepMatcher`（和 Rust 的 Matcher 同一套开关语义）算；位置是 UTF-16（JS 字符串本来就是）；
 * 执行前核对内容没变过；撤销前核对此刻还是「改后」那份。
 * 不模拟的：两段提交、替换日志落盘、崩溃恢复（那些是盘上的事，在 replacesvc 的测试里验）；跨行的命中（桩只按行找，
 * `lines` 恒为 1 —— 跨行的界面要在真 .app 上看，smoke ㉒ 有一条）；
 * `$<name>` 这类 JS 和 Rust 正则方言不同的展开写法。
 */
import { type A, bump, FILES, grepMatcher, NOT_MINE } from "./data";

type Hit = { start: number; end: number; line: number; col: number; text: string; spans: [number, number][]; lines: number; block: string | null };
type File = { rel: string; path: string; editor: boolean; text: string; hits: Hit[] };

let last: { files: File[]; regex: boolean; matcher: (t: string) => [number, number][]; re: RegExp | null } | null = null;
/** 「替换日志」：只在内存里，刷新页面就没了（真实现在盘上） */
let journal: { path: string; rel: string; before: string; after: string }[] | null = null;

const ROOT = "/proj/";

function hitsOf(text: string, find: (t: string) => [number, number][]): Hit[] {
  const out: Hit[] = [];
  const lines = text.split("\n");
  let off = 0;
  lines.forEach((ln, i) => {
    for (const [a, b] of find(ln)) {
      out.push({ start: off + a, end: off + b, line: i + 1, col: a + 1, text: ln.slice(0, 400), spans: [[a, Math.min(b, 400)]], lines: 1, block: null });
    }
    off += ln.length + 1;
  });
  return out;
}

/** 这一处换成什么：正则模式下照 JS 的 `$1` 展开，字面量原样 */
function replacementFor(f: File, h: Hit, repl: string): string {
  if (!last?.regex || !last.re) return repl;
  const m = f.text.slice(h.start, h.end);
  return m.replace(new RegExp(last.re.source, last.re.flags.replace("g", "")), repl);
}

export async function replaceCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    case "replace_scan": {
      const matcher = grepMatcher(String(a.pattern), a);
      const dirty = new Map<string, string>();
      for (const d of (a.open as unknown as { path: string; text: string; dirty: boolean }[]) ?? []) {
        if (d.dirty) dirty.set(d.path, d.text);
      }
      const files: File[] = [];
      for (const [path, content] of Object.entries(FILES)) {
        if (!path.startsWith(ROOT)) continue;
        const editor = dirty.has(path);
        const text = editor ? dirty.get(path)! : content;
        const hits = hitsOf(text, matcher);
        if (hits.length) files.push({ rel: path.slice(ROOT.length), path, editor, text, hits });
      }
      files.sort((x, y) => x.rel.localeCompare(y.rel));
      let re: RegExp | null = null;
      try {
        re = a.regex ? new RegExp(String(a.pattern), a.case ? "gmu" : "gimu") : null;
      } catch {
        /* grepMatcher 已经报过了 */
      }
      last = { files, regex: !!a.regex, matcher, re };
      const total = files.reduce((n, f) => n + f.hits.length, 0);
      return {
        files: files.map((f) => ({ rel: f.rel, path: f.path, editor: f.editor, hits: f.hits.map(({ line, col, text, spans, lines, block }) => ({ line, col, text, spans, lines, block })) })),
        skipped: [],
        binary: 0,
        total,
        truncated: total > 5000,
        indexTruncated: false,
      };
    }
    case "replace_preview": {
      if (!last) throw new Error("没有扫描结果，先搜一次");
      const repl = String(a.replacement);
      return last.files.map((f) =>
        f.hits.map((h) => {
          const ls = f.text.lastIndexOf("\n", h.start - 1) + 1;
          const le = f.text.indexOf("\n", h.end) < 0 ? f.text.length : f.text.indexOf("\n", h.end);
          const r = replacementFor(f, h, repl);
          const text = f.text.slice(ls, h.start) + r + f.text.slice(h.end, le);
          return { text: text.slice(0, 400), spans: [[h.start - ls, h.start - ls + r.length]], block: null };
        }),
      );
    }
    case "replace_apply": {
      if (!last) throw { code: "failed", message: "没有扫描结果，先搜一次", bytes: 0 };
      if (last.files.reduce((n, f) => n + f.hits.length, 0) > 5000) throw { code: "truncated", message: "命中太多，缩小范围再替换", bytes: 0 };
      const repl = String(a.replacement);
      const picks = a.picks as unknown as { rel: string; hits: number[] }[];
      const changed = [];
      const skipped = [];
      journal = [];
      for (const p of picks) {
        const f = last.files.find((x) => x.rel === p.rel);
        if (!f) continue;
        const now = f.editor ? f.text : FILES[f.path];
        if (now !== f.text) {
          skipped.push({ rel: f.rel, why: "changed", text: "预览之后被改过，没动它" });
          continue;
        }
        const idx = [...new Set(p.hits)].filter((i) => i < f.hits.length).sort((x, y) => x - y);
        let out = "";
        let at = 0;
        const edits = [];
        for (const i of idx) {
          const h = f.hits[i];
          const r = replacementFor(f, h, repl);
          out += f.text.slice(at, h.start) + r;
          edits.push({ from: h.start, to: h.end, insert: r });
          at = h.end;
        }
        out += f.text.slice(at);
        if (!f.editor) {
          FILES[f.path] = out;
          bump(f.path);
        }
        journal.push({ path: f.path, rel: f.rel, before: f.text, after: out });
        changed.push({ rel: f.rel, path: f.path, count: idx.length, edits, onDisk: !f.editor });
      }
      last = null;
      return { changed, skipped, undo: true };
    }
    case "replace_undo": {
      if (!journal) throw new Error("没有可以撤销的替换");
      const open = new Map((a.open as unknown as { path: string; text: string; dirty: boolean }[]).map((d) => [d.path, d]));
      const changed = [];
      const skipped = [];
      for (const e of journal) {
        const o = open.get(e.path);
        const now = o ? o.text : FILES[e.path];
        if (now !== e.after) {
          skipped.push({ rel: e.rel, why: "edited-since", text: "替换之后又改过，没动它" });
          continue;
        }
        const onDisk = !o || !o.dirty;
        if (onDisk) {
          FILES[e.path] = e.before;
          bump(e.path);
        }
        changed.push({ rel: e.rel, path: e.path, count: 1, edits: o ? [{ from: 0, to: o.text.length, insert: e.before }] : [], onDisk });
      }
      journal = null;
      return { changed, skipped, undo: false };
    }
    case "replace_pending":
      return journal ? { state: "done", root: "/proj", atMs: Date.now(), files: journal.length, changed: journal.length } : null;
    case "replace_recover":
      return { changed: [], skipped: [], undo: !!journal };
    default:
      return NOT_MINE;
  }
}
