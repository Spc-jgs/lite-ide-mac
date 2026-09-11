# Lite IDE 技术架构

> 本文是 `PLAN.md`（产品方案）的工程落地篇。PLAN 回答"做什么"，本文回答"怎么做才不会崩"。
> 定稿日期：2026-08-26

---

## 0. 结论先行：这个方案有没有搞头

**有。而且赌点选得准。** 三条理由：

1. **痛点是真的，且 VSCode 结构性解决不了。**
   GB 级日志秒开不是伪需求 —— Java 后端每天面对几百 MB 到 GB 的日志，VSCode 打开即卡死，
   现状只能退回 `less` / `grep`。这不是 VSCode 没优化，是 Chromium 的字符串模型决定的，
   它永远不会解决。这个空白十年没人填（Sublime 能读但没日志语义，lnav 是 TUI 没 GUI），
   **这是整个项目唯一的、也是充分的存在理由**。

2. **范围克制是这份方案最值钱的部分。**
   不做插件系统 / 不做自动更新 / 不做设置 UI / 不做分发 —— 砍掉了 80% 的工程量，
   而这 80% 恰恰是个人项目的标准死法。PLAN 里那张"已敲定的决策"表，价值高于任何技术选型。

3. **没有一样是自研黑洞。**
   CM6 被 Obsidian 验证，ripgrep 被 VSCode 验证，mmap + 行索引被 Sublime / lnav / glogg 验证，
   Tauri 被 Zed 之外的一票产品验证。唯一要自己写的是大文件引擎，而它是**只读**的 ——
   工程量比"可编辑的大文件引擎"低一个数量级。这个"只读"的自我设限，是全方案第二聪明的决定。

**风险在哪：** 不在选型，在**执行顺序**。原计划 M0 先搭地基（Tauri 窗口 + 文件树 + 多标签 + CM6），
2–3 周之后才碰大文件引擎。这是反的 —— 地基那部分是确定能做成的已知工程，
大文件引擎是唯一的未知数。**先花 3 周做确定的事，再去撞唯一可能撞不通的墙，是最坏的排序。**
见第 6 节的里程碑重排。

---

## 1. 对原方案的 7 处修正

| # | PLAN 原方案 | 修正 | 理由 |
|---|---|---|---|
| 1 | 按 **50MB** 切换编辑/日志模式 | 复合判据：`size>32MB \|\| lines>300k \|\| maxLineLen>10k` | CM6 的瓶颈是**行数与单行长度**，不是文件大小。一个 40MB 的单行 JSON 比 200MB 的多行日志更容易让 CM6 死掉 |
| 2 | "mmap + **行偏移索引**" | **稀疏 checkpoint 索引**（每 1024 行存一个偏移） | 全量行偏移：1GB 日志约 800 万行 × 8B = **64MB 内存**，10GB 就是 640MB，"内存与文件大小无关"当场破功。稀疏后 1GB → **62KB**，10GB → 620KB，才是真的无关 |
| 3 | IPC 层未提 | 数据面强制走**二进制** `tauri::ipc::Response::new(Vec<u8>)` | Tauri 默认 invoke 走 JSON 序列化。传 1000 行日志：JSON ≈ 15ms，二进制 ≈ 1ms。滚动要 60fps 意味着单帧预算 16ms，JSON 方案直接出局 |
| 4 | 日志模式"虚拟滚动列表" | 明确**不复用 CM6**，自研只读虚拟列表 | CM6 的 EditorState 是不可变全文档模型，塞不进流式数据。日志模式与编辑模式共享的只有主题 token |
| 5 | M0 地基 → M1 日志引擎 | **反过来**，先做日志垂直切片 | 唯一技术风险点，早验证早止损。见第 6 节 |
| 6 | 空闲内存 **<100MB** | 修正为 **<200MB** | macOS 上 WKWebView 自身就是多进程（WebContent + Networking + GPU），Tauri 空窗实测 80–150MB。<200MB 仍然吊打 VSCode 的 650MB，不必为一个够不到的数字设 KPI |
| 7 | M4 上 LSP（含 Java） | **不上 LSP，也不上 tree-sitter**：跳转靠 CM6 已有的 Lezer 树 + import 解析 | jdtls 启动 5–10s、常驻 1GB+，与"轻量"直接冲突。而 tree-sitter 也不需要 —— CM6 为高亮本来就把文档解析好了，再挂一套 wasm parser 是同一份代码解析两遍，且每种语言几百 KB，撞入口包 150KB 的红线。2026-09-09 落地，见下方「跳转做到哪一层」 |

### 跳转做到哪一层（修正 7 的落地形态）

按**精度**分层，不按范围分。只做前两层，因为只有它们不需要类型解析就能确定：

| 层 | 靠什么 | 准不准 |
|---|---|---|
| 一、本文件里的声明 | CM6 已经解析好的 Lezer 树（`outlineOf`） | 准 |
| 二、`import` / `package` 推出来的文件 | 语言规范强制的「包路径 = 目录路径」 | 准 |
| 三、全项目按名字搜 | ripgrep | **不挂在跳转上**，见下 |

