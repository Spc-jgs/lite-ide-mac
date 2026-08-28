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

/** 桩里按文件名猜编码，用来在浏览器里把编码相关的界面路径走一遍 */
function encOf(path: string): string {
  if (path.includes("gbk")) return "GBK";
  if (path.includes("big5")) return "Big5";
  if (path.includes("sjis")) return "Shift_JIS";
  if (path.includes("bom")) return "UTF-8";
  return "UTF-8";
}

/** 造一条分支，字段与 Rust 侧 BranchDto 对齐 */
function b(name: string, isHead: boolean, isRemote: boolean, upstream: string, subject: string) {
  return { name, sha: name.slice(0, 7), upstream, isHead, isRemote, when: "2 天前", subject };
}

/**
 * 一段带合并的假历史。刻意造出「分支岔出去 → 各自提交 → 合并回来」，
 * 泳道图的三种情形（直线、分叉、汇合）在浏览器里就能一眼看全。
 */
const MOCK_LOG = [
  ["h8", "合并 M12：界面打磨与使用手册", ["h7", "f2"], ["HEAD", "m13/git"]],
  ["f2", "M12 界面打磨：侧边栏、终端字体、使用手册", ["f1"], []],
  ["f1", "终端字体改用具体字体名", ["h7"], []],
  ["h7", "合并 M11：符号大纲", ["h6", "e1"], ["main", "origin/main"]],
  ["e1", "M11 符号大纲：⌘⇧O 文件结构", ["h6"], ["m11/symbols"]],
  ["h6", "合并 M10：堆栈折叠", ["h5"], []],
  ["h5", "M10 堆栈折叠：复用过滤机制", ["h4"], []],
  ["h4", "M9 多终端标签", ["h3"], []],
  ["h3", "M8 语言与日志适配", ["h2"], []],
  ["h2", "M7 外部修改检测", ["h1"], []],
  ["h1", "M0 日志引擎垂直切片", [], []],
].map(([sha, subject, parents, refs], i) => ({
  sha: sha as string,
  short: (sha as string).padEnd(7, "0"),
  author: i % 3 === 0 ? "pc shao" : "李兆义",
  email: "dev@example.com",
  when: `${i + 1} 天前`,
  date: `2026-08-${String(26 - i).padStart(2, "0")}`,
  subject: subject as string,
  parents: parents as string[],
  refs: refs as string[],
}));

/** 造一条 git 状态条目，字段与 Rust 侧 GitEntryDto 严格对齐 */
function g(
  path: string,
  index: string,
  work: string,
  extra: { staged?: boolean; untracked?: boolean; isDir?: boolean; conflicted?: boolean; orig?: string } = {},
) {
  const untracked = extra.untracked ?? false;
  const conflicted = extra.conflicted ?? false;
  return {
    path,
    index,
    work,
    untracked,
    isDir: extra.isDir ?? false,
    conflicted,
    // 与 Rust 侧 Entry::staged / unstaged 完全一致 —— 包括「冲突条目
    // 既不算已暂存也不算未暂存」这条。桩要是和真实现分叉，它就没用了
    staged: !conflicted && !untracked && index !== "." && index !== " ",
    unstaged: !conflicted && (untracked || (work !== "." && work !== " ")),
    orig: extra.orig ?? null,
  };
}

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
  // 一份足够长的文件，用来验证缩略图「画不下时滑动」那条路径
  "/proj/src/long.ts": Array.from({ length: 900 }, (_, i) =>
    i % 11 === 0
      ? `// ${i} 分段注释：这一行是注释，缩略图上应该是灰色`
      : i % 7 === 0
        ? `const label${i} = "字符串 ${i}"; // 尾注`
        : i % 5 === 0
          ? `export function fn${i}(a: number, b: string): boolean {`
          : i % 5 === 1
            ? `    if (a > ${i}) { return true; }`
            : i % 5 === 2
              ? `    return b.length > ${i % 40};`
              : i % 5 === 3
                ? `}`
                : ``,
  ).join("\n"),
  "/proj/src/gbk-legacy.java": `// 这个文件在桩里被当成 GBK：状态栏该显示 GBK
public class LegacyOrder {
    // 订单处理失败，重试中
    private static final int RETRIES = 3;
}
`,
  "/proj/src/big5-notes.txt": `訂單處理失敗，重試中
第二行：庫存不足
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
  "/proj/package.json": `{
  "name": "lite-ide",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "dev": "vite",
    "app:build": "tauri build --no-bundle"
  },
  "dependencies": {
    "@codemirror/lang-json": "^6.0.2",
    "@tauri-apps/api": "^2"
  },
  "engines": { "node": ">=20" },
  "enabled": true,
  "retries": 5,
  "timeout": null
}
`,
  "/proj/Cargo.toml": `[package]
name = "lite-ide"
version = "0.1.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }

# 发布构建：体积与速度并重
[profile.release]
opt-level = 3
lto = true
strip = true
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
  "/proj": [["src", true], ["logs", true], ["docs", true], ["README.md", false], ["package.json", false], ["Cargo.toml", false], ["vite.config.ts", false]],
  "/proj/src": [["OrderService.java", false], ["main.py", false], ["gbk-legacy.java", false], ["big5-notes.txt", false], ["long.ts", false]],
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

