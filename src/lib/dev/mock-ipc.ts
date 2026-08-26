/**
 * 浏览器里的 Tauri IPC 桩 —— 只在 `pnpm dev` 且不在 Tauri 里时装载。
 *
 * 为什么值得留着：调 UI 若走 Tauri，每改一行都要等约 40 秒重新编译壳；
 * 挂上这个桩后在浏览器里改，热更新是毫秒级。生产构建里
 * `import.meta.env.DEV` 为假，整个模块会被 tree-shake 掉。
 *
 * 喂的数据与 Rust 侧格式严格一致（含 log_lines 的线格式二进制），
 * 否则桩就失去了验证价值。
 */

const LINES = [
  "2026-08-24 14:03:21.442 INFO  [http-nio-exec-4] c.l.OrderService - 处理完成 orderId=8842011 cost=142ms status=SUCCESS",
  "2026-08-24 14:03:22.015 DEBUG [pool-3-thread-2] c.l.CacheManager - evict key=order:8842011 ttl=300s",
  "2026-08-24 14:03:25.512 WARN  [http-nio-exec-2] c.l.RetryPolicy - 重试 attempt=2/5 backing off 800ms cause=Read timeout",
  "2026-08-24 14:03:25.780 ERROR [scheduler-1] c.l.OrderService - 订单落库失败 orderId=8842013 cause=DeadlockLoserDataAccessException",
  "java.lang.IllegalStateException: connection pool exhausted",
  "\tat com.zaxxer.hikari.pool.HikariPool.createTimeoutException(HikariPool.java:696)",
  "\tat com.zaxxer.hikari.pool.HikariPool.getConnection(HikariPool.java:197)",
  "\tat com.liteide.OrderService.persist(OrderService.java:142)",
  "Caused by: java.sql.SQLTransientConnectionException: HikariPool-1 timed out",
  "2026-08-24 14:03:26.101 INFO  [kafka-listener-0] c.l.KafkaConsumer - 处理完成 orderId=8842014 cost=37ms status=SUCCESS",
  "2026-08-24 14:03:26.550 TRACE [main] c.l.InventoryLock - enter acquire(sku=A-1180)",
  "2026-08-24 14:03:27.003 INFO  [http-nio-exec-1] o.s.web.DispatcherServlet - Completed 200 OK in 12ms",
];

/** 级别在 LINES 里的下标，供桩内过滤用；顺序同 Rust 侧 Level */
const LINE_LEVEL = [2, 3, 1, 0, 5, 5, 5, 5, 5, 2, 4, 2];

const TOTAL = 9_141_707;
const enc = new TextEncoder();

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

const lineAt = (n: number) => LINES[n % LINES.length];

let filterHits: number[] | null = null;

function runFilter(levelBits: number, pattern: string, caseSensitive: boolean): number[] {
  const hits: number[] = [];
  const pat = caseSensitive ? pattern : pattern.toLowerCase();
  // 桩只在前 5 万行上筛，够验证交互，不必真跑 900 万
  for (let n = 0; n < 50_000; n++) {
    if ((levelBits & (1 << LINE_LEVEL[n % LINES.length])) === 0) continue;
    if (pat) {
      const text = caseSensitive ? lineAt(n) : lineAt(n).toLowerCase();
      if (!text.includes(pat)) continue;
    }
    hits.push(n);
  }
  return hits;
}

export function installMockIpc(): void {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    metadata: { currentWebview: { label: "main" }, currentWindow: { label: "main" } },
    transformCallback: (cb: unknown) => {
      const id = Math.floor(Math.random() * 1e9);
      (window as unknown as Record<string, unknown>)[`_cb${id}`] = cb;
      return id;
    },
    invoke: async (cmd: string, args: Record<string, never>) => {
      const a = args as unknown as Record<string, number & string & boolean>;
      switch (cmd) {
        case "initial_file":
          return "/var/log/app.log";
        case "open_log":
          return { handle: 1, name: "access-2026-08-24.log", size: 1_073_741_885 };
        case "log_stat":
          return {
            lineCount: TOTAL,
            indexedBytes: 1_073_741_885,
            totalBytes: 1_073_741_885,
            complete: true,
            indexBytes: 71_472,
            levels: [456_822, 914_684, 5_026_804, 2_742_487, 0, 910],
            levelsComplete: true,
            levelsScanned: 1_073_741_885,
          };
        case "log_lines": {
          const out: string[] = [];
          const n = Math.min(Number(a.count), TOTAL - Number(a.start));
          for (let i = 0; i < n; i++) out.push(lineAt(Number(a.start) + i));
          return encodeBlock(Number(a.start), out);
        }
        case "log_filter": {
          const bits = Number(a.levelBits);
          const pat = String(a.pattern ?? "");
          if (bits === 0b111111 && !pat) {
            filterHits = null;
            return false;
          }
          filterHits = runFilter(bits, pat, Boolean(a.caseSensitive));
          return true;
        }
        case "log_filter_stat":
          return filterHits === null
            ? null
            : { hits: filterHits.length, complete: true, scannedLines: 50_000 };
        case "log_lines_filtered": {
          if (!filterHits) return encodeBlock(Number(a.start), []);
          const slice = filterHits.slice(Number(a.start), Number(a.start) + Number(a.count));
          return encodeBlock(slice[0] ?? 0, slice.map(lineAt));
        }
        case "log_filter_map":
          return filterHits
            ? filterHits.slice(Number(a.start), Number(a.start) + Number(a.count))
            : [];
        case "log_refresh":
          return { kind: "none", newLines: 0, lineCount: TOTAL };
        case "close_log":
          filterHits = null;
          return true;
        default:
          return null;
      }
    },
  };
  // eslint-disable-next-line no-console
  console.info("[dev] Tauri IPC 桩已装载 —— 数据是假的，用于纯前端调试");
}