第三层是菜单里单独一项「在项目里找这个名字」，**不挂在 ⌘Click 上**：一个长得
像跳转、精度却是正则的东西，会在你最需要它的那一次把你带到错的地方，而你不会
怀疑它。GitHub 全站的 code navigation 也是按名字匹配，但它照样把候选列出来
让人挑，不假装自己知道答案。

**所以「有下划线」才是这个功能真正的产品。** ⌘hover 只在前两层命中时才画
下划线；给不出准确答案的词什么都不显示。对多模块的 Spring 项目还白捡一条：
跨模块的类在同一个项目根下查得到源码，第三方在 jar 里查不到 ——
下划线正好把「我们自己的代码」和「外部依赖」分开了。

**明确不做**：跳类型 / 跳实现 / 调用层次 / 重命名。这四个的价值恰恰在于
「穷尽」，而按名字匹配给不了穷尽性 —— 要它们就是要 LSP。

实现分两个文件：`src/lib/editor/jump.ts`（纯函数，17 条测试）和
`jump-ext.ts`（CM6 的下划线与点击）。文件索引复用 ⌘P 那一份，没有另建索引。

---

## 2. 系统架构

```
┌─ WebView 进程（WKWebView，系统自带）──────────────────────┐
│  Svelte 5（runes 状态）                                    │
│  ┌────────────────┐          ┌──────────────────────────┐ │
│  │ 编辑模式        │          │ 日志模式（只读）           │ │
│  │ CodeMirror 6   │          │ 自研虚拟列表 + 块 LRU 缓存 │ │
│  └────────────────┘          └──────────────────────────┘ │
│  外壳：工具栏 · 文件树 · 标签页 · 状态栏 · 随处搜索浮窗      │
└────────────┬─────────────────────────┬─────────────────────┘
             │ invoke (JSON)           │ Response/Channel (二进制)
             │ 控制面 · 低频            │ 数据面 · 高频
┌────────────┴─────────────────────────┴─────────────────────┐
│  Rust Core（主进程）                                        │
│   tokio runtime（异步 IO）  ·  rayon pool（索引构建）        │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ crates/logengine  ★ 硬骨头，零 Tauri 依赖，可独立 bench│  │
│  │   MappedFile → LineIndex → LogSession                 │  │
│  ├──────────────────────────────────────────────────────┤  │
│  │ crates/fsservice   notify 文件监听 + 文件树            │  │
│  │ crates/searchsvc   ripgrep 子进程（--json 流式）        │  │
│  │ crates/ptysvc      portable-pty → zsh                 │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────┬───────────────────────────────────┘
                          │ 子进程
                    rg  ·  zsh  ·  (M6 可选 lsp)
```

**为什么 logengine 要独立成 crate 且不依赖 Tauri：** 它是唯一需要 benchmark 和压力测试的模块。
独立后可以 `cargo bench` 直接拿 1GB 测试文件跑，不用启动整个 app。这是能不能持续优化的前提。

---

## 3. 大文件引擎详细设计（硬骨头，全项目唯一的技术风险）

### 3.1 分层

```
LogSession   会话态：过滤结果 / 搜索命中 / tail 开关 / 级别统计
    ↓
LineIndex    稀疏索引：行号 ⇄ 字节偏移
    ↓
MappedFile   mmap 管理：初始映射 / 追加增长 / logrotate 检测
    ↓
文件
```

### 3.2 LineIndex —— 稀疏 checkpoint（本方案的核心技巧）

```rust
pub struct LineIndex {
    /// 每 stride 行记录一个字节偏移
    checkpoints: Vec<u64>,
    /// 已索引到的字节位置（增量构建游标）
    indexed_upto: u64,
    /// 已确认的总行数
    line_count: u64,
    stride: u32,              // 默认 1024
}
```

**内存账（这是硬指标的兑现方式）：**

| 文件 | 行数(约) | 全量偏移索引 | 稀疏索引(stride=1024) |
|---|---|---|---|
| 100MB | 80 万 | 6.4 MB | **6 KB** |
| 1 GB | 800 万 | 64 MB | **62 KB** |
| 10 GB | 8000 万 | 640 MB ❌ | **620 KB** ✅ |

**定位第 N 行**（O(1) + 有界扫描）：
1. `base = checkpoints[N / stride]`
2. 从 `base` 起用 `memchr` 数 `N % stride` 个 `\n`
3. 最坏扫 1024 行 ≈ 100KB ≈ **10μs**，完全无感

**增量构建（首屏不等待）：**
- 打开：`mmap` 是 O(1) 的，不读盘 → 窗口立刻出来
- 同步扫前 64KB → **首屏立刻可渲染**
- 剩余交给 rayon 后台线程 `memchr` 全扫，每 16MB 发一次 `log:index-progress`
  事件，前端据此实时更新滚动条比例（从"未知长度"渐进到精确）
