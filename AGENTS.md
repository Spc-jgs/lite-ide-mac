# AGENTS.md

macOS 个人工作台。Tauri 2 + Svelte 5 + CodeMirror 6，日志引擎自研（mmap + 稀疏索引）。

## 命令

```bash
pnpm dev            # 浏览器 + IPC 桩（src/lib/dev/mock-ipc.ts + mock/）。改 UI 用这个，热更新毫秒级
pnpm app            # Tauri 开发模式
pnpm app:build      # 只编可执行文件
pnpm app:bundle     # 打包 .app + .dmg，产物在 src-tauri/target/release/bundle/。先跑 scripts/java-bundle.sh（内置的 jdtls + 裁过的 Java 21，没变就跳过）
pnpm check          # svelte-check
pnpm test           # 前端测试：纯函数 + 状态层（*.state.test.ts，runes 在裸 node 里跑，见 tests/runes/）
cd src-tauri && cargo test --workspace
cd src-tauri && cargo clippy --workspace --features lite-ide/test-bridge -- -A clippy::all -D clippy::disallowed_methods   # 规矩检查，CI 也跑
scripts/build-test-app.sh      # 带测试通道的临时身份 .app（验收用，碰不到真实数据；正式包永远不带）。放在 target/test-app.noindex/，Spotlight 不收录
scripts/ls-clean.sh            # 注销系统登记里已经不在盘上的 lite-ide（打 .dmg 挂载的临时卷每次留一条）；app:bundle 末尾自动跑
python3 design/icon/make_icon.py # 重新生成应用图标（Assets.car + icon.icns + 几张 PNG，要 Xcode 26+）。改图标只改这个脚本，产物都提交
scripts/accept/settings.sh     # 走测试通道的真 .app 验收样板：不发按键、不读 AX、不截图、不抢焦点。设置对话框（写回 settings.json 注释还在、恢复默认、写坏了不动刀、真模态、此项目）；键位可改（22 条：录键时摘掉菜单的键、keymap.json 只存改过的、菜单和两个窗口里开着的编辑器当场换、手改文件不用重启）
scripts/accept/tasks.sh        # 任务运行的机制（39 条，含日志跟随时往上翻不被拽回去）；quit.sh 是 Dock 右键退出那条路（6 条）
scripts/accept/tasks-real.sh   # 你机器上的真 mvn / pnpm / python（缺哪样哪段记「未验」）
scripts/accept/merge.sh        # 几份日志按时间合并（15 条）：顺序、堆栈不拆散、过滤、偏移、跨块、会话恢复
scripts/accept/patterns.sh     # 日志归类（8 条）：次数对得上、点一类筛出来正好那么多、和昨天比、合并视图、关标签收句柄
scripts/accept/spring.sh       # 认出 Spring Boot 并照 IDEA 那样起 + Services 工具窗 + Maven 工具窗 + 跑单测 + 断点调试 + 构建 + 热替换 + 堆用量 + 运行配置（109 条，真 mvn / java / jdtls / java-debug / JaCoCo）：兄弟模块不用 install、web 服务停干净；Services 里分组、端口、⌘F2 / 双击 / 工具栏起停；工具窗的嵌套 / Profiles / 9 个阶段 / 真插件和依赖、勾的 profile 和跳过测试带进命令、双击 package 出 jar；单测的 ▶ / ⌃⇧R、结果树、行号旁 ✓✗、重跑没过的、边跑边出（一个类跑完亮一个、上一次的报告不混进来）、带覆盖率跑（代理挂在 surefire fork 的 JVM 上、兄弟模块的文件也列、编辑器绿红、「清掉」收颜色）；调试停在兄弟模块的断点上、变量带 toString、求值、F8 单步、F9 继续之后照常跑完；条件断点为假不停、为真照停、写错了照样停并说清为什么；日志断点不停、那句打进运行窗（插在行首）、命中次数没到不停、异常断点停在被接住的那个异常上并说出抛的是什么；运行到光标、行内变量值、监视；调试单测（⌃⇧D）停在测试里、求值用得到兄弟模块、跑完结果树照常出来；⌘F9 构建：编译错误按文件列出、同一条只列一次；pom 要 21、默认是 17 时用 21 编；Services 选中的服务、工具栏上的堆三个数对得上；热替换：停在断点上改了 ⌘S 就自己构建、换进去（设置里关掉就不自动）、停着那一帧跑完旧的、下一次调用是新的、断点照停，行号变了的断点这次不停、说清楚，加了方法的一个都不换；运行配置的 profile / VM 参数 / 环境变量真到了 JVM，从 IDEA 的 workspace.xml 导入的也是、自己建的应用配置也是、IDEA 的文件一个字节不动；项目有 mvnw 用 mvnw；远程 JVM 调试：挂上别人起的 JVM、停在断点上、⌘F2 断开之后那个 JVM 照常在跑、对面退了这边自己结束
scripts/accept/mybatis.sh      # MyBatis 接口 ↔ XML 跳转（7 条，读文本、不要 mvn / jdtls）：图标画在哪几行（注释、MyBatis-Plus 那种方法、匿名类不算）、进 src 的 XML 不进 target 的、回 main 的接口不回 test 的、没保存的也算
scripts/accept/spring-nav.sh   # Spring 接口搜索和配置跳转（8 条，读文本）：「接口」范围列全、target 里的不算、常量照原名；粘真 URL 对上 {id}、写死的排在 {id} 前面；@Value 上 ⌘B 宽松绑定、三份配置都列、一处直接跳、没有说一句
scripts/accept/http.sh         # `.http` 发请求（17 条，真 curl + 脚本自己的回显服务）：没选环境不发、私有环境盖公开的、头和体逐字对、按原样次序排 JSON、< 文件、重定向、4xx 标红、3 MB、取消后 curl 没了、`< 项目外面的文件` 不发、不往项目里写
scripts/accept/history.sh      # 本地历史（9 条）：GBK + CRLF 存的是原始字节、读回来解得对；外面改过补记一版；面板里恢复是编辑器里的一笔修改（不写盘）；不往项目里写
scripts/accept/java.sh         # Java 智能（86 条，内置的 jdtls + Java 21）：跑的是 .app 里那份、Lombok 不报假错、实时报错、补全（自动 import、方法带括号）、参数提示（换参数、出了括号收起、⌘P 手动弹）、改名（文件跟着改名、jdtls 从文件监听知道、撤销、撞名不动）、格式化（照编辑器的缩进、一笔撤销、只排选中的）、快速修复（import 候选的次序、补上后波浪线消失）、⌘B 进兄弟模块 / jar / JDK 源码、⌥F7、⌥⌘B 跳到实现、⌘U 父方法（进 JDK 的 Object）、F1 快速文档（Javadoc 的 HTML 不当 HTML 画）、同名高亮（读写分色、同名的别的变量不标）、参数名提示（只给字面量、不进正文、设置里关掉当场收）、生成 ⌃↵（Getter 和 Setter 里 final 的没有 setter、toString、构造器、equals / hashCode、覆盖方法默认不勾）、调用 / 类型层级（调用方带调用那一行、↵ 跳到调用处、接口上 ⌃H 列实现、⌃H 不再删字）、⇧⇧ 找类（驼峰缩写、默认只找项目的、含 jar 里的、停在声明那行、p95 延迟）、代码模板（sout + Tab 展开、jdtls 的 .var 整行换掉、补上的 .sout）、代码风格照 IDEA（项目的 .idea/codeStyles：⌃⌥O 合成 .*、static 排最后，⌥⌘L 照行宽折行、续行缩进）、重构（⌥⌘V 几种做法挑、抽完接着改名、抽出来是空格不是 Tab；⌥⌘M / ⌥⌘C 同样；⌥⌘N 内联回去）、移动类（F6：列的包、先预览盘上不动、挪进去之后 import 跟着改、jdtls 不报错、撤销挪回来）、不往项目里写、退出不留。JDTLS_HOME=… 验「用自己那份」
scripts/smoke.sh               # 发版前的端到端清单（141 条，约 100 秒），同样走测试通道，先打测试 .app
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
macOS 的 sed 不认 `\|`（`sed '/a\|b/d'` 什么都没删，跑的还是原代码）；
macOS 自带的 bash 3.2 在双引号里的 `$(…)` 里照样做花括号展开（`"$(f "{ a: 1, b: 2 }")"` 拆成两段）、`$(case … in x) …)` 的 `)` 会提前结束 `$(`（2026-10-20 一天撞两次，验收报的「没回话」「不成立」全是脚本自己）；验收脚本接 `| head` 会被截断、走不到退出
（测试应用留着，下一轮 `lite_launch` 现在会先收掉它，见 bridge.sh）。改坏用 python 改、打印改了几处；命令写全，不存进变量。**改回来先拷一份备份、再拷回来**，别用 `git checkout` 还原一个还没提交的文件 —— 这一轮对它的改动会一起没（2026-10-23 撞过）。

**写进注释的依赖行为，读过它的源码再写。** 废纸篓的注释写了很久「走 `NSFileManager`」，`trash` crate 默认其实是让 Finder 去删
（2026-10-12 读源码才发现）—— 照着错的注释去查「为什么弹了控制 Finder 的权限框」，会找不到原因。

## 边界

**一律要做：**

- 起子进程走 `crates/procutil`（`spawn` / `run_capped`）：stdin 接空、两个管道并发读、输出设闸、丢掉就收尸。
  先问「它的输出有上限吗」—— 不走它的只有任务（`tasksvc`）、git fetch / push、语言服务器（`lspsvc`），理由在 rust.md。
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

- 「在应用里跑别人代码」的插件系统、插件市场、Windows 支持、遥测 —— 立项时就明确排除了，
  理由在 [PLAN.md](PLAN.md) 和 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。2026-10-22 改了两处（[docs/TAKEOVER.md](docs/TAKEOVER.md) 第 9 节 D8）：
  **自动更新器要做**（不打断、不带遥测）；插件只做**声明式集成包**（语言服务器、构建工具、运行配置类型用 JSON 声明，不在应用里跑代码）
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
| 设置、偏好、`settings.json`、设置对话框、运行配置 | [docs/SETTINGS.md](docs/SETTINGS.md) —— 设置页和你都能写 `settings.json`（只改那一个值的字符，第 13 节），按钮改的另存一份；按项目的设置存在应用目录、不往项目里写；加设置项先改 `settings.rs` 的 `DEFS`（连同 `label`），别的都从它来 |
| 任务运行（⌃R / ⌃⌥R / ⌘F2） | [docs/TASKS.md](docs/TASKS.md) —— 登录 shell、管道不用 pty、整组先软后硬地停、端口卡片、崩溃后收尸 |
| 跨文件替换 | [docs/REPLACE.md](docs/REPLACE.md) —— 两段提交、撤销日志、命中太多不许执行 |
| 几份日志按时间合并 | [docs/MERGE.md](docs/MERGE.md) —— k 路归并、一行一字节的合并索引、没时间戳的行跟着上一行走、借日期要说出来 |
| 日志归类 | [docs/PATTERNS.md](docs/PATTERNS.md) —— Drain 改了两处（按段换变量、相似度只数常量词）、只归记录头、点一类 = 常量词当过滤条件、对比时两边放进同一棵树 |
| 接住 IDEA 的 Java 后端日常（语言服务器、Maven、Spring Boot、调试） | [docs/IDEA.md](docs/IDEA.md) —— 本心的硬约束（秒开不等、独立进程可关、不打断、正式包不变大），每一步都要过它的审；第 7 节 jdtls 实测、第 9 节语言服务器怎么接的 |
| 下一阶段的总图：取代 IDEA 还缺什么、分几期做 | [docs/TAKEOVER.md](docs/TAKEOVER.md) —— 四方对照（IDEA / VS Code / Sublime / Lithe）、32 个缺口按多久碰一次排、15 条交互、设置树、四处大修、MVP-0 到 MVP-8 |
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
