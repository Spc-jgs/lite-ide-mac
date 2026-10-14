# AGENTS.md

macOS 个人工作台。Tauri 2 + Svelte 5 + CodeMirror 6，日志引擎自研（mmap + 稀疏索引）。

## 命令

```bash
pnpm dev            # 浏览器 + IPC 桩（src/lib/dev/mock-ipc.ts + mock/）。改 UI 用这个，热更新毫秒级
pnpm app            # Tauri 开发模式
pnpm app:build      # 只编可执行文件
pnpm app:bundle     # 打包 .app + .dmg，产物在 src-tauri/target/release/bundle/
pnpm check          # svelte-check
pnpm test           # 前端测试：纯函数 + 状态层（*.state.test.ts，runes 在裸 node 里跑，见 tests/runes/）
cd src-tauri && cargo test --workspace
cd src-tauri && cargo clippy --workspace --features lite-ide/test-bridge -- -A clippy::all -D clippy::disallowed_methods   # 规矩检查，CI 也跑
scripts/build-test-app.sh      # 带测试通道的临时身份 .app（验收用，碰不到真实数据；正式包永远不带）。放在 target/test-app.noindex/，Spotlight 不收录
scripts/ls-clean.sh            # 注销系统登记里已经不在盘上的 lite-ide（打 .dmg 挂载的临时卷每次留一条）；app:bundle 末尾自动跑
python3 design/icon/make_icon.py # 重新生成应用图标（Assets.car + icon.icns + 几张 PNG，要 Xcode 26+）。改图标只改这个脚本，产物都提交
scripts/accept/settings.sh     # 走测试通道的真 .app 验收样板：不发按键、不读 AX、不截图、不抢焦点
scripts/accept/tasks.sh        # 任务运行的机制（36 条）；quit.sh 是 Dock 右键退出那条路（6 条）
scripts/accept/tasks-real.sh   # 你机器上的真 mvn / pnpm / python（缺哪样哪段记「未验」）
scripts/accept/merge.sh        # 几份日志按时间合并（15 条）：顺序、堆栈不拆散、过滤、偏移、跨块、会话恢复
scripts/accept/patterns.sh     # 日志归类（8 条）：次数对得上、点一类筛出来正好那么多、和昨天比、合并视图、关标签收句柄
scripts/accept/spring.sh       # 认出 Spring Boot 并照 IDEA 那样起（8 条，真 mvn / java）：兄弟模块不用 install、web 服务停干净
scripts/smoke.sh               # 发版前的端到端清单（97 条，约 100 秒），同样走测试通道，先打测试 .app
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
**系统的登记里也只留一份**（2026-10-13，用户问「app 的名你是不是改了」）：测试身份的 `lite-ide-mwtest` 原来挨着正式包放、被 Spotlight 收录，
每次注销又被登记回来；打 `.dmg` 挂的临时卷每次留一条 `/Volumes/dmg.*/lite-ide.app`，攒了 114 条。测试包挪进 `*.noindex`、`app:bundle` 末尾跑 `ls-clean.sh`。

**加完测试要把被测代码改回错误的样子跑一遍，确认它真的红。**
不会失败的测试比没有测试更糟 —— 它给一个假的安全感。
（`检出远程分支要建跟踪分支`、`截断的路径列表要丢掉末尾那条半截的` 都是这么验过的。）
**改回来之后要确认真的重编了**：`sed -i.bak` 改坏 → 跑红 → `mv` 备份回来，第二次照样红 ——
`mv` 保留的是备份的旧 mtime，比刚编出来的产物还老，cargo 认为没变，跑的还是改坏那版。
`touch` 一下再跑（2026-09-16 `分支删除和改名` 踩的）。
**改坏的那一版要还原一个真会写出来的错**，不是随手改坏一处：2026-10-12 验 procutil 时第一次造的错版本把数据吞了，
测试是因为丢了数据才红、不是因为卡住 —— 它什么也没证明。
**错版本「没输出」不等于没红，先确认它真的跑了、真的改坏了**（2026-10-12 / 13 各栽一次，三种形状）：
zsh 不按空格拆变量（`T="node a.ts"; $T` 是去找一个叫 `node a.ts` 的文件，报错被 `grep ✗` 滤掉）；
macOS 的 sed 不认 `\|`（`sed '/a\|b/d'` 什么都没删，跑的还是原代码）；验收脚本接 `| head` 会被截断、走不到退出
（测试应用留着，下一轮 `lite_launch` 现在会先收掉它，见 bridge.sh）。改坏用 python 改、打印改了几处；命令写全，不存进变量。

**写进注释的依赖行为，读过它的源码再写。** 废纸篓的注释写了很久「走 `NSFileManager`」，`trash` crate 默认其实是让 Finder 去删
（2026-10-12 读源码才发现）—— 照着错的注释去查「为什么弹了控制 Finder 的权限框」，会找不到原因。

## 边界

**一律要做：**

- 起子进程走 `crates/procutil`（`spawn` / `run_capped`）：stdin 接空、两个管道并发读、输出设闸、丢掉就收尸。
  先问「它的输出有上限吗」—— 不走它的只有任务（`tasksvc`）和 git fetch / push，理由在 rust.md。
  **clippy 卡着**（`src-tauri/clippy.toml` 的 `disallowed-methods`，连同会吞文件的 `File::create` / `fs::write` / `fs::rename` 和直接删文件）：
  有意要用的标 `#[allow(clippy::disallowed_methods)]`、同一行写为什么