- 1GB 全量索引预期 **0.3–0.5s**（memchr 单线程 5–10GB/s，实际瓶颈在 page fault）
- 先做单线程，够快就不上并行分段

> **实现补记（M1）**：级别统计**不能**塞进索引扫描。顺路统计看似省一遍遍历，
> 实测把索引从 143ms 拖到 870ms（6×）—— 级别探测要逐字节看行首，成本远高于
> `memchr` 找换行。索引是关键路径（决定"打开后多久能准确滚动"），必须保持纯粹。
> 级别改为独立的第二个后台任务，与索引并行，88ms 跑完；顺带把每行级别存成
> 4bit 打包数组（1GB 约 4.4MB），级别过滤因此变成纯内存操作，点 chips 立即响应。
> 这是本引擎唯一与行数线性相关的结构，用它换过滤的即时性。

> **实现补记（M0 实测）**：索引结构**不能**用 `RwLock<LineIndex>` 让后台线程直接持写锁
> 分块推进 —— 写者放锁后立刻重新申请，读者根本抢不进去，首屏读取被拖到 1112ms。
> 必须改成**后台无锁构建 + 快照发布**（`Mutex<Arc<LineIndex>>`，读者只克隆 Arc）。
> 修后首屏读取 0.008ms。另外 mmap 必须 `advise(Advice::Sequential)`，
> 否则 26 万次 page fault 会把索引吞吐从 1.79GB/s 压到 0.77GB/s。详见 `docs/BENCHMARK.md`。

### 3.3 MappedFile —— tail 追随与 logrotate（最容易踩的坑）

**坑：mmap 的映射长度在创建时固定，文件被追加写之后映射不会自动变长。**

处理：
- 轮询文件 size 增长 → 重新 `MmapOptions::new().map()`
  （**M1 实做修正**：原计划用 `notify`，改为 500ms 轮询 —— macOS 的 FSEvents
  对单文件有秒级合并延迟，轮询反而更快更可控，还少一个依赖）
- 旧映射用 `Arc` 持有，等正在读的块释放后自然析构 —— **绝不能直接 drop**，
  否则正在渲染的行会读到已 unmap 的内存段（段错误）
- 只对新增字节补索引，`indexed_upto` 往后走，已有 checkpoint 不动
- **logrotate 检测**：inode 变化 或 size 变小 → 判定文件被轮转/截断 → 整个 session reset

### 3.4 块读取与 IPC 负载格式

前端请求 `log_lines(handle, start, count)`，Rust 返回**紧凑二进制**而非 JSON 数组：

```
[u32 count][u32 off_0][u32 off_1]...[u32 off_n][utf8 payload bytes]
```

前端 `TextDecoder` 按 offset 切片。相比 `Vec<String>` 走 JSON：
- 省掉 Rust 侧 serde 序列化 + JS 侧 JSON.parse
- 省掉字符串数组的 GC 压力（滚动时每帧都在产生垃圾）
- 实测量级差异：**15ms → 1ms**

### 3.5 编码

- 只探测 UTF-8 与 GBK（Java 老日志高频），用 `encoding_rs`
- 非法字节替换为 U+FFFD，**任何情况下不 panic** —— 日志文件本来就可能被截断在半个字符上

### 3.6 过滤：不返回内容，只返回行号

**反面做法**：Rust 里 grep 出所有匹配行的内容返回前端 —— 几百万命中直接爆内存。

**正确做法**（lnav / glogg 同款）：
- 只收集**命中行号** `Vec<u64>`：100 万命中 = 8MB，可接受
- 前端虚拟滚动维护"视图行 → 物理行"的映射，滚到哪取哪
- 过滤进度可查询，换条件时旧任务立即取消

> **M1 实做修正**：原计划起 `rg --json` 子进程，实际改为**进程内实现**
> （`aho-corasick`，支持大小写不敏感）。两条理由：文件已经 mmap 在内存里，
> 起 rg 会让它重新 IO 一遍 1GB；而单文件搜索用不上 rg 的看家本领
> （多文件遍历、gitignore 处理）。rg 留给 M4 的全局搜索。
> 实测 1GB / 914 万行：纯级别 86ms，带文本 158–284ms。

---

## 4. IPC 协议

**分两个面，这是性能纪律：**

### 控制面（`invoke`，JSON，低频）
```
open_file(path)                  -> { handle, mode: "edit"|"log", size, encoding }
log_stat(handle)                 -> { line_count, indexed, levels:{error,warn,info,debug} }
log_filter(handle, pattern, opts)-> filter_id          // 异步，进度走事件
log_tail(handle, enable)         -> ()
pty_spawn(cwd)                   -> pty_id
fs_tree(path, depth)             -> TreeNode[]
```

### 数据面（`Response` 二进制 / `Channel`，高频）
```
log_lines(handle, start, count)  -> ArrayBuffer        // 见 3.4 格式
pty_output                       -> Channel<&[u8]>
pty_ack(id, bytes)               -> ()                 // 背压的回程，见下
```

