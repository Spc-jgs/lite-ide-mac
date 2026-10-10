# 通用任务运行（issue #48）—— 设计

> 状态：**已拍板**（2026-10-10，Q1–Q7 全按建议）。第 0 步实测进行中，结果写在第 10 节。第 2 节是拍板前的业内调研。
> 其中「shell 怎么起」那一条已经先测了（第 2 节最后一段），它直接定掉了一个方案。

## 0. 一句话

把 `mvn spring-boot:run`、`pnpm dev`、`python main.py` 这类「一跑就是半天」的命令存成项目里的任务，**一个键跑、一个键停**；
输出不进终端，进**日志视图** —— ERROR 标红、过滤、跟随、堆栈帧跳源码都是现成的，日志是这个应用最强的一块。
停的时候**整组进程一起停**，先礼后兵：先让它自己收尾（Spring 的关闭钩子要跑），不走再强杀，停完端口必须是空的。

## 1. 现状

| 有 | 在哪 | 对 #48 意味着什么 |
|---|---|---|
| 日志视图：mmap + 稀疏索引，GB 级秒开；过滤、跟随、级别着色、堆栈帧 ⌘Click 跳源码 | `logengine` + `LogPane.svelte` | **输出写成文件、用它看**，内存和输出多少无关。`LogPane` 只认一个日志句柄，放在底部面板里也能用 |
| 跟随一个还在长的文件，被截断 / 轮转也认得 | `logengine::LogFile::refresh` | 写日志的一方可以按大小轮转，看的一方不用改 |
| 日志视图**不认 ANSI 颜色码** | —— | 输出里带 `\e[32m` 会原样显示成乱码 → 第 3 节 Q3 |
| 起子进程、放进自己的进程组、取消时 `killpg` 一锅端 | `gitsvc/src/remote.rs`（`git fetch` 的孙进程那次教训） | 停任务照抄这套，再加「先软后硬」 |
| 真终端（pty） | `ptysvc` | 任务**不**进终端（理由见 Q3），但终端的「登录 shell」那套起法要借 |
| 设置文件 `settings.json` | `settingsctl` | #44 当时说「工具路径挪到 #48，经过登录 shell 跑」—— 下一段实测证明这条路是对的 |
| 底部工具窗：终端 / Git 两个，导轨切换，「有内容才出现」 | `Panel.svelte`、ui.md 第十条 | 「运行」做成第三个工具窗，每个任务一个标签（IDEA 的 Run 工具窗同一个层级） |

**从 Finder 启动没有 shell 的 PATH —— 已实测（2026-10-10）**。模拟 Finder 给的空环境（`env -i HOME=…`）：

| 起法 | 找得到 `mvn`？ | 找得到 `pnpm`？ |
|---|---|---|
| `zsh -lc '…'`（登录、非交互） | 否 | 否 |
| `zsh -ilc '…'`（登录 + 交互） | 是 | 是 |

原因：这台机器的 PATH 是在 `~/.zshrc` 里设的（fnm、pyenv、maven），而 **zsh 只在交互式时读 `.zshrc`**。
所以任务必须经 `$SHELL -ilc '<命令>'` 跑 —— 这也是 VS Code / JetBrains「解析 shell 环境」的做法（见第 2 节）。

## 2. 业内怎么做、痛在哪（2026-10-10 调研）

