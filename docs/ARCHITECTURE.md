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
| 7 | M4 上 LSP（含 Java） | **Java 不上 LSP**，用 tree-sitter 做符号索引 | jdtls 启动 5–10s、常驻 1GB+ 内存，与"轻量"定位直接冲突。看陌生 Java 代码要的是"跳转定义 + 大纲"，tree-sitter 够了。JS/Python 的 LSP 轻，可选开 |

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
```

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
│  ├─ App.svelte
│  ├─ app.css                    # IDEA Dark design tokens（见 PLAN 附表）
│  └─ lib/
│     ├─ ipc/                    # commands.ts（invoke 封装 + 手写 DTO，靠 dto_sync 测试卡住漂移）
│     ├─ logview/    ★           # LogView.svelte / virtual-list.ts / line-cache.ts
│     │                          # colorize.ts / filter-bar.svelte
│     ├─ editor/                 # Editor.svelte / theme-idea-dark.ts
│     │                          # langs.ts（四语言懒加载）/ markdown-live.ts
│     ├─ shell/                  # Toolbar / FileTree / Tabs / StatusBar
│     ├─ search/                 # 双击 Shift 随处搜索浮窗
│     ├─ terminal/               # xterm.js 封装
│     └─ stores/                 # Svelte 5 runes
└─ src-tauri/
   ├─ tauri.conf.json            # bundle id 固定 com.liteide.app（UNINSTALL.md 的前提）
   ├─ src/
   │  ├─ main.rs
   │  ├─ commands/               # #[tauri::command] 薄封装，不写业务逻辑
   │  └─ state.rs                # 会话表 handle -> LogSession
   └─ crates/
      ├─ logengine/   ★          # src/{index,mmap,reader,session}.rs + benches/
      ├─ fsservice/
      ├─ searchsvc/
      └─ ptysvc/
```

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
| M6 可选 | tree-sitter 符号索引（跳转定义/大纲），JS/PY 轻量 LSP | 按需 | — |

合计约 **9 周**业余时间。**M0 那一周是全项目的生死线**——
它用最小成本回答"这条技术路线到底通不通"，通了后面全是已知工程，不通则及早止损。

---

## 7. 性能预算与验收标准（写死，实现时对表）

| 指标 | 目标 | 对照 VSCode |
|---|---|---|
| 冷启动到可交互 | **< 0.5s** | ~3.0s |
| 打开 1GB 日志（M0 实测 1.76ms） | **< 1s** ✅ | 卡死 |
| 空闲内存 | **< 200MB** | 650MB+ |
| 打开 1GB 日志到首屏 | **< 1s** | 卡死 |
| 1GB 日志滚动帧率 | **60fps** | 不可用 |
| 1GB 日志常驻内存 | **< 200MB**（与文件大小无关） | 不可用 |
| 全文过滤 1GB | **< 2s**（rg 子进程） | 不可用 |
| 安装包 | **~10MB** | 100MB+ |
| 进程数 | ≤ 5 | 23+ |

---

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