**`pty_ack` 是这里唯一一条反方向的高频调用。** 终端是全应用唯一一个
「设闸的答案不是截断」的子进程 —— 截断会把终端变成一个会骗人的终端，
所以走的是背压：前端在 `term.write(bytes, cb)` 的**回调**里报回消费了多少，
未确认量到 256KB 时 Rust 侧的读线程就先不读，让 pty master 的缓冲区
自己把 shell 顶回去。实测 `yes` 跑 600ms：没有闸读出 47MB，有闸 256KB。
判据和「前端不回话怎么办」写在 `ptysvc::Flow` 与
[.claude/rules/rust.md](../.claude/rules/rust.md)。

### 事件（Rust → 前端推送）
```
log:index-progress { handle, indexed_lines, done }
log:appended       { handle, new_lines }               // tail 模式
filter:progress    { filter_id, hits, done }
fs:changed         { path, kind }
```

**类型同步**（**已偏离原计划，2026-08-28 修正**）：原文写的是「用 `ts-rs` 导出到
`src/lib/ipc/types.ts`」。那套东西**从来没落地过** —— 两侧一直是手写两遍，
而这份文档一直宣称它存在。

不上 ts-rs 的理由（事后确认，不是偷懒）：要给 crate 加依赖、加生成步骤、
把生成物提交进仓库，而全部 DTO 只有 15 个、且都集中在 `commands.rs` 一个文件里。
代价与收益不成比例。

真正落地的是 `src-tauri/tests/dto_sync.rs`：**一条只读源码的测试**，解析
`commands.rs` 里带 `#[derive(serde::Serialize)]` 的结构体和 `commands.ts` 里的
`export interface`，逐字段比。漏改一侧就红，还会强制新 DTO 到 `PAIRS` 表里登记一行。
它同时卡住 `#[serde(rename_all = "camelCase")]` —— 少了它，`line_count` 会原样
序列化成 snake_case，而 TS 侧写的是 `lineCount`，运行时就是一个 `undefined`，
界面上表现为一片空白，没人会往类型上想。

---

## 5. 目录结构

```
lite-ide/
├─ rust-toolchain.toml          # pin 住 stable-1.98.0，防止 rustup update 后行为漂移
├─ package.json / pnpm-workspace.yaml
├─ src/                          # 前端（Svelte 5）
│  ├─ App.svelte                 # ⚠ 3500 行，见下面「已知的架构偏移」
│  ├─ app.css                    # 材质分层：外壳透光 / 内容挡光 / 浮层不透
│  └─ lib/
│     ├─ ipc/                    # commands.ts（invoke 封装 + 手写 DTO，靠 dto_sync 测试卡住漂移）
│     ├─ logview/    ★           # LogView / LogPane / FilterBar + line-cache
│     ├─ editor/                 # Editor.svelte / theme / markdown-live
│     │                          # langs.ts（识别，入口包要）+ langs-load.ts（67 种，跟着编辑器懒加载）
│     ├─ shell/                  # FileTree / Tabs / Icon / FileGlyph / ContextMenu / Crash
│     ├─ git/                    # GitPane / GitLog / DiffView / MergeView / BranchPicker / RemoteBars
│     ├─ search/                 # 双击 Shift 随处搜索 + 大纲 + 键位速查
│     ├─ terminal/               # xterm.js 封装
│     ├─ state/                  # Svelte 5 runes（keymap / session / doc / notify）
│     ├─ lazy/                   # lazy() / lazyGroup()，按需加载的唯一出处
│     └─ dev/                    # mock-ipc.ts，只在 DEV 构建里存在
└─ src-tauri/
   ├─ tauri.conf.json            # bundle id 固定 com.liteide.app（UNINSTALL.md 的前提）
   ├─ src/
   │  ├─ main.rs / lib.rs        # lib.rs 里装小 runtime、建菜单、挂窗口材质
   │  ├─ commands.rs             # #[tauri::command] 薄封装，不写业务逻辑
   │  ├─ menu.rs                 # 菜单栏（keymap.ts 的一份拷贝，menu_sync 卡住）
   │  └─ state.rs                # 句柄表：日志会话 / 过滤任务 / pty / 远程操作
   └─ crates/
      ├─ logengine/   ★          # index / mmap / reader / filter / level + benches
      ├─ applog/                 # 应用自己的运行日志（只记异常，2 份 × 2MB 封顶）
      ├─ excludes/               # 哪些目录不进视野：两档名字（确定的 / 要问 git 的），树与搜索共用
      ├─ fsservice/
      ├─ searchsvc/
      ├─ gitsvc/                 # 含 progress.rs / remote.rs（M7 的网络那半边）
      └─ ptysvc/
```

### 已知的架构偏移（2026-09-06 审查）

**`App.svelte` 3526 行、68 个函数、20 个 `$state`、21 个 `$effect`。**
上面这张图说前端按 `lib/` 分模块，而实际上「顶层壳 + 所有跨组件状态」全挤在一个文件里：
标签管理、会话恢复、Git 动作、面板与终端、菜单事件、快捷键分派、拖放。

