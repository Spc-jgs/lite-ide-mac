/**
 * 桩的共享部分：假数据（文件内容、目录表、日志行）、路径工具、时间戳。
 * 从 `mock-ipc.ts` 拆出来（2026-09-24，纯搬家）。只被一个领域用到的都跟着那个领域走了；
 * 这里是两个以上领域都要用的。**会被重新赋值的 `let` 不许放这里**（ES 模块的导入是只读绑定，
 * 别的文件改不了它）—— `clock` 例外：只有这个文件里的 `bump` 改它。
 */

export const LINES = [
  "2026-08-24 14:03:21.442 INFO  [http-nio-exec-4] c.l.OrderService - 处理完成 orderId=8842011 cost=142ms status=SUCCESS",
  "2026-08-24 14:03:22.015 DEBUG [pool-3-thread-2] c.l.CacheManager - evict key=order:8842011 ttl=300s",
  "2026-08-24 14:03:25.512 WARN  [http-nio-exec-2] c.l.RetryPolicy - 重试 attempt=2/5 backing off 800ms cause=Read timeout",
  "2026-08-24 14:03:25.780 ERROR [scheduler-1] c.l.OrderService - 订单落库失败 orderId=8842013 cause=DeadlockLoserDataAccessException",
  "java.lang.IllegalStateException: connection pool exhausted",
  "\tat com.zaxxer.hikari.pool.HikariPool.createTimeoutException(HikariPool.java:696)",
  "\tat com.zaxxer.hikari.pool.HikariPool.getConnection(HikariPool.java:197)",
  // 堆栈跳源码（2026-09-23）：这一帧的包路径对得上 moduleB 里那份 OrderClient.java，画成链接；
  // 上面两帧 Hikari 在 jar 里，不画 —— 桩上两种形态都要看得到
  "\tat com.demo.core.OrderClient.ping(OrderClient.java:4)",
  "Caused by: java.sql.SQLTransientConnectionException: HikariPool-1 timed out",
  "2026-08-24 14:03:26.101 INFO  [kafka-listener-0] c.l.KafkaConsumer - 处理完成 orderId=8842014 cost=37ms status=SUCCESS",
  "2026-08-24 14:03:26.550 TRACE [main] c.l.InventoryLock - enter acquire(sku=A-1180)",
  "2026-08-24 14:03:27.003 INFO  [http-nio-exec-1] o.s.web.DispatcherServlet - Completed 200 OK in 12ms",
];

/** 级别在 LINES 里的下标，供桩内过滤用；顺序同 Rust 侧 Level */
export const LINE_LEVEL = [2, 3, 1, 0, 5, 5, 5, 5, 5, 2, 4, 2];

export const TOTAL = 9_141_707;

