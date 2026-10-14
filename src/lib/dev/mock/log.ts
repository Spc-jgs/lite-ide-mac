/** 桩：日志引擎：打开、统计、按行取、过滤、按时间跳、刷新、关闭。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, BIG_LOG, existsInMock, FILES, LOG_SRC, type LogSrc, nameOf, NOT_MINE, src, TOTAL } from "./data";
import { parseQuery, matchLine, isEmptyQuery } from "../../logview/query";
import { encodeBlock } from "./block";

/** 认级别：和 Rust 侧 `logengine` 一样，按行里出现的那个词判 */
const LEVEL_WORDS = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];
function levelOf(line: string): number {
  const i = LEVEL_WORDS.findIndex((w) => line.includes(w));
  return i < 0 ? 5 : i;
}

function srcOf(path: string): LogSrc {
  const text = FILES[path];
  if (text === undefined) return BIG_LOG;
  const lines = text.replace(/\n$/, "").split("\n");
  const levels = [0, 0, 0, 0, 0, 0];
  for (const l of lines) levels[levelOf(l)]++;
  return {
    at: (n) => lines[n] ?? "",
    total: lines.length,
    levelAt: (n) => levelOf(lines[n] ?? ""),
    levels,
    bytes: text.length,
  };
}

let filterHits: number[] | null = null;
/** log_filter_stat 被问了几次 —— 前三次说「还没扫完」 */
let filterStatCalls = 0;
/** log_stat 按句柄被问了几次 —— 前几次说「索引中 / 级别扫描中」 */
const logStatCalls = new Map<string, number>();

/** 句柄 → 打开时那条路径。真实现的 LogFile 也是这么存的 */
const LOG_PATHS: Record<number, string> = {};
let nextLogHandle = 1;
function runFilter(s: LogSrc, levelBits: number, pattern: string, caseSensitive: boolean): number[] {
  const hits: number[] = [];
  // 语法和真实现同一套（`logview/query.ts`，Rust 侧 `logengine::query`）
  const q = parseQuery(pattern);
  // 桩只在前 5 万行上筛，够验证交互，不必真跑 900 万
  const n1 = Math.min(s.total, 50_000);
  for (let n = 0; n < n1; n++) {
    if ((levelBits & (1 << s.levelAt(n))) === 0) continue;
    if (!isEmptyQuery(q) && !matchLine(s.at(n), q, caseSensitive)) continue;
    hits.push(n);
  }
  return hits;
}

/*
 * 合并视图（真实现在 logengine::merge）。桩照同一套语义真合一遍：认行首的 `YYYY-MM-DD HH:MM:SS(.mmm)` 和裸的 `HH:MM:SS(.mmm)`，
 * 没时间戳的行沿用上一条（堆栈不被拆散），开头的横幅用这份第一条的时间，同一毫秒按第几份定先后，只有时分秒的借第一份带日期的那天。
 * 每份只取前 5 万行（同过滤那条），够验交互
 */
interface Merged {
  paths: string[];
  offsets: number[];
  srcs: LogSrc[];
  clocks: ("full" | "timeOnly" | "none")[];
  fillDay: number | null;
  src: number[];
  line: number[];
}
const MERGES: Record<number, Merged> = {};
const DAY = 86_400_000;

function tsOfLine(line: string, fillDay: number | null): { ms: number; full: boolean } | null {
  let m = /^\[?(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:[.,](\d{1,3}))?/.exec(line);
  if (m) return { ms: Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6], +(m[7] ?? "0").padEnd(3, "0")), full: true };
  m = /^(\d{2}):(\d{2}):(\d{2})(?:[.,](\d{1,3}))?/.exec(line);
  if (m) return { ms: (fillDay ?? 0) + ((+m[1] * 60 + +m[2]) * 60 + +m[3]) * 1000 + +(m[4] ?? "0").padEnd(3, "0"), full: false };
  return null;
}