它还没到「改不动」的程度（每一块内部都有注释说清了判据），但**下一次加功能之前该拆**。
拆的顺序按「依赖最少的先走」：

1. `lib/state/tabs.svelte.ts` —— 标签表 + `openPath` / `doClose` / `settled` 那一族（约 400 行）
2. `lib/state/panel.svelte.ts` —— 底部工具窗 + 终端列表（约 150 行）
3. `lib/git/actions.ts` —— `gitDo` 包着的那十几个动作（约 300 行）
4. 剩下的才是真正的「壳」：布局、菜单事件、键盘分派

**不要为了拆而拆**：`$state` 跨文件用要走 `.svelte.ts`，而组件里 `$derived`
的依赖收集不跨模块边界失效 —— 这条得先在一个小块上验过再往下推。

**纪律：`commands/` 里只做参数解包和错误转换，一行业务逻辑都不写。**
业务全在 crates 里，这样才能脱离 Tauri 单测和 bench。

---

## 6. 里程碑重排（关键改动）

原计划先搭 2–3 周地基再碰大文件引擎。**反过来：**

| 期 | 内容 | 时长 | 出口标准 |
|---|---|---|---|
| **M0 垂直切片** ★ | Tauri 空窗 + 拖入文件 + mmap 稀疏索引 + 二进制 IPC + 虚拟滚动。**丑没关系，只验证性能** | **1 周** | 拖入 1GB 日志：**打开 <1s、滚动 60fps、内存 <200MB**。达不到就地重新评估技术路线 |
| M1 日志模式完整 ✅ | 级别着色 + chips 过滤 + 文本搜索 + tail 吸底 + 堆栈视觉 + 级别统计 | 2 周 | 日常真能拿它替代 `less` 看线上日志 |
| M2 编辑模式 ✅ | CM6 + 四语言高亮 + 文件树 + 多标签 + IDEA Dark 主题落地 | 2 周 | 能舒服地改代码 |
| M3 终端 ✅ | portable-pty zsh + 退出时 kill 子进程 | 1.5 周 | 能跑 gradle/npm |
| M4 导航 ✅ | 双击 Shift 随处搜索 + ⌘P + 全局搜索 | 1.5 周 | 手不离键盘 |
| M5 Markdown ✅ | CM6 decoration live preview | 1 周 | 笔记体验对齐 Obsidian |
| M6 符号与跳转 ✅ | 大纲复用 Lezer 树；⌘Click / ⌘B 跳声明（本文件 + import 两层） | — | 看陌生代码不用切 IDEA |
| M7 拉取与推送 ✅ | `fetch` → `pull` → `push`，子进程 + 流式进度 + 可取消 | — | 提交完不用再切终端 |
| M7.1 askpass | `GIT_ASKPASS` 弹框，服务「第一次连一个新远程」 | 按需 | 口子已留在 `git_cmd` |

### M7 为什么值得做，以及它和前面所有 Git 功能都不是一类

**要做的理由很硬**：界面上已经在显示 `↑2 ↓3` 了 —— 那是一个**做不到的承诺**。
告诉人「你落后 3 个提交」却让他自己去开终端，比不显示更糟。
提交是本地闭环的最后一步，推送才是这件事真正的终点。

**但它和 gitsvc 现有的每一条命令都不是一类**，动手前先看清三件事：

1. **凭据。** 现在每一条 git 调用都写着 `GIT_TERMINAL_PROMPT=0`，
   目的正是「绝不让子进程卡住等输入」。fetch/pull/push 是**唯一**真的需要
   凭据的操作 —— 那个开关留着就是「需要密码的仓库当场失败」，
   去掉就是「后台刷新时应用静默挂死」。两条都不能接受。
   走向只有一条：**继续 `GIT_TERMINAL_PROMPT=0`，靠系统钥匙串的
   credential helper 和 ssh-agent**，拿不到凭据就**清清楚楚地失败**，
   并在界面上说清「这个仓库需要你先在终端里认证一次」。
   顺带一个具体的坑：从**访达**启动时，`SSH_AUTH_SOCK` 通常在（launchd 给的），
   但自定义的 `~/.ssh/config`、`GIT_SSH_COMMAND`、带口令的私钥都不在。
   在终端里 `git push` 好使，不等于在 `.app` 里好使。

2. **它们是长的、走网络的、必须能取消的。** 现有每一条 git 调用都是本地、
   毫秒级、`output()` 一把读完。fetch 在慢网上要几十秒，
   而且 **git 的进度是写到 stderr 的**——要按 AGENTS.md「子进程输出必须设闸」
   那条流式读，还要有取消路径。这是 gitsvc 里第一个需要「进行中」状态的操作。

3. **push 是第一个会改到别人东西的动作。** 在此之前所有操作都只碰本地盘。
   `--force` 一律不做入口，`--set-upstream` 只在「这个分支还没有上游」
   那一种情况下出现，且要说清它要建的是什么。

