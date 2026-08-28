# AGENTS.md

给在这个仓库里干活的 agent。下面每一条都是**已经踩过**的坑，不是预防性的规矩。

## 项目是什么

macOS 个人工作台。Tauri 2 + Svelte 5 + CodeMirror 6，日志引擎自研。
定位见 [README.md](README.md)，架构见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

**它不打算成为通用编辑器。** 「加个插件系统」「支持 Windows」「接 LSP」这类方向
在立项时就明确排除了（理由在 PLAN.md 和 ARCHITECTURE.md 里），不要顺手加。

---

## 验证纪律

### 只认 `pnpm app:build`，不要 `cargo build --release`

`cargo build --release` 产出的二进制是**开发模式**的 Tauri 壳 —— 它去加载
`http://localhost:1420/`，而不是打包进去的前端。跑起来是白屏或者旧界面，
而你会以为自己的改动生效了。

这个坑让好几处「已端到端验证」的结论失效过一次。

### `app:build` 不会更新 `.app`

`pnpm app:build` 只编 `target/release/lite-ide` 这个可执行文件。
`target/release/bundle/macos/lite-ide.app` 里那份是上一次 `app:bundle` 留下的，
可能差好几天 —— 而双击启动的正是后者。

要更新用户实际会打开的那个：`pnpm app:install`。

**报上来的 bug 复现不了时，先确认对方跑的是哪个构建**：标题栏悬停应用名会显示
构建时间。已经发生过一次「照着现象查了半天代码，最后发现 bug 早就修好了」。

### 改前端后 Rust 侧要能感知

`src-tauri/build.rs` 里有 `cargo:rerun-if-changed=../dist`。少了它，
改完前端重新构建，产物里嵌的还是旧的。

### 测试必须验证它会失败

加完一条测试，把被测代码改回错误的样子跑一遍，确认它真的红。
不会失败的测试比没有测试更糟 —— 它给你一个假的安全感。

（本仓库里 `检出远程分支要建跟踪分支` 这条就是这么验过的。）

---

## Rust 侧

### 命令层不写业务

`src-tauri/src/commands.rs` 只做三件事：解包参数、查句柄、转错误。
一行业务逻辑都不写。业务全在 `crates/` 里 —— 那四个 crate **不依赖 Tauri**，
才能脱离 GUI 单测和跑 bench。

日志引擎是这个项目唯一的技术未知数，它必须能独立验证。别把它跟 Tauri 绑死。

### 数据面走二进制

日志行**不能**用默认的 `invoke`（JSON）。传 1000 行 JSON 约 15ms、二进制约 1ms，
而 60fps 单帧预算只有 16ms。用 `tauri::ipc::Response` 传 `Vec<u8>`，
线格式在 `crates/logengine/src/block.rs`。

### 起子进程时的两条硬纪律

`gitsvc` 和 `searchsvc` 都起子进程（`git` / `rg`）：

1. **绝不拼 shell 字符串**。全部走 `Command::arg`，路径前一律加 `--` ——
   否则一个叫 `-f` 的文件就能变成命令行开关。
2. **绝不让子进程卡住等输入**。`GIT_TERMINAL_PROMPT=0` 关掉凭据提问；
   `GIT_OPTIONAL_LOCKS=0` 让 `git status` 不抢 index 锁（用户正在终端里
   跑 rebase 时，后台刷新不该把它顶失败）。

另外 `LC_ALL=C`：用户 locale 是中文时，别让 git 把机器格式翻译了。

### 新代码加在测试模块**之前**

用 `cat >>` 往文件尾追加过一次，结果生产代码落到了 `#[cfg(test)] mod tests` 后面，
读文件的人会以为测试模块之后就没东西了。

### git 的短名会吃掉 `/HEAD`

`refs/remotes/origin/HEAD` 的 `%(refname:short)` 是 **`origin`**，不是 `origin/HEAD`。
过滤这类引用要按**全名**判。同理 `git switch origin/foo` 会直接失败 ——
DWIM 只对短名生效，传全名要走 `--track`。

---

## 前端

### `$state` 数组里的元素：拿到手的可能是原始对象