/** 文件指纹。桩里用一个自增计数模拟 mtime */
const STAMPS: Record<string, { mtimeMs: number; size: number }> = {};
let clock = 1_700_000_000_000;
function bump(path: string) {
  clock += 1000;
  STAMPS[path] = { mtimeMs: clock, size: FILES[path]?.length ?? 0 };
  return STAMPS[path];
}
function stampOf(path: string) {
  return STAMPS[path] ?? bump(path);
}

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
  // 开发期钩子：在控制台模拟「文件被编辑器外改动」，用来验证冲突处理
  (window as unknown as Record<string, unknown>).__mockEditFileOutside = (
    path: string,
    content: string,
  ) => {
    FILES[path] = content;
    bump(path);
  };

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
        case "detect_encoding":
          return encOf(String(a.path));
        case "list_encodings":
          return [
            ["UTF-8", "UTF-8"],
            ["GB18030", "GB18030（简体中文，GBK 的超集）"],
            ["GBK", "GBK（简体中文）"],
            ["Big5", "Big5（繁体中文）"],
            ["Shift_JIS", "Shift_JIS（日文）"],
            ["UTF-16LE", "UTF-16 小端"],
          ];
        case "read_text": {
          // 形状必须与 Rust 侧 TextDto 一致，否则桩就失去了验证价值
          const p = String(a.path);
          const enc = (a.label as string) || encOf(p);
          return {
            content: FILES[p] ?? "// 桩里没有这个文件\n",
            encoding: enc,
            bom: p.includes("bom"),
            // 桩里模拟「按 UTF-8 读一个 GBK 文件」的乱码情形：
            // 换成 GBK 重新打开就不再有损，正好把状态栏的告警路径走一遍
            lossy: p.includes("gbk") && enc.toLowerCase() === "utf-8",
          };
        }
        case "write_text":
          FILES[String(a.path)] = String(a.content);
          return bump(String(a.path));
        case "file_stamp":
          return stampOf(String(a.path));
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
        case "pty_spawn":
          return 1;
        case "pty_write":
        case "pty_resize":
          return null;
        case "pty_kill":
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

        // ── Git ──
        // 造一份含所有状态位的假仓库：改动 / 新增 / 删除 / 改名 / 未跟踪目录 /
        // 冲突都占一条，浏览器里就能把染色和分组全看一遍。
        case "git_root":
          return "/proj";
        case "git_status":
          return {
            root: "/proj",
            branch: "m13/git",
            upstream: "origin/m13/git",
            ahead: 2,
            behind: 0,
            detached: false,
            unborn: false,
            truncated: false,
            entries: [
              g("src/OrderService.java", "M", ".", { staged: true }),
              g("src/App.svelte", ".", "M"),
              g("README.md", "A", "."),
              g("docs/old.md", ".", "D"),
              g("src/renamed.ts", "R", ".", { orig: "src/before.ts" }),
              g("scratch/", ".", "?", { untracked: true, isDir: true }),
              g("notes.txt", ".", "?", { untracked: true }),
              g("src/conflict.rs", "U", "U", { conflicted: true }),
            ],
          };
        case "git_diff":
          return {
            truncated: false,
            text: `diff --git a/${a.path} b/${a.path}
index 1a2b3c4..5d6e7f8 100644
--- a/${a.path}
+++ b/${a.path}
@@ -12,7 +12,8 @@ public void persist(Order order) {
     var conn = pool.getConnection();
-    int timeout = 300;
+    int timeout = 5000;
     try {
-        repo.save(order);
+        repo.saveAndFlush(order);
+        metrics.record("order.persist", order.id());
     } finally {
         conn.close();
     }`,
          };
        case "git_stage":
        case "git_unstage":
        case "git_discard":
          return null;
        case "git_commit":
          return "[m13/git abc1234] 桩提交";

        // 造一段带合并的历史，泳道图的分叉与汇合都能看到
        case "git_log_entries":
          return MOCK_LOG;
        case "git_commit_files":
          return [
            g("src/OrderService.java", "M", "."),
            g("src/App.svelte", "A", "."),
            g("docs/gone.md", "D", "."),
          ];
        case "git_commit_diff":
          return {
            truncated: false,
            text: `diff --git a/${a.path || "src/OrderService.java"} b/${a.path || "src/OrderService.java"}
@@ -8,4 +8,5 @@
 public class OrderService {
-    private int retries = 3;
+    private int retries = 5;
+    private Duration backoff = Duration.ofMillis(800);
 }`,
          };
        case "git_branches":
          return [
            b("main", true, false, "origin/main", "M12 界面打磨"),
            b("m13/git", false, false, "origin/m13/git", "M13 Git 版本管理"),
            b("m11/symbols", false, false, "", "M11 符号大纲"),
            b("origin/main", false, true, "", "M12 界面打磨"),
            b("origin/dev", false, true, "", "开发主线"),
          ];
        case "git_switch":
          return `Switched to branch '${a.name}'`;
        case "git_worktrees":
          return [
            { path: "/proj", sha: "abc1234", branch: "m13/git", detached: false, bare: false, locked: false, current: true },
            { path: "/proj-hotfix", sha: "def5678", branch: "hotfix/urgent", detached: false, bare: false, locked: false, current: false },
          ];
        case "git_worktree_add":
          return `/proj-${a.branch || "new"}`;
        case "git_worktree_remove":
          return null;

        default:
          return null;
      }
    },
  };

  /*
   * 事件插件的内部对象。
   *
   * `@tauri-apps/api/event` 的 unlisten 走的是**这个**对象上的
   * `unregisterListener`，不是 __TAURI_INTERNALS__ 上的。少了它，
   * App 里那个 onDragDropEvent 的清理函数一跑就抛
   * 「Cannot read properties of undefined」—— 而且是 uncaught，
   * 每次热更新刷一条，正是它要淹掉的那类真错误。
   *
   * 桩与真实现分叉就失去了全部价值。这里补齐它。
   */
  (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => {},
  };
  // eslint-disable-next-line no-console
  console.info("[dev] Tauri IPC 桩已装载 —— 数据是假的，用于纯前端调试");
}