export const FILES: Record<string, string> = {
  // 两份草稿，配合上面 DIRS 里的目录（issue #40）。一份有标题，一份是几行零碎的
  "/Users/you/Library/Application Support/com.liteide.app/scratches/2026-09-10 1644.md":
    "# 周会要点\n\n- 日志引擎 1GB 冷启动 460ms\n- 下周切 CI\n",
  "/Users/you/Library/Application Support/com.liteide.app/scratches/2026-09-12 0915.md":
    "\n\ncurl -s http://localhost:8080/health | jq .\n",
  // 只有头、没有正文的一份：启动直接进草稿时要**复用它**而不是再建一份（`tabflow.launchScratch`）
  "/Users/you/Library/Application Support/com.liteide.app/scratches/2026-09-16 2310.md":
    "---\nproject: /proj\nbranch: m13/git\nhead: h800000\n---\n\n",
  // 带锚点的一份（M10）：列表要按项目分组、chip 要能跳回那一行，桩上得有一条能点的
  "/Users/you/Library/Application Support/com.liteide.app/scratches/2026-09-14 1120.md":
    "---\nproject: /proj\nbranch: m13/git\nhead: h800000\nat: src/OrderService.java:18\n---\n\n18 行那个 timeout 是临时的，上线前改回 300\n",
  // 粘了一段日志的一份（M10 ②）：段头、级别着色、「只看 WARN+」都在它身上验
  "/Users/you/Library/Application Support/com.liteide.app/scratches/2026-09-15 0930.md":
    "---\nproject: /proj\nbranch: m13/git\nhead: h800000\n---\n\n# 8842013 为什么落库失败\n\n从 order.log 里抠出来的：\n\n" +
    LINES.join("\n") +
    "\n\n看起来是连接池 10 个不够，先把 maximumPoolSize 调到 20 观察。\n\n改了这一处（②b 的 diff 段）：\n\n" +
    "diff --git a/src/OrderService.java b/src/OrderService.java\nindex 1111111..2222222 100644\n--- a/src/OrderService.java\n+++ b/src/OrderService.java\n" +
    "@@ -17,3 +17,3 @@ public void persist(Order order) {\n     var conn = pool.getConnection();\n-    int timeout = 300;\n+    int timeout = 5000;\n     try {\n\n上线前记得改回来。\n",
  // 别的项目的一份：折在「其他」组里
  "/Users/you/Library/Application Support/com.liteide.app/scratches/2026-09-13 1800.md":
    "---\nproject: /Users/you/other\nbranch: main\n---\n\n另一个项目的笔记\n",
  /*
   * issue #13 的现场，摆在桩里才看得见。
   *
   * `build/` 是**源码**（CMake 项目把构建脚本放这儿，而且提交进仓库），
   * `dist/` 是产物。两个名字都在「有争议」那一档，判据是 git 忽不忽略 ——
   * 桩里由 `GIT_IGNORED` 扮演那个答案。
   *
   * 生成物目录里也给真内容：树里点得开却读不出来，是桩自己在骗人。
   */
  "/proj/build/toolchain.cmake": "set(CMAKE_OSX_DEPLOYMENT_TARGET 13.0)\n# NEEDLE 这行搜得到\n",
  "/proj/dist/bundle.js": "// NEEDLE 这行搜不到，dist 被 git 忽略了\n",
  "/proj/node_modules/svelte/package.json": '{ "name": "svelte", "version": "5.0.0" }\n',
  "/proj/node_modules/.package-lock.json": '{ "lockfileVersion": 3 }\n',
  "/proj/target/debug/build.log": "NEEDLE 生成物里的噪声\n",

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
  "/proj/src/NewController.java": `package com.etianqu.api.lawyer.controller.ai;

import com.etianqu.framework.shared.response.Response;
import com.etianqu.share.client.ShareClient;
import io.swagger.v3.oas.annotations.Operation;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;

/** 新加的分享接口。桩里用它撑起「整个文件都是新增」那种 diff */
@RestController
public class NewController {

    private final ShareClient shareClient;

    public NewController(ShareClient shareClient) {
        this.shareClient = shareClient;
    }

    @Operation(summary = "创建分享")
    @PostMapping("/share")
    public Response<String> create(@RequestBody String body) {
        // 超时 5000 是压测之后定的
        int timeout = 5000;
        return Response.ok(shareClient.create(body, timeout));
    }
}
`,
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

    // 成员跳转（2026-09-23）：⌘Click ping 跨模块跳到 OrderClient 的方法，tag 跳同包那个类的方法
    public String status() {
        return orderClient.ping() + helper.tag();
    }
}
`,
  "/proj/moduleA/src/main/java/com/demo/api/SamePkgHelper.java": `package com.demo.api;

public class SamePkgHelper {
    public String tag() { return "同包，不写 import"; }
}
`,
  "/proj/moduleA/src/test/java/com/demo/api/AdminControllerTest.java": `package com.demo.api;