function buildMerge(paths: string[], offsets: number[]): Merged {
  const srcs = paths.map(srcOf);
  const n = srcs.map((s) => Math.min(s.total, 50_000));
  const firsts = srcs.map((s, i) => {
    for (let l = 0; l < Math.min(n[i], 2000); l++) {
      const t = tsOfLine(s.at(l), 0);
      if (t) return t;
    }
    return null;
  });
  const full = firsts.filter((t) => t?.full).map((t) => t!.ms);
  const fillDay = full.length ? Math.floor(Math.min(...full) / DAY) * DAY : null;
  const clocks = firsts.map((t) => (!t ? "none" : t.full ? "full" : "timeOnly") as Merged["clocks"][number]);
  // 每份每行的键：有时间戳用它（加偏移），没有就沿用上一条；开头的用第一条
  const keys = srcs.map((s, i) => {
    const out: number[] = [];
    let c = -Infinity;
    for (let l = 0; l < Math.min(n[i], 2000); l++) {
      const t = tsOfLine(s.at(l), fillDay);
      if (t) {
        c = t.ms;
        break;
      }
    }
    for (let l = 0; l < n[i]; l++) {
      const t = tsOfLine(s.at(l), fillDay);
      if (t) c = t.ms;
      out.push(c + offsets[i]);
    }
    return out;
  });
  const ptr = n.map(() => 0);
  const src: number[] = [];
  const line: number[] = [];
  for (;;) {
    let best = -1;
    for (let i = 0; i < n.length; i++) {
      if (ptr[i] >= n[i]) continue;
      if (best < 0 || keys[i][ptr[i]] < keys[best][ptr[best]]) best = i;
    }
    if (best < 0) break;
    src.push(best);
    line.push(ptr[best]++);
  }
  return { paths, offsets, srcs, clocks, fillDay, src, line };
}

function mergedSrc(m: Merged): LogSrc {
  const levels = [0, 0, 0, 0, 0, 0];
  for (const s of m.srcs) s.levels.forEach((v, i) => (levels[i] += v));
  return {
    at: (r) => m.srcs[m.src[r]]?.at(m.line[r]) ?? "",
    total: m.src.length,
    levelAt: (r) => m.srcs[m.src[r]]?.levelAt(m.line[r]) ?? 5,
    levels,
    bytes: m.srcs.reduce((a, s) => a + s.bytes, 0),
  };
}

function mergeInfo(h: number, m: Merged) {
  const fill = m.fillDay === null ? null : new Date(m.fillDay).toISOString().slice(0, 10);
  return {
    handle: h,
    sources: m.paths.map((p, i) => ({ path: p, name: nameOf(p), clock: m.clocks[i], offsetMs: m.offsets[i] })),
    fillDate: fill,
    size: m.srcs.reduce((a, s) => a + s.bytes, 0),
  };
}

/*
 * 日志归类（真实现在 logengine::pattern，Drain）。桩做简化版：按真实现同一套「带数字的那段换成 <*>」归一，**归一后整行一样的算一类**，
 * 不做 Drain 的「过半就合并」—— 演示数据里够用。带时间戳的源只归有时间戳的行。前两次问说「还在归」（真实现大文件要跑几秒，
 * 浮层的「还在归」和状态栏进度只在那段时间里有，同 log_filter_stat 那条）
 */
const patternCalls = new Map<number, number>();
function normTok(t: string): string {
  return t.replace(/[\p{L}\p{N}]+/gu, (m) => (/\d/.test(m) ? "<*>" : m));
}
function mockPatterns(s: LogSrc) {
  const n = Math.min(s.total, 50_000);
  const ts = /^(\[?\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}|\d{2}:\d{2}:\d{2})/;
  let hasTs = false;
  for (let l = 0; l < Math.min(n, 2000) && !hasTs; l++) hasTs = ts.test(s.at(l));
  const groups = new Map<string, { count: number; levels: number[] }>();
  let lines = 0;
  for (let l = 0; l < n; l++) {
    const line = s.at(l);
    if (hasTs ? !ts.test(line) : !line.trim() || /^\s+at /.test(line)) continue;
    lines++;
    const key = line.split(/\s+/).filter(Boolean).slice(0, 40).map(normTok).join(" ");
    const g = groups.get(key) ?? { count: 0, levels: [0, 0, 0, 0, 0, 0] };
    g.count++;
    g.levels[s.levelAt(l)]++;
    groups.set(key, g);
  }
  return { groups: [...groups].sort((x, y) => y[1].count - x[1].count), lines };
}