- 碰盘、碰子进程、碰网络的命令写成 `async` + `commands::blocking`；真要留在主线程的写进 `src-tauri/tests/main_thread.rs` 的白名单、写清理由
- 按窗口登记的资源表用 `Owned<K, V>`（`src-tauri/src/owned.rs`），`release_window` 里收 —— `state.rs` 那条读源码的测试会卡住漏收
- 改了过 IPC 的 DTO，两侧一起改 —— `src-tauri/tests/dto_sync.rs` 会卡住漂移
- 改了键位或菜单，先改 `src/lib/state/keymap.ts` —— `menu_sync.rs` 会卡住漂移
- 改了 Rust 侧 DTO / 命令，同步改桩 `src/lib/dev/mock/<领域>.ts`（和 `src-tauri/src/commands/<领域>.rs` 一一对应）。桩一分叉就开始骗人
- 页面里的文字走 Svelte 的文本插值，不直接插 HTML —— `tests/no-html-sinks.test.ts` 卡着（一旦能跑别人的脚本就等于拿到这台机器）
- 文本框里处理回车 / Esc / 方向键，先问 `imeKey(e)` —— `tests/ime.test.ts` 卡着（拼音确认候选字的那一下回车也是 keydown：
  任务列表会把选中的跑起来、替换框会把半截拼音替换进正文）
- 删用户的文件只走废纸篓（`fsservice::move_to_trash` / `move_all_to_trash`）。应用自己生成的东西才直接删，范围卡死在自己的那几种文件名上
  （rust.md「删除只走废纸篓」；clippy 卡着 —— 第一次跑就抓到一处直接删未跟踪文件的）
- 截断了要说出来、给真数字：一份看着完整、其实少了一截的东西，比一句「显示不下」危险（rust.md「子进程输出必须设闸」；以后拼给模型的同理）
- 改完给出数字。「快了很多」没有信息量，「1112ms → 0.008ms」有
- 改了代码的那一轮，交付前 `pnpm app:bundle`：人是双击 `.app` 验收的。只改文档的轮次打出来的 `.app` 和上一轮一样，不用打

**先问再做：**

- 加依赖（尤其是会进入口包的前端依赖 —— CI 卡 160 KB 红线，152 告警；同仓库里的新 crate 不算）
- 改 `tauri.conf.json` 的 `bundle.identifier`、CSP、窗口透明相关的任何一项
- 提交、推送、建分支
- 下载东西（工具链、Gradle 发行包、图标）—— 说清是什么、从哪来、多大
- 往 GitHub 上发 issue / 评论：**仓库是公开的**，不带个人路径和用户名
- 会占用前台、打断正在用电脑的人的操作（真按键、拉起别的应用做实验）

**不做：**

- 插件系统、Windows 支持、遥测、自动更新器 —— 立项时就明确排除了，
  理由在 [PLAN.md](PLAN.md) 和 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- 拿 `cargo build --release` 当验证
