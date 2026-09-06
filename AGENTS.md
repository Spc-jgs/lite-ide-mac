# AGENTS.md

macOS 个人工作台。Tauri 2 + Svelte 5 + CodeMirror 6，日志引擎自研（mmap + 稀疏索引）。

## 命令

```bash
pnpm dev            # 浏览器 + IPC 桩（src/lib/dev/mock-ipc.ts）。改 UI 用这个，热更新毫秒级
pnpm app            # Tauri 开发模式
pnpm app:build      # 只编可执行文件
pnpm app:bundle     # 打包 .app + .dmg，产物在 src-tauri/target/release/bundle/
pnpm check          # svelte-check
pnpm test           # 前端纯函数测试
cd src-tauri && cargo test --workspace
```

## 验证纪律

**验证只认 `pnpm app:build`。** `cargo build --release` 产出的是**开发模式**的 Tauri 壳 ——
它去加载 `http://localhost:1420/` 而不是打包进去的前端，跑起来是白屏或者旧界面，
而你会以为改动生效了。这条让好几处「已端到端验证」的结论失效过一次。

**`app:build` 不更新 `.app`，`app:bundle` 才更新。** 双击启动的是
`bundle/macos/lite-ide.app` 那一份，它可能比可执行文件旧好几天。
复现不了别人报的 bug 时**先确认对方跑的是哪个构建** —— 悬停标题栏的项目挂件，
tooltip 第二行是构建时间。已经发生过一次「照着现象查了半天，最后发现早就修好了」。

**盘上只留一份 `.app`。** 往 `~/Applications` 复制过一份，结果 Spotlight 里两个同名图标，
只要有一次「打包了没重装」就分叉，点错的那次调的是几天前的构建。

**加完测试要把被测代码改回错误的样子跑一遍，确认它真的红。**
不会失败的测试比没有测试更糟 —— 它给一个假的安全感。
（`检出远程分支要建跟踪分支`、`截断的路径列表要丢掉末尾那条半截的` 都是这么验过的。）

## 边界

**一律要做：**

- 起子进程前先问「它的输出有上限吗」，没有就设闸
- 改了过 IPC 的 DTO，两侧一起改 —— `src-tauri/tests/dto_sync.rs` 会卡住漂移
- 改了键位或菜单，先改 `src/lib/state/keymap.ts` —— `menu_sync.rs` 会卡住漂移
- 改了 Rust 侧 DTO / 命令，同步改 `src/lib/dev/mock-ipc.ts`。桩一分叉就开始骗人
- 改完给出数字。「快了很多」没有信息量，「1112ms → 0.008ms」有

**先问再做：**

- 加依赖（尤其是会进入口包的前端依赖 —— CI 卡 150 KB 红线）
- 改 `tauri.conf.json` 的 `bundle.identifier`、CSP、窗口透明相关的任何一项
- 提交、推送、建分支

**不做：**

- 插件系统、Windows 支持、LSP、遥测、自动更新器 —— 立项时就明确排除了，
  理由在 [PLAN.md](PLAN.md) 和 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- 拿 `cargo build --release` 当验证
- 在 `src-tauri/src/commands.rs` 里写业务逻辑
- 写错的注释。错的注释比没有注释更害人，这条是有过教训的

## 按你在改什么去读

| 在改 | 读 |
|---|---|
| `src-tauri/**` | [.claude/rules/rust.md](.claude/rules/rust.md) —— 子进程纪律、std API 会吃掉已有文件、废纸篓、DTO |
| `src/**/*.svelte` `src/**/*.ts` | [.claude/rules/frontend.md](.claude/rules/frontend.md) —— Svelte 5 runes 的坑、CM6、懒加载、会话恢复、桩 |
| 界面长什么样 | [.claude/rules/ui.md](.claude/rules/ui.md) —— 十二条写死的界面规矩、键位与菜单栏 |
| 架构与性能预算 | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| 踩过的坑的全过程 | [docs/JOURNAL.md](docs/JOURNAL.md) |
| 发版 / CI | [docs/RELEASE.md](docs/RELEASE.md) |

Claude Code 会在读到匹配文件时自动加载 `.claude/rules/*.md`（靠 frontmatter 里的
`paths:`），别的 agent 按上表自己打开。

## 写文档和提交信息

- **写为什么，不写是什么。** 代码说得清「是什么」，注释和文档要回答
  「为什么是这样，以及试过哪条路不行」。
- **坑要写下来，连同它当时长什么样。** 这是 JOURNAL.md 的价值所在。
- 中文正文，技术术语和标识符保持原文。
- 提交信息：标题一句话说清做了什么，正文说清**为什么**和**验证方式**。