```js
const tab = { id, ... };
tabs = [...tabs, tab];   // tabs 是 $state
activeId = tab.id;       // 这里渲染了一次
await something();
tab.diffRaw = raw;       // ← 改得动数据，但**不产生信号**，界面不会重渲染
```

数组元素是**读取时**才被包成代理的，局部变量拿的是创建时那个原始对象。
异步流程尤其容易踩：`await` 回来时手上的引用早已不是响应式那份。

**规则：所有异步路径按 id 重新从数组里取**（`tabById(id)`）。
这个 bug 表现为「差异面板一直显示没有差异」，很难往响应式上想。

### 按住 Shift 时 `e.key` 是大写

`e.key === "g" && e.shiftKey` 永远不成立 —— 规范里 `key` 是修饰后的字符值。
⌘⇧G / ⌘⇧O / ⌘⇧F 都因为这个悄悄失效过。统一小写化：

```js
const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
```

### CM6：不能在 `update()` 里读布局

会抛 `Reading the editor layout isn't allowed during an update`。
CM6 把一帧切成读、写两个阶段，正是为了避免读写交替触发强制重排。
用 `view.requestMeasure({ read, write })`。

滚动也别只靠 `ViewUpdate` —— 视口内的小幅滚动不一定触发 `viewportChanged`。
直接听 `view.scrollDOM` 的 `scroll` 事件。

### 重的东西一律按需加载

入口包只放两种模式都要的东西（ARCHITECTURE 的红线）。
CM6、xterm、Git 那套、67 个语言包全部 lazy。用 `src/lib/lazy/lazy.svelte.ts` 里的
`lazy()` / `lazyGroup()`，别再手写一遍 then/catch/finally 样板。

改完跑一下，确认入口包没长（当前 126 KB）：

```bash
pnpm build && ls -l dist/assets/$(grep -o 'assets/[^"]*\.js' dist/index.html | head -1 | cut -d/ -f2)
```

### 状态消息走 `notify`

别直接写 `error = …` 再自己 `setTimeout` 清除。
`src/lib/state/notify.svelte.ts` 三个通道：`ok` / `fail` / `block`（多行，不自动消失）。
各通道自己管定时器 —— 手写会出现「后一条消息被前一条的定时器抹掉」。

### 盘上的东西被外部改了，两件事要一起做

切分支、丢弃改动、动工作树、从终端切回来 —— 都要
`workingTreeChanged()`：重读已打开文件 **且** 重列已展开目录。
少做哪一件都会留下一个说谎的界面。

### 桩必须和真实现严格对齐

`src/lib/dev/mock-ipc.ts` 是浏览器里调 UI 用的（`pnpm dev`，热更新毫秒级，
比等 Tauri 重编译快得多）。它喂的数据结构**必须**和 Rust 侧 DTO 一致 ——
分叉之后它就失去了全部价值，还会骗人。改 DTO 记得同步改桩。

生产构建里 `import.meta.env.DEV` 为假，整个模块会被 tree-shake 掉。

---

## 怎么写文档和提交信息

这个仓库的文档有明确的调子，跟着来：

- **写为什么，不写是什么**。代码本身说得清「是什么」，注释和文档要回答
  「为什么是这样，以及试过哪条路不行」。
- **带上数字**。「快了很多」没有信息量，「1112ms → 0.008ms」有。
- **坑要写下来，连同它当时长什么样**。JOURNAL.md 的价值大半在这里。
- **不要写错的注释**。错的注释比没有注释更害人 —— 这条是有过教训的。
- 中文正文，技术术语和标识符保持原文。

提交信息同理：标题一句话说清做了什么，正文说清**为什么**和**验证方式**。

---

## 常用命令

```bash
pnpm install
pnpm dev                                  # 浏览器 + IPC 桩，改 UI 用这个
pnpm app                                  # Tauri 开发模式
pnpm app:build                            # 只编可执行文件
pnpm app:install                          # 打包并装到 ~/Applications
pnpm check                                # 类型检查
cd src-tauri && cargo test --workspace    # Rust 测试
```

日志引擎的 bench（需要先生成样本）：

```bash
cd src-tauri
cargo run -p logengine --example gen_log --release      # 造 1GB 样本
cargo run -p logengine --example bench --release
cargo run -p logengine --example bench_filter --release
```
