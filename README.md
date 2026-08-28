# lite-ide

[![CI](https://github.com/Spc-jgs/lite-ide-mac/actions/workflows/ci.yml/badge.svg)](https://github.com/Spc-jgs/lite-ide-mac/actions/workflows/ci.yml)

macOS 上 1 秒打开的个人工作台。GB 级日志秒开不卡，代码高亮够用就停，Markdown 所见即所得。

Tauri 2 + Svelte 5 + CodeMirror 6，日志引擎自研（mmap + 稀疏索引）。
**没有插件系统、没有遥测、没有更新器。**

> 这是给自己用的工具，公开出来是因为里面几个决定可能对别人有用 ——
> 尤其是「GB 级文件怎么秒开」和「什么时候该起子进程而不是链库」这两件事。
> 它没打算成为通用编辑器，也不接受「加个插件市场」这类方向的需求。

---

## 为什么又造一个

不是因为 VSCode 不好，是因为它在**我最高频的那件事**上是弱项：看几百 MB 到几 GB 的
Java 服务日志。Chromium 的字符串模型决定了大文件要么卡死要么爆内存。而我为了这件事
装一个 IDE，代价是空载 300–500MB、冷启动几秒。

所以 lite-ide 只做四件事，按优先级：

1. **看 GB 级日志** —— 秒开、按级别过滤、tail、堆栈折叠
2. **读改代码** —— Java / JS / TS / Python 等 67 种语言，够用的高亮与符号大纲
3. **Markdown 笔记** —— 所见即所得，光标所在行显示源码
4. **偶尔敲命令** —— 真 pty，能跑 gradle / npm

加上一套完整的 Git（改动、历史泳道图、分支、工作树、双栏差异、冲突解决）。

## 实测数字

在 1GB / 9,141,707 行的 Java 日志上（M 系列 Mac，`--release`）：

| | 实测 | 说明 |
|---|---|---|
| 打开耗时 | **0.38 ms** | mmap 是 O(1)，不读盘 |
| 首屏 50 行 | **0.008 ms** | 索引在后台建，不阻塞 |
| 索引全文 | 145 ms | 6.87 GB/s，后台跑 |
| 索引内存 | **69.8 KB** | 每 1024 行一个 checkpoint；全量偏移要 69.7 MB |
| 按级别过滤 | 86 ms | 45.6 万条命中 |
| 常驻内存 | **98 MB** | 含 mmap 引擎 + WebKit 两个进程 |
| 二进制 | 4.9 MB | `.dmg` 2.7 MB |

细节与踩过的坑见 [docs/BENCHMARK.md](docs/BENCHMARK.md)。

## 装

需要 macOS、[Rust](https://rustup.rs)、Node 20+、pnpm。

```bash
pnpm install
pnpm app:bundle      # 打包
```

产物就在项目目录里，**不往系统里装任何东西**：

```
src-tauri/target/release/bundle/
├── macos/lite-ide.app     ← 双击它就能用，Spotlight 也搜得到
└── dmg/*.dmg              ← 发给别人的
```

刻意不往 `~/Applications` 复制一份：两份 `.app` 只会在 Spotlight 里出现两个
同名条目，而它们迟早分叉 —— 点错的那次，你调的是几天前的构建。

日常开发：

```bash
pnpm app             # Tauri 开发模式（改 Rust 要等重编译）
pnpm dev             # 纯前端 + IPC 桩，改 UI 是毫秒级热更新
```

> `pnpm app:build` 只编可执行文件，**不会更新 `.app`**。
> 拿不准手上跑的是哪个构建：标题栏上把鼠标停在项目名/应用名上，提示里有构建时间。

完整用法、快捷键、卸载见 [docs/USAGE.md](docs/USAGE.md)。

## 两种模式

打开文件时自动判定，判据是复合的：**大小 > 32MB、行数 > 30 万、最长行 > 1 万字符、
含 NUL 字节** —— 任一超标就走只读的日志模式。状态栏可以手动切回来。

| | 编辑模式 | 日志模式 |
|---|---|---|
| 引擎 | CodeMirror 6 | 自研 mmap + 稀疏索引 |
| 内存 | 与文件大小成正比 | 与文件大小**无关** |
| 能做 | 编辑、保存、符号大纲、Markdown 预览 | 级别过滤、正则搜索、tail、堆栈折叠 |

这两条路**不共享除主题 token 以外的任何东西**。让 CM6 去扛 GB 级文件，或者让日志视图
支持编辑，都会把两边一起拖垮。

## 几个可能对别人有用的决定

**稀疏索引，不是全量偏移。** 每 1024 行存一个字节偏移，读某一行时从最近的 checkpoint
往前扫。1GB 文件索引占 69.8 KB 而不是 69.7 MB —— 差 1024 倍，这是「内存与文件大小无关」
能不能兑现的分界线。

**数据面走二进制，不走 JSON。** Tauri 的 `invoke` 默认序列化成 JSON，传 1000 行约 15ms；
`tauri::ipc::Response` 传二进制约 1ms。60fps 单帧预算只有 16ms，这个差距是决定性的。

**Git 和搜索都起子进程，不链库。** `.gitignore` 的优先级规则、`core.excludesfile`、
worktree、submodule、rename 检测 —— 自己实现永远在追移动靶，而 git 本身就是这些规则的
定义。代价是每次调用 5–15ms 的进程启动，对「焦点变化时刷新」这种频率完全够用。
libgit2 静态链进来要多 2MB，整个 `.app` 现在才 4.9MB。

**重的东西一律按需加载。** CM6 约 340KB、xterm 约 250KB、Git 那套约 60KB、67 个语言包 ——
只看日志的人一个都不该付钱。入口包 126 KB。

**快照发布，不用读写锁。** 索引在后台线程建，`Mutex<Arc<LineIndex>>` 每次发布一个新快照。
第一版用 `RwLock`，写者饿死读者，首屏要等 1112ms —— 正好是建完整索引的时间。改成快照后
0.008ms。

更多在 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 文档

| | 是什么 |
|---|---|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构决策长什么样，以及对原方案的七处修正 |
| [BENCHMARK.md](docs/BENCHMARK.md) | 性能数字与实现陷阱 |
| [JOURNAL.md](docs/JOURNAL.md) | 时间线上每一步的经过与取舍，含每个踩过的坑 |
| [USAGE.md](docs/USAGE.md) | 怎么装、快捷键速查、两种模式的区别 |
| [RELEASE.md](docs/RELEASE.md) | 打包、产物在哪、怎么发版、CI 在做什么 |
| [PLAN.md](PLAN.md) | 立项时的调研与产品方案（历史文档，已被 ARCHITECTURE 修正过） |
| [UNINSTALL.md](UNINSTALL.md) | 怎么卸干净 |

## 结构

```
src/                    前端（Svelte 5 + CM6）
  lib/logview/          日志视图：虚拟滚动、块解析、格式识别
  lib/editor/           编辑器：语言表、Markdown 实时预览、缩略图、符号大纲
  lib/git/              Git：改动面板、泳道日志、双栏差异、冲突解决、分支选择器
  lib/shell/            文件树、标签栏
  lib/dev/mock-ipc.ts   浏览器里的 IPC 桩，生产构建会被 tree-shake 掉

src-tauri/
  src/commands.rs       命令层：只解包参数和转错误，不写业务
  crates/logengine/     日志引擎（零 Tauri 依赖，可单独 bench）
  crates/fsservice/     文件读写 + 编码检测
  crates/gitsvc/        Git（起 git 子进程）
  crates/searchsvc/     文件索引与内容搜索（起 rg，没有则内置实现）
  crates/ptysvc/        真 pty
```

五个 crate 都**不依赖 Tauri**，可以脱离 GUI 单测和跑 bench。这是刻意的：
日志引擎是这个项目唯一的技术未知数，它必须能独立验证。

## 测试

```bash
cd src-tauri && cargo test --workspace    # Rust 95 条
pnpm check                                # 类型检查
pnpm test                                 # 前端纯函数 87 条断言
```

前端那 87 条不引测试框架：测的全是纯函数（diff 解析、双栏对照、泳道布局、
冲突解析、改动行标记），输入输出都是普通数据结构，Node 22+ 能直接跑 `.ts`。
为它们装一套 vitest 加一堆 transform 配置，维护成本比被测代码还高。

CI 每次 push 都跑这三样，外加一道**入口包体积门禁**（超过 160 KB 就失败）——
「重的东西不进入口包」这条红线很容易在「顺手加个 import」时破掉，
而破了之后没有任何症状，只是启动慢了一点。

## 状态

M0–M16 全部完成，日常在用。不追求功能完备，够自己用就停。

## 许可

MIT