export async function logCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    case "open_log": {
      /*
       * 记住**打开时那条路径**，因为真实现就是这么存的
       * （`LogFile { path, inode, .. }`），而 refresh 走的是
       * `std::fs::metadata(&self.path)`。桩里原来 refresh 一律返回
       * "none"，于是「文件被改名之后 tail 还能不能用」这条路
       * 在浏览器里根本走不到 —— 而它是真会断的。
       */
      const h = nextLogHandle++;
      LOG_PATHS[h] = String(a.path);
      LOG_SRC[h] = srcOf(String(a.path));
      return { handle: h, name: nameOf(String(a.path)), size: LOG_SRC[h].bytes };
    }
    case "log_stat": {
      const s = src(a.handle);
      /*
       * 故意分三步：真实现索引 1GB 要一秒多、级别扫描再一秒，状态栏「索引中 / 级别扫描中」
       * 和级别胶囊上的「…」只在那段时间存在。轮询 100ms 一次：前 3 次「索引中」（行数按比例长），
       * 再 3 次「级别扫描中」，之后齐了。同 git_commit 那条的道理（rules/frontend.md）。
       */
      const n = (logStatCalls.get(String(a.handle)) ?? 0) + 1;
      logStatCalls.set(String(a.handle), n);
      const indexing = n <= 3;
      const scanning = n <= 6;
      return {
        lineCount: indexing ? Math.floor((s.total * n) / 3) : s.total,
        indexedBytes: indexing ? Math.floor((s.bytes * n) / 3) : s.bytes,
        totalBytes: s.bytes,
        complete: !indexing,
        indexBytes: 71_472,
        levels: scanning ? [0, 0, 0, 0, 0, 0] : s.levels,
        levelsComplete: !scanning,
        levelsScanned: scanning ? Math.floor((s.bytes * Math.max(0, n - 3)) / 3) : s.bytes,
      };
    }
    case "log_lines": {
      const s = src(a.handle);
      const out: string[] = [];
      const n = Math.min(Number(a.count), s.total - Number(a.start));
      for (let i = 0; i < n; i++) out.push(s.at(Number(a.start) + i));
      return encodeBlock(Number(a.start), out);
    }
    case "log_filter": {
      const bits = Number(a.levelBits);
      const pat = String(a.pattern ?? "");
      // 合并视图：只看哪几份（真实现在 MergedLog::start_filter）。全看 = 没有这一条
      const m = MERGES[Number(a.handle)];
      const all = m ? (1 << m.paths.length) - 1 : 0;
      const mask = m && a.sources != null ? Number(a.sources) & all : all;
      // 真实现按 `is_noop` 判：全级别 + 切完没有条件 = 清除过滤（`"  "`、`//` 都算空）
      if (bits === 0b111111 && isEmptyQuery(parseQuery(pat)) && mask === all) {
        filterHits = null;
        return false;
      }
      filterHits = runFilter(src(a.handle), bits, pat, Boolean(a.caseSensitive));
      if (m && mask !== all) filterHits = filterHits.filter((r) => (mask & (1 << m.src[r])) !== 0);
      filterStatCalls = 0;
      return true;
    }
    /*
     * 跳到时间（真实现在 logengine::seek，二分）。桩只认 `YYYY-MM-DD HH:MM:SS` 开头的行，
     * 在前 5 万行里线性找第一条不早于目标的 —— 够验交互；日期不带就按第一条有时间戳的行补。
     */
    case "log_seek_time": {
      const s = src(a.handle);
      const q = String(a.query).trim();
      const m = /^(?:(\d{4}-\d{2}-\d{2})[ T])?(\d{2}:\d{2}(?::\d{2}(?:[.,]\d+)?)?)$/.exec(q);
      if (!m) return null;
      const tsOf = (n: number) => /^(\d{4}-\d{2}-\d{2}) (\d{2}:\d{2}:\d{2}(?:[.,]\d+)?)/.exec(s.at(n));
      const n1 = Math.min(s.total, 50_000);
      let date: string | undefined = m[1];
      for (let n = 0; !date && n < n1; n++) date = tsOf(n)?.[1];
      if (!date) return null;
      const key = `${date} ${m[2]}`;
      for (let n = 0; n < n1; n++) {
        const t = tsOf(n);
        if (t && `${t[1]} ${t[2]}` >= key) return n;
      }
      return n1 - 1;
    }
    case "log_filter_stat": {
      if (filterHits === null) return null;
      /*
       * 故意分四次才 complete：真实现扫 1GB 要一两秒，过滤条上「N 条 + 环」那个状态
       * 只在扫描中间存在，桩一次到位的话它在浏览器里一次都看不到（同 git_commit 那条）。
       * 命中数按比例长上去，看得出是在扫。
       */
      filterStatCalls++;
      const done = filterStatCalls >= 4;
      const hits = done ? filterHits.length : Math.floor((filterHits.length * filterStatCalls) / 4);
      // scannedLines 按总行数比例走：状态栏的进度格用它算百分比，写死 12,500 在 900 万行里永远是 0%
      const total = src(a.handle).total;
      return { hits, complete: done, scannedLines: done ? total : Math.floor((total * filterStatCalls) / 4) };
    }
    case "log_lines_filtered": {
      if (!filterHits) return encodeBlock(Number(a.start), []);
      const slice = filterHits.slice(Number(a.start), Number(a.start) + Number(a.count));
      return encodeBlock(slice[0] ?? 0, slice.map(src(a.handle).at));
    }
    case "log_filter_map":
      return filterHits
        ? filterHits.slice(Number(a.start), Number(a.start) + Number(a.count))
        : [];
    case "log_refresh": {
      // 合并视图不跟 tail（真实现同）
      if (MERGES[Number(a.handle)]) return { kind: "none", newLines: 0, lineCount: src(a.handle).total };
      const p = LOG_PATHS[Number(a.handle)];
      if (p === undefined) throw new Error("句柄已失效");
      // 真实现在这里会 metadata(&self.path) 失败 —— 文件改了名，
      // 而引擎记着的还是旧路径
      if (!existsInMock(p)) throw new Error(`刷新失败：${p} (os error 2)`);
      // 模拟一次轮转：localStorage 里 `lite-ide.mock-rotate` = "1" 就报一次 rotated
      // 然后清掉标志。真实现在这一步已经按名重开、过滤任务清空 —— 桩也把命中表清掉，
      // 前端要能靠自己按原条件重跑
      let rotated = false;
      try {
        rotated = localStorage.getItem("lite-ide.mock-rotate") === "1";
        if (rotated) localStorage.removeItem("lite-ide.mock-rotate");
      } catch {
        /* 私密窗口等拿不到 localStorage，当没有 */
      }
      if (rotated) {
        filterHits = null;
        return { kind: "rotated", newLines: 0, lineCount: TOTAL };
      }
      return { kind: "none", newLines: 0, lineCount: TOTAL };
    }
    case "open_merged": {
      const paths = (a.paths as string[]).map(String);
      if (paths.length < 2) throw new Error(`合并不了：合并要 2 到 32 份日志，给了 ${paths.length} 份`);
      const h = nextLogHandle++;
      const m = buildMerge(paths, paths.map(() => 0));
      MERGES[h] = m;
      LOG_SRC[h] = mergedSrc(m);
      return mergeInfo(h, m);
    }
    case "merge_rows": {
      const m = MERGES[Number(a.handle)];
      if (!m) return { src: [], line: [] };
      const start = Number(a.start);
      const rows = a.filtered
        ? (filterHits ?? []).slice(start, start + Number(a.count))
        : Array.from({ length: Math.max(0, Math.min(Number(a.count), m.src.length - start)) }, (_, i) => start + i);
      return { src: rows.map((r) => m.src[r]), line: rows.map((r) => m.line[r]) };
    }
    case "merge_set_offsets": {
      const h = Number(a.handle);
      const old = MERGES[h];
      if (!old) throw new Error("句柄已失效");
      const m = buildMerge(old.paths, (a.offsets as number[]).map(Number));
      MERGES[h] = m;
      LOG_SRC[h] = mergedSrc(m);
      // 真实现：重新归并 + 清掉过滤；log_stat 从「合并中」重新走一遍
      filterHits = null;
      logStatCalls.delete(String(h));
      return mergeInfo(h, m);
    }
    case "log_patterns": {
      const h = Number(a.handle);
      const calls = a.restart ? 1 : (patternCalls.get(h) ?? 0) + 1;
      patternCalls.set(h, calls);
      const done = calls >= 3;
      const { groups, lines } = mockPatterns(src(h));
      const shown = done ? groups : groups.slice(0, Math.ceil(groups.length / 2));
      const enc = new TextEncoder();
      const bytes = src(h).bytes;
      return {
        patterns: shown.slice(0, Number(a.limit)).map(([t, g]) => ({ template: [...enc.encode(t)], count: g.count, base: 0, levels: g.levels })),
        total: shown.length,
        lines: done ? lines : Math.floor(lines / 2),
        baseLines: 0,
        overflow: 0,
        scannedBytes: done ? bytes : Math.floor(bytes / 2),
        totalBytes: bytes,
        done,
      };
    }
    /*
     * 对比（真实现：两边放进同一个 Drain 一起归，Rust 的 change_rank 排序）。桩：两边各按归一后的整行分组、按同一个模板认同一类，
     * 排序照真实现的三档：新出现 → 没了 → 按频率的变化幅度。一次就 done（对比的「还在归」和上面那条是同一个界面状态，那边已经演过）
     */
    case "log_patterns_compare": {
      const cur = mockPatterns(src(Number(a.handle)));
      const base = (a.base as string[]).map((p) => mockPatterns(srcOf(String(p))));
      const baseLines = base.reduce((x, b) => x + b.lines, 0);
      const all = new Map<string, { count: number; base: number; levels: number[] }>();
      for (const [t, g] of cur.groups) all.set(t, { count: g.count, base: 0, levels: [...g.levels] });
      for (const b of base)
        for (const [t, g] of b.groups) {
          const e = all.get(t) ?? { count: 0, base: 0, levels: [0, 0, 0, 0, 0, 0] };
          e.base += g.count;
          g.levels.forEach((v, i) => (e.levels[i] += v));
          all.set(t, e);
        }
      const rank = (e: { count: number; base: number }): [number, number] => {
        if (e.base === 0) return [0, e.count];
        if (e.count === 0) return [1, e.base];
        const r = e.count / Math.max(1, cur.lines) / (e.base / Math.max(1, baseLines));
        return [2, Math.max(r, 1 / r)];
      };
      const list = [...all].sort((x, y) => {
        const [ka, va] = rank(x[1]);
        const [kb, vb] = rank(y[1]);
        return ka - kb || vb - va;
      });
      const enc = new TextEncoder();
      return {
        patterns: list.slice(0, Number(a.limit)).map(([t, e]) => ({ template: [...enc.encode(t)], count: e.count, base: e.base, levels: e.levels })),
        total: list.length,
        lines: cur.lines,
        baseLines,
        overflow: 0,
        scannedBytes: 1,
        totalBytes: 1,
        done: true,
      };
    }
    case "close_log":
      patternCalls.delete(Number(a.handle));
      delete MERGES[Number(a.handle)];
      delete LOG_PATHS[Number(a.handle)];
      filterHits = null;
      return true;
    default:
      return NOT_MINE;
  }
}
