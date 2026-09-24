/** 桩：日志引擎：打开、统计、按行取、过滤、按时间跳、刷新、关闭。从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。 */
import { type A, BIG_LOG, enc, existsInMock, FILES, LOG_SRC, type LogSrc, nameOf, NOT_MINE, src, TOTAL } from "./data";
import { parseQuery, matchLine, isEmptyQuery } from "../../logview/query";

/** 与 Rust 侧 block::encode 完全一致的线格式 */
function encodeBlock(first: number, texts: string[]): ArrayBuffer {
  const parts = texts.map((t) => enc.encode(t));
  const total = 12 + 4 * parts.length + parts.reduce((a, b) => a + b.length, 0);
  const buf = new ArrayBuffer(total);
  const dv = new DataView(buf);
  const u8 = new Uint8Array(buf);
  dv.setBigUint64(0, BigInt(first), true);
  dv.setUint32(8, parts.length, true);
  let pos = 12 + 4 * parts.length;
  parts.forEach((p, i) => {
    dv.setUint32(12 + i * 4, p.length, true);
    u8.set(p, pos);
    pos += p.length;
  });
  return buf;
}

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
      // 真实现按 `is_noop` 判：全级别 + 切完没有条件 = 清除过滤（`"  "`、`//` 都算空）
      if (bits === 0b111111 && isEmptyQuery(parseQuery(pat))) {
        filterHits = null;
        return false;
      }
      filterHits = runFilter(src(a.handle), bits, pat, Boolean(a.caseSensitive));
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
    case "close_log":
      delete LOG_PATHS[Number(a.handle)];
      filterHits = null;
      return true;
    default:
      return NOT_MINE;
  }
}