**顺序也是判据**：先 `fetch`（只读、不改工作区、失败了没有后果），
拿它把凭据和进度这两块地基趟平；再 `pull`（会产生冲突，
而冲突有 `MergeView` 接着，这条路已经是通的）；最后才 `push`。

反过来先做 push 的话，第一次撞上的就是「凭据没配好 + 改了远程 + 不知道改成什么样了」。

合计约 **9 周**业余时间。**M0 那一周是全项目的生死线**——
它用最小成本回答"这条技术路线到底通不通"，通了后面全是已知工程，不通则及早止损。

---

## 7. 性能预算与验收标准（写死，实现时对表）

| 指标 | 目标 | 对照 VSCode |
|---|---|---|
| 冷启动到可交互 | **< 0.5s** | ~3.0s |
| 打开 1GB 日志（M0 实测 1.76ms） | **< 1s** ✅ | 卡死 |
| 空闲内存 | **< 200MB** | 650MB+ |
| 进程数（实测 4） | ≤ 5 ✅ | 23+ |
| 打开 1GB 日志到首屏 | **< 1s** | 卡死 |
| 1GB 日志滚动帧率 | **60fps** | 不可用 |
| 1GB 日志常驻内存 | **< 200MB**（与文件大小无关） | 不可用 |
| 全文过滤 1GB | **< 2s**（rg 子进程） | 不可用 |
| 安装包 | **~10MB** | 100MB+ |

### 2026-09-06 实测（`.app`，会话恢复了几个标签 + 一个终端）

| 进程 | 线程 | Physical footprint |
|---|---|---|
| `lite-ide`（我们自己的） | 8–10 | **32 MB** |
| `WebKit.WebContent` | 4 | 106–173 MB |
| `WebKit.GPU` | 6–8 | 16–29 MB |
| `WebKit.Networking` | 3–4 | 4.5 MB |
| **合计** | **21–26** | **155–238 MB** |

三条要说清的：

- **Rust 侧只占 32MB**，涨的那部分全在 WebKit 的渲染进程上，随着打开的
  编辑器 / 终端 / 日志走。「< 200MB」这条预算**在开了几个标签之后会破**，
  它当初写的是「空闲」，而这个应用没有真正的空闲态（会话恢复会把上次的标签开回来）。
- 主进程线程数 **24 → 13**：Tauri 默认的 async runtime 起 `available_parallelism()`
  个 worker，这台 18 核机器上就是 18 条常驻空转的 `tokio-rt-worker`。
  换成 2 个 worker + 阻塞池（`lib.rs::install_runtime`）。
- **线程少了 11 条，footprint 没降**（29MB 上下，在噪声里）——
  空转线程的栈是虚拟的，几乎不落物理页。这条改动买的是结构正确性
  （阻塞的活不占 worker），不是内存。

---

## §7.5 怎么知道它出了问题（2026-09-10 补）

这一节回答的是一个具体的问题：**一个装好的 `.app` 出了事，我们怎么知道。**

### 在这之前的答案是「不知道」

| 层 | 手段 | 盖得住什么 |
|---|---|---|
| 写代码时 | `pnpm check`、14 个前端测试、`cargo test --workspace` | 纯函数、类型 |
| 跨语言接缝 | `dto_sync.rs` / `menu_sync.rs` | 两侧漂移 —— **这个仓库最值钱的一类测试** |
| 组装完 | `scripts/smoke.sh`，驱动真 `.app` | 端到端 |
| 进 CI | 入口包 150KB 红线 | 体积回归 |
| **发出去之后** | **几乎没有** | |

最后一行是缺口。前端 `window.onerror` 打到 `diag` → stderr，而 `diag` 默认闭嘴、
双击启动的 `.app` 又根本没有 stderr；Rust panic 同样；IPC 报错变成界面上一句话，
说完就没。**一次真实的崩溃不留任何痕迹，只能等人再复现一遍。**

「切到日志模式按了没反应」那个 bug 就是这样在盘上待了不知道多久，
最后靠用户截图才被发现。

### 现在的答案：三层，第一层已经落地

**一、让错误必然落成文件**（`crates/applog`，2026-09-10 落地）。
`~/Library/Logs/com.liteide.app/app.log`，**默认开** ——
`LITE_IDE_DEBUG=1` 那道门槛挡住的正是每一次真实故障，
「默认关掉的可观测性等于没有」。收 4 类：前端未捕获错误、Promise 拒绝、
CSP 违规、Rust panic。保留策略与隐私边界写在 [UNINSTALL.md](../UNINSTALL.md)。

这件事在这个项目上几乎白送：**它自己就是个日志查看器**，
「帮助 → 打开应用日志」就是用自研 mmap 引擎打开这个文件，
级别过滤和 tail 全是现成的。成本只剩「把字写下去」那一半。