- 在 `src-tauri/src/commands/` 里写业务逻辑
- 写错的注释。错的注释比没有注释更害人，这条是有过教训的
- 在下载来的参考代码里跑构建、脚本、测试：只读。构建脚本和测试都能执行任意命令，读外来代码只用「读」这一种操作
- 验收碰真实数据、发全局按键：一律走测试身份 `.app` 和测试通道（`scripts/lib/bridge.sh`）——
  System Events 的按键落在最前面的应用上，改过用户正在用的应用的缩放；smoke 往用户的 zsh 历史里写过十行

## 按你在改什么去读

| 在改 | 读 |
|---|---|
| `src-tauri/**` | [.claude/rules/rust.md](.claude/rules/rust.md) —— 子进程纪律、std API 会吃掉已有文件、废纸篓、DTO |
| `src/**/*.svelte` `src/**/*.ts` | [.claude/rules/frontend.md](.claude/rules/frontend.md) —— Svelte 5 runes 的坑、CM6、懒加载、会话恢复、桩 |
| 界面长什么样 | [.claude/rules/ui.md](.claude/rules/ui.md) —— 十四条写死的界面规矩、键位与菜单栏 |
| 架构与性能预算 | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| 窗口、事件发给谁、会话快照、退出 | [docs/MULTIWINDOW.md](docs/MULTIWINDOW.md) —— 一个项目一个窗口；进程级的资源要记 owner，跨窗口的东西只有 Rust 一个主人 |
| 设置、偏好、`settings.json` | [docs/SETTINGS.md](docs/SETTINGS.md) —— 你写的文件应用只读不改，按钮改的另存一份；加设置项先改 `settings.rs` 的 `DEFS`，别的都从它来 |
| 任务运行（⌃R / ⌃⌥R / ⌘F2） | [docs/TASKS.md](docs/TASKS.md) —— 登录 shell、管道不用 pty、整组先软后硬地停、端口卡片、崩溃后收尸 |
| 跨文件替换 | [docs/REPLACE.md](docs/REPLACE.md) —— 两段提交、撤销日志、命中太多不许执行 |
| 几份日志按时间合并 | [docs/MERGE.md](docs/MERGE.md) —— k 路归并、一行一字节的合并索引、没时间戳的行跟着上一行走、借日期要说出来 |
| 日志归类 | [docs/PATTERNS.md](docs/PATTERNS.md) —— Drain 改了两处（按段换变量、相似度只数常量词）、只归记录头、点一类 = 常量词当过滤条件、对比时两边放进同一棵树 |
| 接住 IDEA 的 Java 后端日常（语言服务器、Maven、Spring Boot、调试） | [docs/IDEA.md](docs/IDEA.md) —— 本心的硬约束（秒开不等、独立进程可关、不打断、正式包不变大），每一步都要过它的审 |
| 定位、边界、接下来往哪走 | [docs/DIRECTION.md](docs/DIRECTION.md) —— PLAN 里哪些前提已经变了；加功能前先对一下 D1 那句定位；第 7 节是优先级 |
| 踩过的坑的全过程 | [docs/JOURNAL.md](docs/JOURNAL.md) |
| 想照别的项目的做法改 | [docs/research/codex.md](docs/research/codex.md) —— 读 Codex 的五个主题：对照结论、学了什么、为什么不学 |
| 体验好不好、该先补哪个缺口 | [docs/research/editor-standards.md](docs/research/editor-standards.md) —— 用「编辑器为什么被骂、为什么成名」的七条审过一遍，每条有证据 |
| 发版 / CI | [docs/RELEASE.md](docs/RELEASE.md) |

Claude Code 会在读到匹配文件时自动加载 `.claude/rules/*.md`（靠 frontmatter 里的
`paths:`），别的 agent 按上表自己打开。

## 写文档和提交信息

- **写为什么，不写是什么。** 代码说得清「是什么」，注释和文档要回答
  「为什么是这样，以及试过哪条路不行」。
- **坑要写下来，连同它当时长什么样。** 这是 JOURNAL.md 的价值所在。
- 中文正文，技术术语和标识符保持原文。
- 提交信息：标题一句话说清做了什么，正文说清**为什么**和**验证方式**。