class AdminControllerTest {
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

/**
 * 草稿目录。真实现是 `~/Library/Application Support/com.liteide.app/scratches`，
 * 桩里给一条形状一样的绝对路径就够了 —— 前端只把它当不透明的路径传来传去。
 */
export const SCRATCH_DIR = "/Users/you/Library/Application Support/com.liteide.app/scratches";
/**
 * 生成物目录名。**必须和 `crates/excludes::GENERATED_DIRS` 一份**。
 *
 * 桩里手抄一份是没办法的事（那是 Rust 常量），所以下面的 DIRS 里
 * 特意摆了一个真的 `node_modules` 和一个真的 `build` —— 不摆的话，
 * 「生成物压暗」这条在浏览器里一次都看不到。
 */
export const GENERATED = new Set(["node_modules", "target", "dist", "build", "venv", "__pycache__", "vendor"]);

/**
 * 搜索侧**不跳**的那一档：名字有争议，要问过 git 才算数。
 * 判据抄 `excludes::CONTESTED_DIRS`。
 */
export const CONTESTED = new Set(["dist", "build", "vendor"]);
/**
 * 桩里扮演 `gitsvc::ignored_dirs` 的答案：只有 `dist/` 被忽略，`build/` 不被忽略。
 *
 * **这一条是 issue #13 的全部意思。** 两个目录名都在「有争议」那一档，
 * 而结果一个搜得到一个搜不到 —— 分界线不是名字，是 git。
 */
export const GIT_IGNORED = new Set(["dist", "node_modules", "target"]);

export const DIRS: Record<string, Array<[string, boolean]>> = {
  // 应用日志所在的目录。它**不在项目里**，只有「帮助 → 打开应用日志」够得着
  "/Users/you/Library/Logs/com.liteide.app": [["app.log", false]],
  // 草稿目录里预放两份 —— 不放的话侧边栏的草稿列表在浏览器里永远是空态（issue #40）
  [SCRATCH_DIR]: [["2026-09-10 1644.md", false], ["2026-09-12 0915.md", false], ["2026-09-14 1120.md", false], ["2026-09-15 0930.md", false], ["2026-09-13 1800.md", false], ["2026-09-16 2310.md", false]],
  "/proj": [["src", true], ["moduleA", true], ["moduleB", true], ["logs", true], ["docs", true], [".github", true], ["node_modules", true], ["target", true], ["build", true], ["dist", true], [".env", false], [".gitignore", false], ["README.md", false], ["package.json", false], ["pom.xml", false], ["Cargo.toml", false], ["vite.config.ts", false]],
  // 生成物目录里也要有东西 —— 空目录点开只有一行「空」，看不出「点得开」这件事
  "/proj/node_modules": [["svelte", true], [".package-lock.json", false]],
  "/proj/node_modules/svelte": [["package.json", false]],
  "/proj/target": [["debug", true]],
  "/proj/target/debug": [["build.log", false]],
  "/proj/build": [["toolchain.cmake", false]],
  "/proj/dist": [["bundle.js", false]],
  "/proj/.github": [["workflows", true]],
  "/proj/.github/workflows": [["ci.yml", false]],
  "/proj/src": [["OrderService.java", false], ["main.py", false], ["gbk-legacy.java", false], ["big5-notes.txt", false], ["long.ts", false]],
  // 跳转用的那对 Java（见 FILES 里的说明）。目录一层层列出来，
  // 否则文件树点不进去，而 ⌘P 又能搜到 —— 两边对不上就是桩在骗人
  // 模块底下有 pom.xml —— 不放的话「合并单层目录」会把 moduleA/src/main 并成一行，真项目里不会这样
  "/proj/moduleA": [["src", true], ["pom.xml", false]],
  "/proj/moduleA/src": [["main", true], ["test", true]],
  "/proj/moduleA/src/main": [["java", true]],
  "/proj/moduleA/src/main/java": [["com", true]],
  // Finder 打开过的目录会有 .DS_Store：桩照 Rust 侧不列它，也不让它截断 com.demo.api 那条链
  "/proj/moduleA/src/main/java/com": [["demo", true], [".DS_Store", false]],
  "/proj/moduleA/src/main/java/com/demo": [["api", true]],
  "/proj/moduleA/src/main/java/com/demo/api": [["AdminController.java", false], ["SamePkgHelper.java", false]],
  "/proj/moduleA/src/test": [["java", true]],
  "/proj/moduleA/src/test/java": [["com", true]],
  "/proj/moduleA/src/test/java/com": [["demo", true]],
  "/proj/moduleA/src/test/java/com/demo": [["api", true]],
  "/proj/moduleA/src/test/java/com/demo/api": [["AdminControllerTest.java", false]],
  // moduleB 故意没有 test：src 底下只有 main，合并成「src/main」一行，看得到 / 连接的那一种
  "/proj/moduleB": [["src", true], ["pom.xml", false]],
  "/proj/moduleB/src": [["main", true]],
  "/proj/moduleB/src/main": [["java", true]],
  "/proj/moduleB/src/main/java": [["com", true]],
  "/proj/moduleB/src/main/java/com": [["demo", true]],
  "/proj/moduleB/src/main/java/com/demo": [["core", true]],
  "/proj/moduleB/src/main/java/com/demo/core": [["OrderClient.java", false]],
  "/proj/logs": [["access-2026-08-24.log", false]],
  "/proj/docs": [["ARCHITECTURE.md", false]],
};

export const parentOf = (p: string) => p.slice(0, p.lastIndexOf("/"));
export const nameOf = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/**
 * 桩里这条路径存不存在。
 *
 * 三处都要查：`DIRS` 的键（目录自己）、`FILES`（有内容的文件），
 * **以及父目录的清单** —— 日志文件只在清单里出现，`FILES` 里没有它
 * （`probe_path` 是按 `.log` 后缀判的）。少了第三条，右键改名一个 `.log`
 * 会报「不在盘上了」，而它明明就在树上摆着。
 */
export const existsInMock = (p: string) =>
  DIRS[p] !== undefined ||
  FILES[p] !== undefined ||
  (DIRS[parentOf(p)] ?? []).some(([n]) => n === nameOf(p));

export const enc = new TextEncoder();

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
export interface LogSrc {
  /** 第 n 行。大文件走循环，小文件走真数组 */
  at: (n: number) => string;
  total: number;
  /** 第 n 行的级别下标，顺序同 Rust 侧 Level */
  levelAt: (n: number) => number;
  /** `log_stat` 报的六个级别计数 */
  levels: number[];
  bytes: number;
}

export const lineAt = (n: number) => LINES[n % LINES.length];

/** 那个 1GB 的演示日志。没有对应真文件时一律用它 */
export const BIG_LOG: LogSrc = {
  at: lineAt,
  total: TOTAL,
  levelAt: (n) => LINE_LEVEL[n % LINES.length],
  levels: [456_822, 914_684, 5_026_804, 2_742_487, 0, 910],
  bytes: 1_073_741_885,
};

/** 句柄 → 它的数据源 */
export const LOG_SRC: Record<number, LogSrc> = {};
/** 上一次被问到的那个句柄的源。过滤那几条命令都带 handle，直接查表 */
export const src = (h: unknown) => LOG_SRC[Number(h)] ?? BIG_LOG;

/** 文件指纹。桩里用一个自增计数模拟 mtime */
export const STAMPS: Record<string, { mtimeMs: number; size: number }> = {};
export let clock = 1_700_000_000_000;
export function bump(path: string) {
  clock += 1000;
  STAMPS[path] = { mtimeMs: clock, size: FILES[path]?.length ?? 0 };
  return STAMPS[path];
}
export function stampOf(path: string) {
  return STAMPS[path] ?? bump(path);
}

/** 桩里模拟耗时用。真实现的每一段进度之间本来就有间隔 */
export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/**
 * 内容搜索的匹配，**对齐真实现的 rg**：pattern 当正则、`--smart-case`（没大写就不区分大小写）。
 * 原来一律 `toLowerCase().includes()`，搜「Order」会多回来 `order-service` 那几行 ——
 * 界面上高亮（`fuzzy.ts` 的 `snippet`，也是 smart-case）正确地不标它们，于是浏览器里
 * 看到「有结果却没高亮」，去查高亮却查不出毛病。桩和真实现分叉就是这么骗人的。
 * 不是合法正则就退回字面量（rg 那边会报错，桩里不值得模拟一条错误路径）。
 */
export function grepMatcher(pattern: string): (text: string) => boolean {
  const sensitive = /[A-Z]/.test(pattern);
  try {
    const re = new RegExp(pattern, sensitive ? "" : "i");
    return (t) => re.test(t);
  } catch {
    const p = sensitive ? pattern : pattern.toLowerCase();
    return (t) => (sensitive ? t : t.toLowerCase()).includes(p);
  }
}

/** 处理函数说「这条命令不归我」—— 和返回 `null` / `undefined` 区分开 */
export const NOT_MINE = Symbol("not-mine");

/** invoke 的参数。原来是 `invoke` 里的 `const a = args as unknown as …`，类型原样搬过来 */
export type A = Record<string, number & string & boolean>;