**二、把「我以为如此」写成运行时自检**（`src/lib/state/invariant.ts`，
2026-09-11 落地，issue #27）。这个应用里有一批只活在脑子里的不变量：
日志模式的标签必须有引擎句柄、编辑模式的标签不该拿着句柄、
`dirty` 与 `draft` 必须同真同假、两个标签不能共用一个句柄、
`activeId` 必须指得着。不成立时**写日志（error）但不打断用户** ——
一个会因为自检失败而崩掉的编辑器，比一个偶尔状态不对的编辑器糟得多。
`stashed` 那个 bug 正是这个形状 —— 它当时不满足的是
「快照写回去不该改变 mode」，而它是靠用户截图才被发现的。

落地时有两件事和原来的设想不一样，都写进了代码注释：

- **`dirty ⟺ draft` 对正在被编辑的那个标签不成立。** 编辑器的 `onChange`
  只改 `dirty`，`draft` 要等 `onStash` 才回写 —— 不把这件事告诉自检器，
  它会在**每一次敲键盘**时报假警。所以 `audit` 要知道谁挂着活编辑器。
- **「恢复出来的标签数 = 存进去的数」不是不变量。** `restoreSession` 明确是
  「能恢复多少算多少」，上次开着的文件这次被删了就是会少一个。
  真正的不变量是反方向的那半边：不能凭空多出来（去重失效）。

收敛规则是**2 的幂**（第 1、2、4、8… 次才写）：只记一次分不出
「启动时抖了一下」和「从那以后一直不对」，每次都记则 2MB 的日志
几秒就被一条 assert 灌满。2 的幂两头都占，而且最后那行自带次数，
量级直接读得出来。

**三、把预算变成每次启动都量的数**（`src-tauri/src/budget.rs` +
`scripts/budget.sh`，2026-09-11 落地，issue #28）。启动完成时往日志写一行：

```
2026-09-11 06:00:01.512Z INFO [budget] boot=412ms self=34MB tabs=3 terms=0 editors=1 nodes=4210 v=0.9.0 devtools=0
```

三件事值得单独说：

- **`boot` 从进程真正的起点算**（`ri_proc_start_abstime`），不是从 `run()`
  开头打的点 —— 后者会把 dyld 和框架加载整段漏掉，而那恰恰是
  「加了个依赖之后启动变慢」最会体现的地方。
- **`self` 是 `ri_phys_footprint`，不是 RSS**，而且**只有主进程**。
  WebKit 那三个的 ppid 是 1，从进程内部认不出来 —— 它们归 `scripts/mem.sh`。
  三个符号（`proc_pid_rusage` / `mach_absolute_time` / `mach_timebase_info`）
  都在 libSystem 里，没有引 libc。偏移对不对拿 `vmmap --summary` 核过：
  201.2M vs 201MB。
- **没有 `langs`。** 数它要先 `await import("langs-load")`，而那是个按需
  加载的大块 —— **量它就等于把它加载起来**，`nodes` 和 `self` 跟着失真。

`budget.sh` 按「版本 + 标签数」分组取中位数，因为跨组比较得到的是
「用得多不多」而不是「胖没胖」。

### 比上面三条都优先的一条

**哨兵必须可信。** `smoke.sh` 是间歇红的（issue #22），而
**一个会误报的哨兵，过一阵就没人当真了** —— 2026-09-10 发版前跑了三次
才敢信第三次的结果，那已经是在教人忽略红色。所以修 #22 排在新增手段前面。

## 8. 开发期红线（与 UNINSTALL.md 的承诺绑定）

| 规则 | 原因 |
|---|---|
| npm 依赖一律进项目 `node_modules`，禁 `-g` | 卸载 = 删目录，零残留 |
| 不建 LaunchAgent / 登录项 / 常驻进程 | 删了就干净 |
| pty 与 rg 子进程必须随主窗口退出一并 kill | 防孤儿进程 |
| 配置缓存只写 `com.liteide.app` 标准目录 | 卸载路径确定 |
| bundle id 固定 `com.liteide.app`，永不改 | UNINSTALL.md 全部路径的前提 |
| `rust-toolchain.toml` pin 版本 | 防 rustup update 后编译行为漂移 |
| 前端入口包只放两种模式都要的东西 | CM6 核心约 340KB，静态引入会把入口从 71KB 顶到 412KB；日志模式用不上它，必须按需加载 |
| 验证必须用 `pnpm app:build`，不用 `cargo build` | `cargo build` 产出的是 dev 模式二进制，会去连 devUrl，验证的其实是 dev server（详见 BENCHMARK.md 坑四） |
| capabilities 只开实际用到的权限 | ACL 拒绝在前端表现为静默的 rejection，缺权限很难察觉 |
| CSP 不能为 `null`，且 `tauri.conf.json` 与 `vite.config.ts` 两处保持一致 | WebView 里的 XSS 在 Tauri 下等于拿到全部 IPC（任意读写文件 + 起子进程）。`style-src` 必须带 `'unsafe-inline'`：CM6 与 xterm 都在运行时往 head 里插 `<style>`。CSP 挡下东西不报错，只表现为「某处不好使」，所以 `main.ts` 里挂了 `securitypolicyviolation` 回传 |
| 任何可能无上限的子进程输出都要设闸 | `git diff` 会为一个 30MB 的新增文件原样吐 30MB。见 `gitsvc::MAX_DIFF_BYTES` |