| | 任务存在哪 | 输出去哪 | 停 | 被骂最多的 |
|---|---|---|---|---|
| **Sublime** Build Systems | `.sublime-build`（全局或项目） | 输出面板，管道 | Cancel Build | **输出要等进程结束才出来**：不接终端时 C 库 / Python 改成块缓冲，官方示例自己都得写 `python -u`（[文档](https://docs.sublimetext.io/guide/usage/build-systems.html)、[论坛](https://forum.sublimetext.com/t/build-system-results-stop-receiving-messages-during-run/27088)） |
| **VS Code** tasks | `.vscode/tasks.json`（字段很多：problemMatcher、presentation、isBackground…） | 集成终端（pty） | Terminate Task | **只停最上面那个进程**，`npm run` 底下的 node 服务还活着、端口还占着（[Cursor 论坛](https://forum.cursor.com/t/killing-terminal-doesnt-kill-the-task-with-it/95799)、[IDX](https://community.idx.dev/t/i-accidentally-created-an-immortal-ghost-task-in-the-terminal-and-i-cant-kill-it/1945)）；**解析 shell 环境慢 / 超时**，元凶是 `.zshrc` 里的 nvm、oh-my-zsh（[JetBrains 说明](https://intellij-support.jetbrains.com/hc/en-us/articles/15268184143890)、[Cursor 论坛](https://forum.cursor.com/t/unable-to-resolve-your-shell-environment-in-a-reasonable-time-please-review-your-shell-configuration-and-restart/161024)） |
| **IntelliJ** Run Configurations | `.idea/` 下，图形界面配 | Run 工具窗，每个配置一个标签，管道 + 它自己渲染 ANSI | **第一下软停**（Unix 上 SIGINT，让进程收尾），按钮变成「强制结束」，**第二下 SIGKILL**（[文档](https://www.jetbrains.com/help/idea/2024.2/interactive-groovy-console.html)，写在 Groovy 控制台那页；Run 工具窗的停止按钮是不是同一套，没找到原文） | **Port 8080 already in use**：上一次没停干净 / IDE 崩了 JVM 还活着，人只能自己 `lsof -i :8080` 再 `kill`；论坛里有人专门要「IDE 帮我找出占端口的进程」（[JetBrains 社区](https://intellij-support.jetbrains.com/hc/en-us/community/posts/360007755579/comments/360001517720)、[常见排查](https://oneuptime.com/blog/post/2025-12-22-port-already-in-use-spring-boot/markdown)） |
| **Zed** tasks | `.zed/tasks.json`，字段少：label、command、cwd、env | 集成终端 | —— | 设计上可取的是 **rerun 上一个**（`task: rerun`）和「同一个任务默认不并发跑两份」（[文档](https://zed.dev/docs/tasks.html)） |

**抄什么、避开什么：**

- 抄 Zed 的**少字段**（名字、命令、目录、环境变量），不抄 VS Code 那一套 problemMatcher —— 我们有日志视图，「哪行是错误」它本来就会标。
- 抄 IDEA 的**层级**（底部一个「运行」工具窗、每个任务一个标签）和**先软后硬**的停。
- 三个痛点逐个有解：块缓冲（Q3）、只停最上面那个进程（Q5，杀整个进程组）、端口被占不知道是谁（Q6，把占的人摆出来）。
- 「解析 shell 环境慢」我们绕不开（PATH 就在 `.zshrc` 里），但能做到**不在启动时付**：只在跑任务的那一刻起 shell，慢也只慢那一次，界面不等它。

## 3. 要你定的七个问题（每个都带建议）

### Q1 键位：照 IDEA，不用 issue 里写的 ⌘B / ⇧⌘B

| 动作 | 建议 | 理由 |
|---|---|---|
| 再跑一次上一个任务 | **⌃R** | IDEA macOS 键位的 Run。⌘B 是「跳到声明」（IDEA 同键，天天按），不能让 |
| 选一个任务跑 | **⌃⌥R** | IDEA 的 Run…，弹任务列表 |
| 停止 | **⌘F2** | IDEA 的 Stop；第一下软停，正在软停时再按一下强杀（同 IDEA 按钮变「强制结束」） |

**⌃R 有个坑**：在终端里 ⌃R 是 shell 的「搜历史」，天天用。如果 ⌃R 挂进菜单（`owner: menu`），AppKit 会先把键吃掉，
终端里就再也搜不了历史 —— keymap.ts 文件头那张表说的正是这个。所以 ⌃R **归 `key`**：页面里接，焦点在终端里时不接、让给 shell。
菜单里照样有「运行上一个任务」这一项（只是不带快捷键显示，同 ⌘P 那条的做法）。⌃⌥R、⌘F2 在终端里没有常用含义，可以进菜单。

### Q2 任务从哪来：项目里一个文件 + 自动认出 package.json 的脚本

- **`.lite-ide/tasks.json`**（JSONC，可以写注释和尾逗号，和 `settings.json` 一个解析器）：

  ```jsonc
  [
    { "name": "后端", "command": "mvn spring-boot:run", "cwd": "admin" },
    { "name": "前端", "command": "pnpm dev", "cwd": "web" },
    { "name": "脚本", "command": "python main.py", "env": { "APP_ENV": "dev" } }
  ]
  ```

  四个字段：`name`、`command`（一整行，交给 shell）、`cwd`（相对项目根，默认根）、`env`。**不进全局配置**：任务是项目的事，跟着仓库走、能提交给同事。
- **自动认出的**：项目根和一层子目录里的 `package.json` 的 `scripts`，按锁文件猜包管理器（`pnpm-lock.yaml` → pnpm、`yarn.lock` → yarn，否则 npm）。
  零配置就能跑 `pnpm dev`；文件里同名的覆盖自动认出的。
- **不自动认 Maven / Gradle / Python**：`spring-boot:run` 要不要加 profile、Python 跑哪个入口，猜错比不猜糟。
  第一次 ⌃⌥R 列表里没有它们时，列表底部给一项「新建 tasks.json」，开出一份带这三行示例（注释掉）的模板。

### Q3 输出怎么接：管道，不用 pty —— **第 0 步要实测确认**

| | 管道（建议） | pty |
|---|---|---|
| 颜色码 | 多数工具发现不是终端就不输出颜色（Spring Boot、Maven、vite 据我所知都这么判，**第 0 步实测**） → 日志里干净 | 全是 `\e[…m`，日志视图要先学会渲染 ANSI |
| 进度条 `\r` | 多数工具不是终端就不画 | 一行里几百个 `\r`，日志里一行巨长 |
| 缓冲（Sublime 的头号痛点） | **会块缓冲** → 给 `PYTHONUNBUFFERED=1`（PyCharm 也是这么做的）；C 程序要自己 flush，这一条写进文档 | 天然行缓冲 |
| 和「在终端里跑」的行为差多少 | 有差：没有 TTY 的工具会少几句交互提示（vite 的「按 h 看帮助」） | 一样 |

建议管道 + `PYTHONUNBUFFERED=1`，写日志时**顺手剥掉** ANSI（防 `FORCE_COLOR` 一类强开颜色的）。日志视图的级别着色替代颜色。
颜色真想要，以后让日志视图认 ANSI，那是另一件事。**第 0 步拿三个真工具各跑一次，管道 / pty 各一遍**，看输出到底长什么样再定。

### Q4 shell 怎么起：`$SHELL -ilc '<命令>'`，每次跑任务时现起

第 1 节的实测已经定了 `-i`。剩下的问题是 **`-i` 没有 TTY 时 zsh 会不会出幺蛾子**（rc 里的 `stty`、p10k 的 instant prompt、
「can't change option: zle」这类警告）—— 第 0 步拿你的真 `.zshrc` 跑一遍。出问题的退路：先用一次性的 pty 跑 `$SHELL -ilc env` 把环境取出来
（VS Code 的做法），缓存住，任务本身用这份环境、不再经 rc 起。用哪个 shell 跟设置里的 `terminal.shell` 走（空 = `$SHELL`）。

### Q5 停：整组进程，先 SIGINT，5 秒后 SIGKILL；再按一次立刻 SIGKILL

- 任务起的时候放进**自己的进程组**（`process_group(0)`），停的时候对整组发信号 —— 解决 VS Code 那个「只停了 `npm`，node 还占着端口」。
- 第一下发 **SIGINT**，也就是终端里按 ⌃C 的效果：Spring Boot 收到会跑关闭钩子；Gradle 的客户端收到会通知守护进程取消（daemon 不在这个组里，
  只有「客户端转告」这一条路能停它 —— 这也是不直接 SIGTERM / SIGKILL 的理由）。**这台机器没装 gradle，这一条没法实测**，验收里如实标「未验」。
- 5 秒没退干净 → SIGKILL 整组。等的这 5 秒里，「停止」按钮变成「强制结束」，按了立刻 SIGKILL（IDEA 同款）。
- 停完要验的不是「主进程退了」，而是**组里一个都不剩、端口空了**（issue 的验收 2）。

### Q6 端口被占：任务失败时把「谁占着」摆出来，结束它要再确认

任务异常退出时扫最后几百行，认几种常见说法（Spring 的 `Port 8080 was already in use`、Node 的 `EADDRINUSE`、Python 的 `Address already in use`），
抠出端口，`lsof -nP -iTCP:<端口> -sTCP:LISTEN` 查出是谁，在运行窗口顶上出一张卡片：

> 8080 被 **java（PID 41237）** 占着 —— 是你上次跑的「后端」，没停干净 ｜ 结束它并重跑 ｜ 取消

能认出「是我们自己起过的」（PID 在我们记过的进程组里）就直说；认不出就只给命令行和 PID。**结束别人的进程是不可逆的**，卡片带 danger、要点一下才做（ui.md 第十三条第三档：把挡路的东西摆出来，给现成的出路）。

### Q7 输出存哪、存多久：应用数据目录，每个任务一份，1GB 轮转

- `<应用数据>/runs/<项目哈希>/<任务名>.log`，每次跑覆盖上一次（上一次的挪成 `.1`，只留一份）。
- 一个开着好几天的 dev server 能吐出几个 GB：**单个文件到 1GB 就轮转**（日志视图本来就认轮转，smoke ⑱）。
- 不放项目目录：不想往别人的仓库里写东西，也不想让 `git status` 多出一堆文件。

## 4. 界面

**底部第三个工具窗「运行」**（照 IDEA 的 Run 工具窗；ui.md 第十条「有内容才出现」—— 跑过任务才出现在导轨上）。
每个任务一个标签，标签上一个状态点（跑着 / 正常退出 / 失败），内容就是 `LogPane`（过滤条、跟随、级别着色全带着）。
头里的动作：重跑、停止 / 强制结束、在编辑区打开这份日志（大屏看）。

**状态栏的进度格不画它**：进度格管的是「会结束的后台活」（拉取、索引、提交钩子），一个 dev server 会一直跑，放进去就是一根永远转的条。
跑着的任务数写在导轨「运行」按钮的角标上。

**⌃⌥R 的任务列表**：照 ⇧⇧ 的浮层骨架（ui.md 第五节）：输入 → 分组（tasks.json / package.json）→ 脚栏键位。正在跑的那几个带状态点，↵ 是「重跑」。

## 5. 归属与收尾（多窗口）

- 任务属于**起它的那个窗口**（同终端，MULTIWINDOW.md）：关窗口停它的任务，退出应用停全部。`release_window` 里加一段，`state.rs` 那条「关掉一个窗口只收它自己的东西」加一组断言。
- **应用崩了怎么办**：Rust 侧是 `panic = abort`，进程当场死，进程组还活着 —— 这就是 IDEA 论坛里「IDE 崩了 JVM 还占着 8080」。
  解法：起任务时把 `(进程组号, 命令行, 起的时间)` 记进 `<应用数据>/runs/live.json`，正常停掉就删；**下次启动时发现还有活着的**（组号还在、命令行对得上），
  出一张卡片「上次有 2 个任务没停干净 ｜ 结束它们 ｜ 留着」。同 #42 替换日志的「上次中断」那张卡片一个形状。

## 6. 分步落地（拍板后）

| 步 | 做什么 | 怎么验 |
|---|---|---|
| 0 | 实测：三个真工具 × 管道 / pty 的输出；`-ilc` 在你真 `.zshrc` 下有没有警告；`mvn spring-boot:run` / `pnpm dev` / `python` 的进程树，SIGINT 之后谁退了、端口什么时候空 | 结果写回本文 |
| 1 | `crates/tasksvc`：起（shell、进程组、合并 stdout/stderr 写文件、剥 ANSI、轮转）、停（软 → 硬）、退出码、`live.json` | 单测：孙进程一起停、端口空了；软停不理 SIGINT 的进程 5 秒后被强杀；输出无上限时文件按 1GB 轮转（测试里上限可注入） |
| 2 | 任务从哪来：`tasks.json` 解析 + package.json 认脚本；命令、DTO、桩 | 纯函数测试 + dto_sync |
| 3 | 界面：运行工具窗 + LogPane、⌃R / ⌃⌥R / ⌘F2、任务列表；keymap / 菜单 | `pnpm dev` 桩上走一遍；状态层测试 |
| 4 | 端口被占的卡片、上次没停干净的卡片 | 测试通道验收：故意先占 8080 |
| 5 | smoke 一段 + `scripts/accept/tasks.sh`（issue 的三条验收），文档 | 真 .app |

入口包预计 +1 KB 以内：工具窗、任务列表、卡片全部懒加载，入口里只有 keymap 三行和「有没有在跑的任务」一个计数。

## 7. 验收（issue #48 那三条，加四条）

1. `mvn spring-boot:run` 跑起来，日志视图里 ERROR 标红、能过滤
2. 停止后 8080 释放，**没有残留 `java` 进程**（查进程组，不只查主进程）
3. `pnpm dev`、`python main.py` 同样能跑；`python` 的 `print` **实时**出来（不是结束才出来）
4. 从 Finder 启动的 lite-ide 里跑，能找到 `mvn` / `pnpm`（不靠从终端启动）
5. 软停不理的进程（`trap '' INT; sleep 999`）5 秒后被强杀；软停期间再按一次立刻强杀
6. 8080 先被占着时跑「后端」：卡片说出是谁占的；点「结束它并重跑」能跑起来
7. 跑着任务时让应用崩掉（测试构建里 `kill -9` 自己），重开 → 「上次没停干净」的卡片，点「结束」端口空了
- Gradle `bootRun` 停得干净 —— **本机没装 gradle，未验**，留给有 gradle 的人

## 8. 这一轮明确不做

- **调试**（断点、单步）：那是 LSP / DAP 的范围，立项时排除了。
- **problemMatcher / 把编译错误标进编辑器**：日志视图已经标级别、堆栈帧能跳，够用；真要做是下一件事。
- **任务之间的依赖、组合任务**（「先 build 再 run」）：命令里写 `&&` 就行。
- **全局任务**：任务属于项目（Q2）。
- **往任务里输入**（stdin）：任务的 stdin 接 `/dev/null`；要交互的命令请在终端里跑。

## 9. 我对这份稿子的把握

有把握的：键位、层级、杀进程组、端口卡片、崩溃后收尸 —— 都有业内的先例或者我们自己踩过的坑（`git fetch` 的孙进程、#42 的中断恢复）垫着。

**没把握、要第 0 步说话的三处：**

1. **管道下三个真工具的输出到底干不干净**（Q3）。如果 Spring Boot 或 vite 在管道下仍然大量输出颜色或进度条，剥 ANSI 够不够、要不要改用 pty + 日志视图认 ANSI，那是不同的工作量。
2. **`-ilc` 没有 TTY 时你的 `.zshrc` 会不会报错或卡住**（Q4）。卡住的话退路是「pty 里取一次环境再缓存」，多一步。
3. **SIGINT 对 `mvn spring-boot:run` 管不管用**：`spring-boot:run` 默认 fork 一个 JVM，SIGINT 发给整组，它们都会收到 —— 但 Maven 自己收到 SIGINT 时会不会抢在子 JVM 跑完关闭钩子之前把它强杀，要实测。
