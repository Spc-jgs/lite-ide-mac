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

/** 桩里的假文件系统：路径 → 内容 */
const FILES: Record<string, string> = {
  "/proj/src/OrderService.java": `package com.liteide.order;

import java.util.List;
import org.springframework.stereotype.Service;

/**
 * 订单服务。桩数据，用于在浏览器里验证语法着色。
 */
@Service
public class OrderService {
    private static final int MAX_RETRY = 5;
    private final OrderRepository repo;

    public OrderService(OrderRepository repo) {
        this.repo = repo;
    }

    public Order persist(Order order) {
        if (order == null) {
            throw new IllegalArgumentException("order 不能为空");
        }
        for (int i = 0; i < MAX_RETRY; i++) {
            try {
                return repo.save(order);
            } catch (DeadlockException e) {
                log.warn("重试 attempt={}/{}", i + 1, MAX_RETRY);
            }
        }
        return null;
    }
}
`,
  "/proj/src/main.py": `import asyncio
from dataclasses import dataclass


@dataclass
class Order:
    id: int
    amount: float = 0.0

    def total(self) -> float:
        # 含税总价
        return self.amount * 1.13


async def main():
    orders = [Order(i, i * 10.0) for i in range(5)]
    for o in orders:
        print(f"order {o.id}: {o.total():.2f}")
    await asyncio.sleep(0)


if __name__ == "__main__":
    asyncio.run(main())
`,
  "/proj/README.md": `# lite-ide

> Mac 上 1 秒打开的个人工作台。

## 特性

- **GB 级日志秒开** —— mmap + 稀疏索引，内存与文件大小无关
- 代码高亮*够用就停*
- Markdown 所见即所得，磁盘上永远是纯 \`.md\`
- ~~插件系统~~ 永远不做

### 性能

打开 1GB 日志只要 \`0.38ms\`，常驻内存 98MB。

---

参见 [架构文档](docs/ARCHITECTURE.md) 与 [开发日志](docs/JOURNAL.md)。

1. 先做最难的
2. 再做确定的
`,
  "/proj/vite.config.ts": `import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "safari15", minify: "esbuild" },
});
`,
};

const DIRS: Record<string, Array<[string, boolean]>> = {
  "/proj": [["src", true], ["logs", true], ["docs", true], ["README.md", false], ["vite.config.ts", false]],
  "/proj/src": [["OrderService.java", false], ["main.py", false]],
  "/proj/logs": [["access-2026-08-24.log", false]],
  "/proj/docs": [["ARCHITECTURE.md", false]],
};
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
        case "initial_path":
          return "/proj";
        case "probe_path": {
          const path = String(a.path);
          if (DIRS[path]) {
            return { kind: "dir", mode: "edit", path, name: path.split("/").pop(), size: 0, reason: "" };
          }
          const isLog = path.endsWith(".log");
          return {
            kind: "file",
            mode: isLog ? "log" : "edit",
            path,
            name: path.split("/").pop(),
            size: isLog ? 1_073_741_885 : (FILES[path]?.length ?? 0),
            reason: isLog ? "文件超过 32MB" : "",
          };
        }
        case "list_dir": {
          const path = String(a.path);
          return (DIRS[path] ?? []).map(([name, isDir]) => ({
            name,
            path: `${path}/${name}`,
            isDir,
            size: isDir ? 0 : (FILES[`${path}/${name}`]?.length ?? 0),
          }));
        }
        case "read_text":
          return FILES[String(a.path)] ?? "// 桩里没有这个文件\n";
        case "write_text":
          FILES[String(a.path)] = String(a.content);
          return null;
        case "list_project_files":
          return Object.keys(FILES).map((f) => f.replace(/^\/proj\//, ""));
        case "grep_project": {
          const pat = String(a.pattern).toLowerCase();
          const out: Array<{ path: string; line: number; text: string }> = [];
          for (const [full, content] of Object.entries(FILES)) {
            const rel = full.replace(/^\/proj\//, "");
            content.split("\n").forEach((text, i) => {
              if (text.toLowerCase().includes(pat)) out.push({ path: rel, line: i + 1, text });
            });
          }
          return out.slice(0, Number(a.limit) || 60);
        }
        case "ripgrep_available":
          return true;
        case "pty_spawn":
          return 1;
        case "pty_write":
        case "pty_resize":
          return null;
        case "pty_kill":
        case "pty_alive":
          return true;
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