---

## §9 前端状态的两条缝（M16）

`App.svelte` 长到快 2000 行时，第一反应通常是「按功能拆文件」。先量了一下
**谁在写谁的状态**，结论和直觉不一样：32 个函数里有 15 个跨领域写状态，
但拆开看，其中 12 个根本不是领域耦合，而是两件被抄了很多遍的小事。

### 缝一：状态消息

45 处 `error = …` / `saved = …`，每处后面跟一个自己写的
`setTimeout(() => (error = ""), N)`，N 有 1800 / 2200 / 2600 / 3000 / 3600 /
4000 六种值。

**这里藏着一个真 bug**：所有消息共用一个变量，却各起各的定时器。
先弹一条 1800ms 的「已保存」，紧接着来一条 4000ms 的错误 ——
那条错误会在 1800ms 时被前一条的定时器抹掉，用户来不及读。

`lib/state/notify.svelte.ts` 给每个通道一个定时器，新消息进来先取消旧的。
三个通道按「用户要不要动手」分：`ok` 做成了、`fail` 一句话说得清的失败、
`block` 多行说明（**不自动消失** —— 一段多行说明还没读完就被收走，
比不显示更气人）。

### 缝二：工作区被外部改了

切分支、丢弃改动、移除工作树、以及用户切到终端里敲完命令再切回来 ——
这四件事都属于「盘上的东西变了，但不是我们改的」，都必须同时做两件事：

- 重读已打开文件的内容（否则标签还显示旧分支的内容）
- 重列已展开的目录（否则树上还挂着已经不存在的文件）

原本这两行在四个地方各写一遍，**其中一处只写了后半句**。
现在是一个有名字的 `workingTreeChanged()`。

### 量出来的结果

| | 跨领域写状态的函数 |
|---|---|
| 之前 | 15 / 32 |
| 之后 | 3 / 33 |

剩下的三个都是本该如此的：`newTerm` / `closeTerm` 会打开底部面板（那就是功能），
`onWindowKey` 跨领域分发按键（那就是它的职责）。

**给这件事留个记号**：文件长不等于耦合重。先量再拆 —— 这次量完发现，
真正该做的是给两件事起名字，而不是把文件切成五份。切成五份之后，
那 12 处「顺手报个消息」会变成 12 处跨模块调用，只会更难看。

## §7.6 Git 控制台（2026-09-11，issue #29）

底部面板的第三页，形状抄 IDEA 的 Git → Console。**只读，不能在里面执行 git** ——
那是终端的事，而终端已经有了；一个半吊子的 git shell 只会让人分不清
哪个能干什么。

### 它补的两个缺口

1. **证据说完就没。** 失败时弹一条横幅，关掉就再也找不回来。
2. **界面上看不到跑的是什么命令。** 而我们给每条 git 都带着一串加固参数
   （`-c core.fsmonitor=`、`-c diff.external=`、`-c protocol.ext.allow=never`，
   外加仓库自带的每个 filter 驱动逐个关掉）—— 那是为了挡住「仓库自己的
   config 让 git 去执行东西」（issue #24）。哪天某个加固参数把一个正常仓库
   弄坏了，在这之前的界面给不出任何线索。

所以这一页显示的是**完整 argv**，加固参数一个不少。看着长，那正是要看的。

### 三个判据

| | |
|---|---|
| 记在哪 | **`run_capped_raw` 和 `remote::run_streaming` 各一处**，不在调用点 —— 同一条纪律写四遍就是迟早漏一遍（HARDENING 当初就是为此挪到 `git_cmd` 上的） |
| 存在哪 | **只在内存**（`gitsvc::console` 的一个环，300 条封顶）。落 `app.log` 会破掉那边「只写异常」的规矩：一次状态刷新就是一条 |
| 凭据 | `mask()`，**进环之前**打码。存原文再在展示时打码的话，凭据已经在进程内存里躺了一遍 |

**记全部，不只记失败。** issue 里担心「全记会把失败的淹掉」，
但这一页有「只看失败」的开关，而失败那条左边一道红竖线 ——
筛选是界面的事，不该靠丢掉数据来实现。而且「什么都没失败，但它到底跑了什么」
本身就是一个要回答的问题。

### 上线第一分钟照出来的事

打开这个仓库，控制台里 `ls-files --others --ignored --directory` **跑了两次**，
间隔 9 毫秒（17ms + 13ms）—— `App.svelte` 里那条取 gitignore 的 effect
同时依赖 `root` 和 `treeTick`，首屏时两个先后变了一次。
浪费不大，但**在有这一页之前，没有任何办法看见它**。
