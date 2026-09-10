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
 *
 * ⚠ **CI 拿这张表里的字符串当哨兵**（`.github/workflows/ci.yml` 的
 * 「开发桩不许进产物」那一步）。2026-09-07 之前这个模块真的漏进过生产入口包
 * 1,195 字节 —— 漏的就是下面这句 `.map()`：方法调用打包器证明不了没有副作用，
 * 于是连数据字面量一起留下了。改这几行之前先看那一步。
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
  extra: { staged?: boolean; untracked?: boolean; conflicted?: boolean; orig?: string } = {},
) {
  const untracked = extra.untracked ?? false;
  const conflicted = extra.conflicted ?? false;
  return {
    path,
    index,
    work,
    untracked,
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
  /*
   * 应用自己的运行日志。四种来源各给一条 —— 「打开应用日志」要验的正是
   * 「这几类事情落下来长什么样、级别过滤认不认得出」。
   */
  "/Users/you/Library/Logs/com.liteide.app/app.log": [
    "2026-09-10 09:12:04.118Z INFO [app] 启动 v0.9.0",
    "2026-09-10 09:12:41.802Z WARN [csp] 挡下 connect-src ← https://example.com/x.json @ :0",
    "2026-09-10 09:14:07.331Z ERROR [window.error] Cannot read properties of null (reading 'view') @ /assets/index.js:2841 ⏎ at Editor.svelte:214 ⏎ at flush",
    "2026-09-10 09:14:07.335Z ERROR [unhandledrejection] Error: 读不到 /proj/src/main.py ⏎ at readText",
    "2026-09-10 09:20:55.010Z ERROR [rust] panic: called `Option::unwrap()` on a `None` value",
    "2026-09-10 09:21:03.774Z INFO [app] 启动 v0.9.0",
  ].join("\n") + "\n",
  // 点文件也要能打开 —— 只在 DIRS 里列出来、点开却是空的，那是另一种骗人
  "/proj/.gitignore": `node_modules/
dist/
target/
.DS_Store
`,
  "/proj/.env": `# 桩数据，不是真密钥
API_BASE=http://localhost:8080
LOG_LEVEL=debug
`,
  "/proj/.github/workflows/ci.yml": `name: CI
on: [push, pull_request]
jobs:
  build:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - run: pnpm install
      - run: pnpm check && pnpm test
`,
  /*
   * XML 单独占一条：`tagName` 继承 `typeName`（IDEA 里类名就是正文色），
   * 元素名会跟正文一个颜色，看起来像「没上色」。桩里没有 XML 文件的时候
   * 这个问题在浏览器里根本看不出来 —— 而这正是 issue #5。
   */
  "/proj/pom.xml": `<?xml version="1.0" encoding="UTF-8"?>
<!-- Maven 项目描述，用于在浏览器里验证 XML 着色 -->
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.liteide</groupId>
  <artifactId>order-service</artifactId>
  <version>1.0.0</version>
  <dependencies>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter-web</artifactId>
      <scope>compile</scope>
    </dependency>
  </dependencies>
</project>
`,
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
  /*
   * **一对真 Maven 目录的 Java 文件**，专门给 ⌘Click / ⌘B 跳转用。
   *
   * 上面那个 `src/OrderService.java` 的 `package com.liteide.order` 和它所在的
   * 目录对不上 —— 而跳转的第二层（import / 同包）全部依据就是
   * 「包路径 = 目录路径」，桩里没有这个形状，那两层在浏览器里一次都走不到。
   *
   * 两个模块是故意的：跨模块跳转（api 引用 core）正是这个功能的立身之本，
   * 而单模块的桩看不出「后缀匹配」和「全等匹配」的区别。
   */
  "/proj/moduleA/src/main/java/com/demo/api/AdminController.java": `package com.demo.api;

import com.demo.core.OrderClient;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class AdminController {
    private final OrderClient orderClient;
    private final SamePkgHelper helper;

    public AdminController(OrderClient orderClient, SamePkgHelper helper) {
        this.orderClient = orderClient;
        this.helper = helper;
    }
}
`,
  "/proj/moduleA/src/main/java/com/demo/api/SamePkgHelper.java": `package com.demo.api;

public class SamePkgHelper {
    public String tag() { return "同包，不写 import"; }
}
`,
  "/proj/moduleB/src/main/java/com/demo/core/OrderClient.java": `package com.demo.core;

public class OrderClient {
    public String ping() { return "跨模块跳过来的"; }
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

/*
 * 桩里要有点文件和点目录。真实现 2026-09-06 起把它们列出来了
 * （`fsservice::list_dir` 不再有 `show_hidden`），桩里一个都没有的话，
 * 浏览器里的文件树和 `.app` 里长得不一样 —— 而改 UI 的主循环就在浏览器里。
 *
 * 生成物目录（`node_modules` / `target` / `dist` / `build` / `venv` /
 * `__pycache__` / `vendor`，名单在 `crates/excludes`）**故意不放**：
 * 真实现永远不列它们，桩里放了反而是假的。
 */
/**
 * 被「丢弃改动」丢掉的路径。桩里唯一一处**有状态**的地方。
 *
 * 加它是因为「切分支被本地改动挡住 → 丢弃 → 再切一次」这条路要在浏览器里
 * 走得通：全无状态的桩会让第二次切换撞上同一个拒绝，看着像修复没生效。
 * 而这条路恰恰是这一轮的主角。
 */
const discarded = new Set<string>();

/** 挡住切到 `m11/symbols` 的那两个文件。和 `git_status` 里的条目对得上 */
const BLOCKERS = ["src/App.svelte", "docs/old.md"];

/**
 * 当前分支。`git_switch` 成功之后要变 —— 否则切完了 `git_status` 还报老名字，
 * 而应用的成功提示说的正是**刷新后的实际分支**（它有意不复述「我请求切到哪儿」）。
 * 桩不跟着变，那句提示在浏览器里就永远是错的。
 */
let curBranch = "m13/git";

/** 本地分支的上游。切分支之后提示语里的「（跟踪 …）」要跟着走 */
const UPSTREAM: Record<string, string> = {
  main: "origin/main",
  "m13/git": "origin/m13/git",
  "m11/symbols": "",
};

/**
 * 草稿目录。真实现是 `~/Library/Application Support/com.liteide.app/scratches`，
 * 桩里给一条形状一样的绝对路径就够了 —— 前端只把它当不透明的路径传来传去。
 */
const SCRATCH_DIR = "/Users/you/Library/Application Support/com.liteide.app/scratches";
/*
 * 应用自己的运行日志。桩里给它几行真的内容 —— 不给的话「打开应用日志」
 * 在浏览器里只能验到「有没有报错」，验不到「打开之后长什么样」，
 * 而后者才是这个功能的全部（它就是用日志模式打开一个文件）。
 */
const APP_LOG = "/Users/you/Library/Logs/com.liteide.app/app.log";

/**
 * 生成物目录名。**必须和 `crates/excludes::GENERATED_DIRS` 一份**。
 *
 * 桩里手抄一份是没办法的事（那是 Rust 常量），所以下面的 DIRS 里
 * 特意摆了一个真的 `node_modules` 和一个真的 `build` —— 不摆的话，
 * 「生成物压暗」这条在浏览器里一次都看不到。
 */
const GENERATED = new Set(["node_modules", "target", "dist", "build", "venv", "__pycache__", "vendor"]);

const DIRS: Record<string, Array<[string, boolean]>> = {
  // 应用日志所在的目录。它**不在项目里**，只有「帮助 → 打开应用日志」够得着
  "/Users/you/Library/Logs/com.liteide.app": [["app.log", false]],
  "/proj": [["src", true], ["moduleA", true], ["moduleB", true], ["logs", true], ["docs", true], [".github", true], ["node_modules", true], ["target", true], [".env", false], [".gitignore", false], ["README.md", false], ["package.json", false], ["pom.xml", false], ["Cargo.toml", false], ["vite.config.ts", false]],
  // 生成物目录里也要有东西 —— 空目录点开只有一行「空」，看不出「点得开」这件事
  "/proj/node_modules": [["svelte", true], [".package-lock.json", false]],
  "/proj/node_modules/svelte": [["package.json", false]],
  "/proj/target": [["debug", true]],
  "/proj/target/debug": [["build.log", false]],
  "/proj/.github": [["workflows", true]],
  "/proj/.github/workflows": [["ci.yml", false]],
  "/proj/src": [["OrderService.java", false], ["main.py", false], ["gbk-legacy.java", false], ["big5-notes.txt", false], ["long.ts", false]],
  // 跳转用的那对 Java（见 FILES 里的说明）。目录一层层列出来，
  // 否则文件树点不进去，而 ⌘P 又能搜到 —— 两边对不上就是桩在骗人
  "/proj/moduleA": [["src", true]],
  "/proj/moduleA/src": [["main", true]],
  "/proj/moduleA/src/main": [["java", true]],
  "/proj/moduleA/src/main/java": [["com", true]],
  "/proj/moduleA/src/main/java/com": [["demo", true]],
  "/proj/moduleA/src/main/java/com/demo": [["api", true]],
  "/proj/moduleA/src/main/java/com/demo/api": [["AdminController.java", false], ["SamePkgHelper.java", false]],
  "/proj/moduleB": [["src", true]],
  "/proj/moduleB/src": [["main", true]],
  "/proj/moduleB/src/main": [["java", true]],
  "/proj/moduleB/src/main/java": [["com", true]],
  "/proj/moduleB/src/main/java/com": [["demo", true]],
  "/proj/moduleB/src/main/java/com/demo": [["core", true]],
  "/proj/moduleB/src/main/java/com/demo/core": [["OrderClient.java", false]],
  "/proj/logs": [["access-2026-08-24.log", false]],
  "/proj/docs": [["ARCHITECTURE.md", false]],
};

/**
 * 桩里的名字校验。**规则必须和 `fsservice::validate_name` 一条不差** ——
 * 分叉之后前端的错误提示就是在浏览器里对着一份假规则调的。
 */
function checkName(name: string): string | null {
  if (name.trim() === "") return "名字不能为空";
  if (name.includes("/")) return "名字里不能有 /";
  if (name.includes("\0")) return "名字里不能有空字符";
  if (name === "." || name === "..") return "不能叫 . 或 ..";
  const bytes = new TextEncoder().encode(name).length;
  if (bytes > 255) return `名字太长（上限 255 字节，这个 ${bytes} 字节）`;
  return null;
}

const parentOf = (p: string) => p.slice(0, p.lastIndexOf("/"));
const nameOf = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/**
 * 桩里这条路径存不存在。
 *
 * 三处都要查：`DIRS` 的键（目录自己）、`FILES`（有内容的文件），
 * **以及父目录的清单** —— 日志文件只在清单里出现，`FILES` 里没有它
 * （`probe_path` 是按 `.log` 后缀判的）。少了第三条，右键改名一个 `.log`
 * 会报「不在盘上了」，而它明明就在树上摆着。
 */
const existsInMock = (p: string) =>
  DIRS[p] !== undefined ||
  FILES[p] !== undefined ||
  (DIRS[parentOf(p)] ?? []).some(([n]) => n === nameOf(p));

/** 把 old 前缀下的所有条目搬到 next 前缀 —— 改一个目录的名字要连子树一起搬 */
function movePrefix(old: string, next: string) {
  for (const [k, v] of Object.entries(FILES)) {
    if (k === old || k.startsWith(`${old}/`)) {
      FILES[next + k.slice(old.length)] = v;
      delete FILES[k];
    }
  }
  for (const [k, v] of Object.entries(DIRS)) {
    if (k === old || k.startsWith(`${old}/`)) {
      DIRS[next + k.slice(old.length)] = v;
      delete DIRS[k];
    }
  }
}

/** 把 p 及其子树整个抹掉 */
function dropSubtree(p: string) {
  for (const k of Object.keys(FILES)) {
    if (k === p || k.startsWith(`${p}/`)) delete FILES[k];
  }
  for (const k of Object.keys(DIRS)) {
    if (k === p || k.startsWith(`${p}/`)) delete DIRS[k];
  }
  const par = DIRS[parentOf(p)];
  if (par) DIRS[parentOf(p)] = par.filter(([n]) => n !== nameOf(p));
}

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

/**
 * 一个打开着的日志背后是什么。
 *
 * **原来这里只有一份全局数据**：不管打开哪个文件，`log_lines` 都返回同一份
 * 循环出来的 900 万行假日志。演示用没问题 —— 直到「打开应用日志」出现：
 * 桩明明手里有 `app.log` 的内容（`FILES` 里躺着），却仍然把演示日志端出来。
 * **那就是桩在骗人**，而且骗的正好是这个功能唯一要验的东西。
 *
 * 现在按句柄分：`FILES` 里有的走真内容，别的（那个 1GB 的演示日志）
 * 仍旧走循环生成。真实现本来就是按句柄拿文件的，这一步是往它靠。
 */
interface LogSrc {
  /** 第 n 行。大文件走循环，小文件走真数组 */
  at: (n: number) => string;
  total: number;
  /** 第 n 行的级别下标，顺序同 Rust 侧 Level */
  levelAt: (n: number) => number;
  /** `log_stat` 报的六个级别计数 */
  levels: number[];
  bytes: number;
}

const lineAt = (n: number) => LINES[n % LINES.length];

/** 那个 1GB 的演示日志。没有对应真文件时一律用它 */
const BIG_LOG: LogSrc = {
  at: lineAt,
  total: TOTAL,
  levelAt: (n) => LINE_LEVEL[n % LINES.length],
  levels: [456_822, 914_684, 5_026_804, 2_742_487, 0, 910],
  bytes: 1_073_741_885,
};

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

/** 句柄 → 打开时那条路径。真实现的 LogFile 也是这么存的 */
const LOG_PATHS: Record<number, string> = {};
/** 句柄 → 它的数据源 */
const LOG_SRC: Record<number, LogSrc> = {};
let nextLogHandle = 1;
/** 上一次被问到的那个句柄的源。过滤那几条命令都带 handle，直接查表 */
const src = (h: unknown) => LOG_SRC[Number(h)] ?? BIG_LOG;

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

function runFilter(s: LogSrc, levelBits: number, pattern: string, caseSensitive: boolean): number[] {
  const hits: number[] = [];
  const pat = caseSensitive ? pattern : pattern.toLowerCase();
  // 桩只在前 5 万行上筛，够验证交互，不必真跑 900 万
  const n1 = Math.min(s.total, 50_000);
  for (let n = 0; n < n1; n++) {
    if ((levelBits & (1 << s.levelAt(n))) === 0) continue;
    if (pat) {
      const text = caseSensitive ? s.at(n) : s.at(n).toLowerCase();
      if (!text.includes(pat)) continue;
    }
    hits.push(n);
  }
  return hits;
}

/** 桩里模拟耗时用。真实现的每一段进度之间本来就有间隔 */
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** 远程操作的假 id，和被取消的那些 */
let mockRemoteId = 0;
const mockCancelled = new Set<number>();

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
        // 浏览器里没有 Rust 侧的 LITE_IDE_DEBUG，那条内存统计链路整个不存在。
        // 落到 default 的 null 也能让前端不建定时器，但那是碰巧对 ——
        // 显式写出来，读桩的人才看得出这条命令被想过。
        case "diag_enabled":
          return false;
        case "app_log":
          // 桩里不落盘 —— 但要留个响，否则浏览器里「异常有没有被记下来」
          // 完全看不出来
          console.info(`[app_log/${a.level}] ${a.source}: ${a.msg}`);
          return null;
        case "app_log_path":
          return APP_LOG;
        case "clear_app_log":
          // 桩里也要真清 —— 只返回 null 的话，「清完界面刷不刷新」这条
          // 在浏览器里永远看着像对的
          FILES[APP_LOG] = "2026-09-10 09:30:00.000Z INFO [app] 日志已清空\n";
          bump(APP_LOG);
          return null;
        case "probe_path": {
          const path = String(a.path);
          if (DIRS[path]) {
            return { kind: "dir", mode: "edit", path, name: path.split("/").pop(), size: 0, reason: "" };
          }
          /*
           * **`.log` 不等于日志模式。** 真实现（`logengine::probe`）只看
           * 体积 / 行数 / 最长行 / 二进制，**从不看文件名** —— 一个 3KB 的
           * `app.log` 在真机上走的是编辑模式。
           *
           * 桩原来对任何 `.log` 都硬报「1GB、超过 32MB」，于是
           * 「小 .log 在编辑模式下能不能切回日志模式」这条路在浏览器里
           * 走不到，而那正是 c16f12d 修的那个 bug 的现场。
           * 现在的判据是「桩手里有没有这个文件的真内容」：
           * 有就按真大小走真判据，没有的才是那个演示用的 1GB 日志。
           */
          const known = FILES[path] !== undefined;
          const isLog = path.endsWith(".log") && !known;
          /*
           * 不认识的路径要**报错**，不能凭空造一个文件出来。
           *
           * 原来对任意路径都返回一个假文件，于是「打开一个不存在的文件」
           * 这条路在浏览器里根本走不到 —— 而真实现是直接 Err。会话恢复
           * 正好高度依赖这条路（上次开着的文件这次可能已经删了/换分支没了），
           * 桩不还原它，那部分逻辑就等于没测过。
           */
          if (!isLog && !known) {
            throw new Error(`读不到 ${path}：No such file or directory (os error 2)`);
          }
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
          // 排序规则抄 Rust 侧 list_dir：目录在前，同类按名称不区分大小写。
          // 桩里原来是按写死的顺序返回的 —— 新建一个文件之后它会吊在列表最后，
          // 而真实现会把它排到该在的位置，「新建完滚过去」那段交互就白验了
          const sorted = [...(DIRS[path] ?? [])].sort(
            (x, y) =>
              Number(y[1]) - Number(x[1]) ||
              x[0].toLowerCase().localeCompare(y[0].toLowerCase()),
          );
          return sorted.map(([name, isDir]) => ({
            name,
            path: `${path}/${name}`,
            isDir,
            size: isDir ? 0 : (FILES[`${path}/${name}`]?.length ?? 0),
            // 判据抄 Rust 侧：只有**目录**才谈得上生成物目录。
            // 一个叫 build 的文件（shell 脚本）不算
            generated: isDir && GENERATED.has(name),
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
        /*
         * 浏览器里没有 Finder。桩不能一律返回成功 ——「路径不在盘上就报错」
         * 是这条命令唯一有分支的行为，桩里抹平它，前端的错误处理就等于没测过。
         */
        case "reveal_in_finder": {
          const path = String(a.path);
          if (!DIRS[path] && FILES[path] === undefined) {
            throw new Error(`${path} 不在盘上了`);
          }
          console.info(`[mock] 在 Finder 中显示 ${path}`);
          return null;
        }
        /*
         * 新建 / 改名 / 废纸篓：桩里**保留每一条错误分支**。
         * 抹平它们，前端那几条错误提示就等于从没走到过 —— 而它们恰恰是
         * 这批功能里最该验的部分（撞名、非法名字、目标已存在）。
         */
        case "create_entry": {
          const dir = String(a.dir);
          const name = String(a.name);
          const isDir = Boolean(a.isDir);
          const bad = checkName(name);
          if (bad) throw new Error(bad);
          const path = `${dir}/${name}`;
          if (existsInMock(path)) throw new Error(`这里已经有一个叫「${name}」的了`);
          DIRS[dir] = [...(DIRS[dir] ?? []), [name, isDir]];
          if (isDir) DIRS[path] = [];
          else {
            FILES[path] = "";
            bump(path);
          }
          return path;
        }
        /*
         * 草稿目录在浏览器里也得是个**真的目录**（`DIRS` 里有它），
         * 否则「打开草稿目录」把它当项目根打开时，文件树列出来是空的，
         * 而真实现里那儿至少有你刚建的那份草稿 —— 桩一分叉就开始骗人。
         */
        case "scratch_dir":
          return SCRATCH_DIR;
        case "create_scratch": {
          const stem = String(a.stem);
          if (!DIRS[SCRATCH_DIR]) DIRS[SCRATCH_DIR] = [];
          // 撞名加序号，跟 Rust 侧 fsservice::create_scratch 一样
          for (let n = 1; n <= 99; n++) {
            const name = n === 1 ? `${stem}.md` : `${stem}-${n}.md`;
            const path = `${SCRATCH_DIR}/${name}`;
            if (existsInMock(path)) continue;
            DIRS[SCRATCH_DIR] = [...DIRS[SCRATCH_DIR], [name, false]];
            FILES[path] = "";
            bump(path);
            return path;
          }
          throw new Error("同一分钟里已经有 99 份草稿了");
        }
        case "discard_empty_scratch": {
          const path = String(a.path);
          // 判据照着 Rust 侧抄一遍。桩里少一条，浏览器上就走得通而真机上走不通
          if (!path.startsWith(`${SCRATCH_DIR}/`)) throw new Error("不在草稿目录里");
          if (!existsInMock(path)) throw new Error(`${path} 不在盘上了`);
          if ((FILES[path] ?? "") !== "") throw new Error("这份草稿里有东西，不能这么丢");
          delete FILES[path];
          DIRS[SCRATCH_DIR] = (DIRS[SCRATCH_DIR] ?? []).filter(
            ([n]) => `${SCRATCH_DIR}/${n}` !== path,
          );
          return null;
        }
        case "rename_entry": {
          const path = String(a.path);
          const name = String(a.name);
          // 校验顺序跟着 Rust 侧走：先看名字，再看源在不在
          const bad = checkName(name);
          if (bad) throw new Error(bad);
          if (!existsInMock(path)) throw new Error(`${path} 不在盘上了`);
          const parent = parentOf(path);
          const to = `${parent}/${name}`;
          if (to === path) return to;
          if (existsInMock(to)) throw new Error(`这里已经有一个叫「${name}」的了`);
          const wasDir = DIRS[path] !== undefined;
          movePrefix(path, to);
          const old = nameOf(path);
          DIRS[parent] = (DIRS[parent] ?? []).map(
            ([n, d]) => (n === old ? [name, d] : [n, d]) as [string, boolean],
          );
          if (!wasDir) bump(to);
          return to;
        }
        case "trash_entry": {
          const path = String(a.path);
          if (!existsInMock(path)) throw new Error(`${path} 不在盘上了`);
          dropSubtree(path);
          console.info(`[mock] 移到废纸篓 ${path}`);
          return null;
        }
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
          return {
            lineCount: s.total,
            indexedBytes: s.bytes,
            totalBytes: s.bytes,
            complete: true,
            indexBytes: 71_472,
            levels: s.levels,
            levelsComplete: true,
            levelsScanned: s.bytes,
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
          if (bits === 0b111111 && !pat) {
            filterHits = null;
            return false;
          }
          filterHits = runFilter(src(a.handle), bits, pat, Boolean(a.caseSensitive));
          return true;
        }
        case "log_filter_stat":
          return filterHits === null
            ? null
            : { hits: filterHits.length, complete: true, scannedLines: 50_000 };
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
          return { kind: "none", newLines: 0, lineCount: TOTAL };
        }
        case "close_log":
          delete LOG_PATHS[Number(a.handle)];
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
            branch: curBranch,
            upstream: UPSTREAM[curBranch] ?? "",
            ahead: 2,
            behind: 0,
            detached: false,
            unborn: false,
            truncated: false,
            /*
             * 整个未跟踪的目录**不在 entries 里** —— 真实现把它摊成里面的
             * 文件，目录名单独走这一路给文件树。桩要照着摊，否则在浏览器里
             * 改动列表长得和 .app 里不一样。
             */
            untrackedDirs: ["scratch/"],
            entries: [
              g("src/OrderService.java", "M", ".", { staged: true }),
              g("src/App.svelte", ".", "M"),
              g("README.md", "A", "."),
              g("docs/old.md", ".", "D"),
              g("src/renamed.ts", "R", ".", { orig: "src/before.ts" }),
              g("scratch/draft.md", ".", "?", { untracked: true }),
              g("scratch/tmp/notes.md", ".", "?", { untracked: true }),
              g("notes.txt", ".", "?", { untracked: true }),
              g("src/conflict.rs", "U", "U", { conflicted: true }),
            ].filter((e) => !discarded.has(e.path)),
          };
        case "git_diff":
          /*
           * 两个 hunk 是有意的：
           *
           * - 第一个是「连续新增一段长 import」。双栏差异的两个已知问题
           *   （右列横向溢出、左边一大块连续斜纹）只在这种形态下才看得出来，
           *   而原来的桩只有一处两行的小改动，在浏览器里怎么看都是好的。
           * - 第二个是普通的行内小改动，保住原来那份覆盖。
           */
          return {
            truncated: false,
            text: `diff --git a/${a.path} b/${a.path}
index 1a2b3c4..5d6e7f8 100644
--- a/${a.path}
+++ b/${a.path}
@@ -14,6 +14,11 @@ import java.util.List;
 import java.util.List;
 import java.util.Optional;
+import com.etianqu.evaluation.client.EvaluationTemplateFeignClient;
+import com.etianqu.evaluation.client.dto.EvaluationTemplateQueryRequest;
+import com.etianqu.evaluation.client.dto.EvaluationTemplateDetailResponse;
+import com.etianqu.evaluation.common.exception.EvaluationServiceException;
+import com.etianqu.evaluation.common.constant.EvaluationTemplateConstants;
 import org.springframework.stereotype.Service;
 import org.springframework.beans.factory.annotation.Autowired;
@@ -42,7 +47,8 @@ public void persist(Order order) {
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
          return null;
        case "git_discard":
          // 记下来 —— `git_status` 和 `git_switch` 都要看它，
          // 否则「丢弃挡路的改动之后再切一次」在浏览器里永远走不通
          for (const x of [...(a.paths as string[]), ...(a.untracked as string[])]) discarded.add(x);
          return null;
        case "git_commit":
          /*
           * **故意慢。** 真实现里 `git commit` 跑在阻塞池上，因为
           * pre-commit 钩子跑什么是仓库说了算 —— 跑一遍 eslint 三十秒
           * （rules/rust.md 那张表）。桩原来 0ms 返回，于是围着这件事
           * 建的两样东西在浏览器里**一次都验不到**：
           *
           * - 「慢操作才说话」那条 300ms 的线（`gitDo` 里的 tip 定时器）
           * - 「一次只允许一个写操作」那道守卫（issue #23）
           *
           * 1.2 秒够跨过 300ms 那条线、也够在它跑着的时候手动点一次拉取，
           * 又不至于让浏览器里调 UI 变难受。
           */
          await sleep(1200);
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
            /*
             * `isHead` 必须和 `git_status` 的 `branch` 是同一个分支。
             * 真实现里两者都来自 checkout 的那一个（`%(HEAD)` 只标它），
             * 而这里原来 status 说 m13/git、branches 却把 main 标成 HEAD ——
             * 于是分支面板的「当前」和 Git 栏的分支名各说各的。
             */
            b("main", curBranch === "main", false, "origin/main", "M12 界面打磨"),
            b("m13/git", curBranch === "m13/git", false, "origin/m13/git", "M13 Git 版本管理"),
            b("m11/symbols", curBranch === "m11/symbols", false, "", "M11 符号大纲"),
            b("origin/main", false, true, "", "M12 界面打磨"),
            b("origin/dev", false, true, "", "开发主线"),
          ];
        case "git_switch": {
          /*
           * **切到 `m11/symbols` 一定失败，而且失败成「本地改动挡着」那一档。**
           *
           * 这条路在桩上必须走得到：真实现里它要工作区脏 + 两边改了同一个文件
           * 才触发，而浏览器里没有真仓库。挡不住的话，那条「去提交 / 丢弃这些
           * 改动 / 取消」的确认条一次都验不了 —— 而它恰恰是这一轮的主角。
           *
           * reject 的是**对象不是字符串**，和 Rust 侧的 `SwitchErrDto` 一致；
           * 桩要是抛个 Error，前端 `err.kind` 读出来是 undefined，
           * 就会走到「原样上抛」那条分支去，在浏览器里看着像功能没做。
           */
          const blocking = BLOCKERS.filter((f) => !discarded.has(f));
          if (a.name === "m11/symbols" && !a.create && blocking.length > 0) {
            throw {
              kind: "local-changes",
              message: `有 ${blocking.length} 个文件的本地改动挡着`,
              files: blocking,
              raw:
                "error: Your local changes to the following files would be overwritten by checkout:\n" +
                blocking.map((f) => `\t${f}`).join("\n") +
                "\nPlease commit your changes or stash them before you switch branches.\nAborting",
            };
          }
          const asked = String(a.name);
          curBranch = asked.replace(/^origin\//, "");
          // 检出远程分支时真实现走 `switch --track`，**会给新分支设上上游**。
          // 桩不设的话，界面上那句「（跟踪 …）」在浏览器里永远不出现
          if (asked.startsWith("origin/")) UPSTREAM[curBranch] = asked;
          return `Switched to branch '${curBranch}'`;
        }
        case "git_worktrees":
          return [
            { path: "/proj", sha: "abc1234", branch: "m13/git", detached: false, bare: false, locked: false, current: true },
            { path: "/proj-hotfix", sha: "def5678", branch: "hotfix/urgent", detached: false, bare: false, locked: false, current: false },
          ];
        case "git_worktree_add":
          return `/proj-${a.branch || "new"}`;
        case "git_worktree_remove":
          return null;

        // ── 菜单栏 ──

        /*
         * 浏览器里没有原生面板，也没有菜单栏 —— 这三条**必须有桩**，
         * 不能落到 default 去。
         *
         * 落到 default 返回 null 的话，`pickFolder()` 拿到 null 看着
         * 就像"用户取消了"，于是「打开文件夹」这个按钮在浏览器里
         * 点了永远没反应，而**没有任何报错** —— 排查时会先怀疑按钮没绑上。
         * 返回一个假目录，至少那条路是通的。
         */
        case "pick_folder":
          return "/proj";
        // 菜单在浏览器里不存在，这两条是空操作 —— 但要显式写出来
        case "set_recent":
        case "sync_menu_state":
          return null;
        // ── 拉取与推送 ──

        /*
         * 进度必须**真的分段推**，不能一次给个 100%。
         *
         * 桩比真实现快，而「进度条、节流、取消」全是时序类的东西 ——
         * 一次推完的话，在浏览器里怎么点都是好的，而真机上可能一路是
         * 0% 然后突然结束。这条教训在会话恢复那个闪烁 bug 上吃过一次
         * （AGENTS.md「桩比真实现快，所以验红这一步在它上面会失效」）。
         */
        case "git_fetch":
        case "git_push": {
          const ch = a.onProgress as { onmessage?: (p: unknown) => void } | undefined;
          // id 由调用方给（真实现同理），桩要照做，否则取消对不上号
          const id = Number(a.opId ?? ++mockRemoteId);
          mockCancelled.delete(id);
          const phases: [string, number][] = [
            ["Enumerating objects", 1248],
            ["Counting objects", 1248],
            ["Compressing objects", 498],
            [cmd === "git_push" ? "Writing objects" : "Receiving objects", 1248],
            ["Resolving deltas", 722],
          ];
          for (const [phase, total] of phases) {
            for (const pct of [0, 37, 74, 100]) {
              if (mockCancelled.has(id)) {
                mockCancelled.delete(id);
                throw { kind: "cancelled", message: "已取消", raw: "" };
              }
              await sleep(90);
              ch?.onmessage?.({
                phase,
                percent: pct,
                done: Math.round((total * pct) / 100),
                total,
                finished: pct === 100,
              });
            }
          }
          // 桩里让「没有上游的推送」走一次被拒，好在浏览器里能看到那条路
          if (cmd === "git_push" && a.branch === "rejected-demo") {
            throw {
              kind: "rejected",
              message: "远程比你多了东西，推不上去",
              raw: "! [rejected]  dev -> dev (non-fast-forward)",
            };
          }
          return null;
        }
        case "git_cancel":
          mockCancelled.add(Number(a.id));
          return true;
        case "git_merge_upstream":
          // ff-only 在桩里总是快进不了 —— 那条分岔决策的路才走得到
          if (a.mode === "ff-only") {
            throw {
              kind: "conflict",
              message: "有冲突要先解决",
              raw: "fatal: Not possible to fast-forward, aborting.",
            };
          }
          return null;
        case "git_outgoing":
          return ["feat(notary): 补公证订单字段", "fix(notary): 退款审核状态对不上"];

        case "open_external": {
          // 真实现只放行 https，桩也照做：不然浏览器里试不出那条约束
          const url = String(a.url ?? "");
          if (!url.startsWith("https://")) throw new Error(`只允许 https 链接，实得：${url}`);
          return null;
        }

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

  /*
   * 从控制台触发一条菜单动作：`__mockMenu("help-log")`。
   *
   * **浏览器里没有菜单栏**，而归菜单的动作有二十多条。它们在这里的另一条路
   * 是「连按两下 ⇧」的随处搜索 —— 那条路在自动化里按不出来
   * （裸修饰键的 keydown 传不下去），于是「打开应用日志」这类功能
   * 在浏览器里**一次都验不到**，只能等打成 `.app` 再说，而那是 45 秒一轮。
   *
   * 实现上不去猜哪个回调是菜单的：`@tauri-apps/api` 的事件负载自带
   * `event` 字段，每个监听器自己会对名字。所以广播给全部回调，
   * 认不认是它们自己的事。
   */
  (window as unknown as Record<string, unknown>).__mockMenu = (id: string) => {
    let n = 0;
    for (const k of Object.keys(window)) {
      if (!k.startsWith("_cb")) continue;
      const cb = (window as unknown as Record<string, unknown>)[k];
      if (typeof cb !== "function") continue;
      try {
        (cb as (e: unknown) => void)({ event: "menu", id: 0, payload: id });
        n++;
      } catch {
        /* 不是菜单的那些回调收到这个形状会抛，正常 */
      }
    }
    return `广播给 ${n} 个回调`;
  };
  // eslint-disable-next-line no-console
  console.info("[dev] Tauri IPC 桩已装载 —— 数据是假的，用于纯前端调试");
}
