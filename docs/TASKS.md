# 通用任务运行（issue #48）—— 设计

> 状态：**已拍板、第 0 步已实测**（2026-10-10，Q1–Q7 全按建议；实测三处都支持原建议，见第 10 节）。第 2 节是拍板前的业内调研。
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

**第 0 步实测定了：管道**（第 10 节：三个真工具在管道下颜色码都是 0）。管道 + `PYTHONUNBUFFERED=1`，写日志时**顺手剥掉** ANSI（防 `FORCE_COLOR` 一类强开颜色的）。日志视图的级别着色替代颜色。
颜色真想要，以后让日志视图认 ANSI，那是另一件事。**第 0 步拿三个真工具各跑一次，管道 / pty 各一遍**，看输出到底长什么样再定。

### Q4 shell 怎么起：`$SHELL -ilc '<命令>'`，每次跑任务时现起

第 1 节的实测已经定了 `-i`。**第 0 步实测：没有 TTY 时你的真 `.zshrc` 一句警告都没有、0.33 秒起来、不写历史**（第 10 节），下面的退路暂时用不上。原来担心的是 **`-i` 没有 TTY 时 zsh 会不会出幺蛾子**（rc 里的 `stty`、p10k 的 instant prompt、
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
| 1 ✅ | `crates/tasksvc`：起（shell、进程组、合并 stdout/stderr 写文件、剥 ANSI、轮转）、停（软 → 硬）、退出码、`live.json` | 单测：孙进程一起停、端口空了；软停不理 SIGINT 的进程 5 秒后被强杀；输出无上限时文件按 1GB 轮转（测试里上限可注入） |
| 2 ✅ | 任务从哪来：`tasks.json` 解析 + package.json 认脚本；命令、DTO、桩 | 纯函数测试 + dto_sync |
| 3 ✅ | 界面：运行工具窗 + LogPane、⌃R / ⌃⌥R / ⌘F2、任务列表；keymap / 菜单 | `pnpm dev` 桩上走一遍；状态层测试 |
| 4 ✅ | 端口被占的卡片、上次没停干净的卡片 | 测试通道验收：故意先占 8080 |
| 5 ✅ | smoke 一段 + `scripts/accept/tasks.sh`（issue 的三条验收），文档 | 真 .app |

入口包预计 +1 KB 以内：工具窗、任务列表、卡片全部懒加载，入口里只有 keymap 三行和「有没有在跑的任务」一个计数。

## 7. 验收（issue #48 那三条，加四条）

2026-10-10 第 5 步逐条过完（`tasks.sh` = 机制，用 `/usr/bin/python3` 小服务，谁的机器都能跑；`tasks-real.sh` = 你机器上的真工具，缺哪样哪段记「未验」）：

1. `mvn spring-boot:run` 跑起来，日志视图里 ERROR 标红、能过滤 —— ✅ `tasks-real.sh`（真 Spring Boot 3.5.11）；`tasks.sh` 用小服务的 ERROR 行再验一遍。
   **「能过滤」是这一步才真的成立的**，见第 15 节
2. 停止后 8080 释放，**没有残留 `java` 进程**（查进程组，不只查主进程）—— ✅ `tasks-real.sh`：组里两个 JVM，⌘F2 后端口空、组里一个不剩，`@PreDestroy` 跑了
3. `pnpm dev`、`python main.py` 同样能跑；`python` 的 `print` **实时**出来（不是结束才出来）—— ✅ `tasks-real.sh`（vite 三层进程停干净；pyenv 的 python 跑着时就看得到输出）
4. 从 Finder 启动的 lite-ide 里跑，能找到 `mvn` / `pnpm`（不靠从终端启动）—— ✅ `tasks-real.sh`：应用的 PATH 设成 Finder 双击时那样（`/usr/bin:/bin:/usr/sbin:/sbin`），
   先断言应用自己找不到 mvn / pnpm，再看任务的登录 shell（读你真的 `.zshrc`）找得到；脚本前后比对 `~/.zsh_history`，一个字节没变。
   **更正（2026-10-10 整体审核）**：第 5 步当时写的是「应用经 `open` 起，和 Finder 双击同一条路」—— 错的，`open` 会把终端的环境整个带给应用，
   那时应用的 PATH 里本来就有 mvn，这条验收是空的。结论当时靠的是第 0 步（模拟的空 PATH 下 `zsh -ilc` 找得到）；现在 bridge.sh 设了 PATH，这条才真的验到
5. 软停不理的进程（`trap '' INT`）5 秒后被强杀；软停期间再按一次立刻强杀 —— ✅ `tasks.sh`（另有 tasksvc 的单测）
6. 8080 先被占着时跑「后端」：卡片说出是谁占的；点「结束它并重跑」能跑起来 —— ✅ `tasks.sh`（第 4 步）
7. 跑着任务时让应用崩掉（测试构建里 `kill -9` 自己），重开 → 「上次没停干净」的卡片，点「结束」端口空了 —— ✅ `tasks.sh`（第 4 步）
- Gradle `bootRun` 停得干净 —— **本机没装 gradle，未验**，留给有 gradle 的人

## 8. 这一轮明确不做

- **调试**（断点、单步）：那是 LSP / DAP 的范围，立项时排除了。
- **problemMatcher / 把编译错误标进编辑器**：日志视图已经标级别、堆栈帧能跳，够用；真要做是下一件事。
- **任务之间的依赖、组合任务**（「先 build 再 run」）：命令里写 `&&` 就行。
- **全局任务**：任务属于项目（Q2）。
- **往任务里输入**（stdin）：任务的 stdin 接 `/dev/null`；要交互的命令请在终端里跑。

## 9. 我对这份稿子的把握

有把握的：键位、层级、杀进程组、端口卡片、崩溃后收尸 —— 都有业内的先例或者我们自己踩过的坑（`git fetch` 的孙进程、#42 的中断恢复）垫着。

**没把握、要第 0 步说话的三处**（2026-10-10 第 0 步都有了答案，见第 10 节；三处都是「建议的方案成立」）：

1. **管道下三个真工具的输出到底干不干净**（Q3）。如果 Spring Boot 或 vite 在管道下仍然大量输出颜色或进度条，剥 ANSI 够不够、要不要改用 pty + 日志视图认 ANSI，那是不同的工作量。
2. **`-ilc` 没有 TTY 时你的 `.zshrc` 会不会报错或卡住**（Q4）。卡住的话退路是「pty 里取一次环境再缓存」，多一步。
3. **SIGINT 对 `mvn spring-boot:run` 管不管用**：`spring-boot:run` 默认 fork 一个 JVM，SIGINT 发给整组，它们都会收到 —— 但 Maven 自己收到 SIGINT 时会不会抢在子 JVM 跑完关闭钩子之前把它强杀，要实测。

## 10. 第 0 步：实测结果（2026-10-10）

做法：模拟 Finder 启动的空环境（`HOME` / `USER` / `SHELL` / `TMPDIR`，`PATH=/usr/bin:/bin:/usr/sbin:/sbin`），经 `/bin/zsh -ilc '<命令>'` 起真工具，
stdin 接 `/dev/null`；管道（stdout + stderr 合进一个文件）和 pty 各跑一遍。每 50ms 看一次输出文件记下每行到达的时间，就绪后对整个进程组发 SIGINT，
每 100ms 查一次「组里还有谁」和「端口谁在听」。Spring Boot 用本机仓库里缓存的 3.5.11 离线搭了一个最小应用（启动时打一条 ERROR、注册关闭钩子和 `@PreDestroy`，监听 18080）；
vite 就是本仓库的 `pnpm dev --port 18081`；Python 是一个每秒 `print` 一行、共 4 行的脚本。

**一、shell 环境（Q4）**

| | 找到 `mvn` / `pnpm` | 警告 | 起来要多久 | 写进用户的 zsh 历史 |
|---|---|---|---|---|
| `zsh -lc`（第 1 节） | 都找不到 | —— | —— | —— |
| `zsh -ilc`，管道 | 都找到 | **无** | 第一行输出在 0.33 秒 | **没有**（3395 行前后不变：`-c` 的命令不经行编辑器，不进历史） |
| `zsh -ilc`，pty | 都找到 | 无 | —— | 没有 |

**二、输出（Q3）**

| | 管道：ESC / `\r` | pty：ESC / `\r` |
|---|---|---|
| Spring Boot 3.5.11（`mvn -o spring-boot:run`，58 行） | **0 / 0** | 292 / 58（每行 `\r\n` + 颜色） |
| vite 6.4.3（`pnpm dev`，7 行） | **0 / 0** | 42 / 9 |

管道下 Spring 的 `ERROR` 行原样是 `… ERROR 52267 --- [main] demo.App : T0 示范 ERROR 行`，日志视图的级别识别直接认得。

**三、缓冲（Sublime 的头号痛点，Q3）** —— 每秒 `print` 一行的脚本，每行什么时候到：

| | 到达（秒，累计行数） |
|---|---|
| 管道 | `(4.32, 4)` —— **4 行在程序结束时一起到** |
| pty | `(0.38,1) (1.36,2) (2.38,3) (3.35,4)` |
| 管道 + `PYTHONUNBUFFERED=1` | `(0.33,1) (1.31,2) (2.35,3) (3.36,4)` —— 和 pty 一样实时 |

**四、进程树与 SIGINT（Q5）**

| | 组里有谁 | SIGINT 之后全退 | 端口空 | 收尾 |
|---|---|---|---|---|
| `mvn spring-boot:run` | Maven 的 JVM + fork 出来的应用 JVM（zsh 自己 exec 掉了，不在） | 0.25 秒 | 0.09 秒 | **关闭钩子、`@PreDestroy`、Tomcat 的 graceful shutdown 都跑了**，Maven 打完 `BUILD SUCCESS` 才退 |
| `pnpm dev` | pnpm（node）→ vite（node）→ esbuild，三层 | 0.24 秒 | 0.09 秒 | —— |

管道和 pty 两种模式数字一样。**第 9 节第 3 条（Maven 会不会抢先强杀子 JVM）的答案：不会。**

**五、实测带出来的两条，进设计：**

1. **「失败」不能只看退出码。** pnpm 被 SIGINT 停掉后会打一行 `[ELIFECYCLE] Command failed.`，退出码非零 —— 我们自己发信号停掉的，状态一律记「已停止」，
   只有没人停它、它自己非零退出才是「失败」（端口卡片也只在这种时候扫输出）。
2. **逃出进程组的进程，SIGINT 整组够不着。** 这次三个工具都老老实实待在组里；会逃的是 Gradle 守护进程、`nohup … &` / `setsid` 起的、Docker 容器。
   这几种「停了但端口还占着」的兜底是 Q6 的端口卡片（它按端口查是谁，不按进程组）。Gradle 本机没装，仍然「未验」。

**结论：Q3（管道）、Q4（`-ilc` 现起）、Q5（SIGINT 整组 → 5 秒后 SIGKILL）三个建议都成立，第 1 步照原设计做。**

## 11. 第 1 步落地时改的 / 撞见的（2026-10-10）

- **`&` 起的后台命令收不到软停。** POSIX：没有作业控制的 shell 里，后台命令把 SIGINT / SIGQUIT 设成忽略（你在终端按 ⌃C 不该打断后台任务）。
  `-ilc` 没有 TTY，zsh 不开作业控制 —— 所以 `nc … & sleep 300 & wait` 这种命令，SIGINT 只停得了前台的 zsh，两个孙子要等宽限期满的 SIGKILL。
  第 0 步的三个工具没用 `&`，没碰上。**整组最后照样没了、端口照样空**（Q5 的保证不变），只是它们没有收尾的机会。
  要收尾的写法：别用 `&` 并排起，拆成两个任务 —— 第 5 步写进 USAGE 的「任务」一节。反过来的好消息：后台命令**没有**被分到新的进程组（实测孙子都在我们的组里），
  作业控制要是开着，`&` 出去的会逃出组，整组信号一个都够不着。
- **stdout / stderr 接同一根管道**，不是两根：两根就得两个线程并发读（rust.md：顺序读会死锁）、还排不出先后；一根管道由内核按写入顺序排好。
- **轮转放在写之前判。** 第一版是「写完这块发现过线、在末尾切开再换」：越线的那块整块进 `.1`，当前那份可能是空的，日志视图跟着一个空文件。
  改成「已经过线、又在行首，先换再写」。`sink.rs` 里有两条确定造出这个形状的单元测试（集成测试要看最后一块碰不碰巧跨线，靠不住）。
- 测试的 shell 是 `/bin/zsh -ilc` + 空的 `ZDOTDIR`：和产品同一条路，又不把跑测试的人的 `.zshrc` 拉进判据（issue #30）。

## 12. 第 2 步落地时定的细节（2026-10-10）

- **解析放在主 crate 的 `taskdefs.rs`**，挨着 `settings.rs`，直接用它的 JSONC 剥注释（`settings::strip`）：一种格式一个解析器。进程那半在 `tasksvc`。
- **`tasks.json` 坏一个跳一个**：缺 name / command、`cwd` 是绝对路径或用 `..` 出了项目、重名（用前面那个）、不认识的键（`cmd`、`dir` 这种拼错）——
  每条都说一句（`problems`），别的任务照常列出来。语法错说第几行。和 settings.json 一个规矩：**静默跳过会让人以为写对了**。
- **package.json 的 scripts 按文件里的顺序**：serde_json 没开 `preserve_order`，`Map` 按键名排序（`dev` 会排到 `build` 后面）。
  不为这一处去开整个依赖树的 feature，`scripts` 自己写了个反序列化器按文档顺序收。
- **不列生命周期钩子**：`preinstall` / `postinstall` / `prepare` 那一串是 npm 自己在 install / publish 时跑的；`preX` / `postX` 在 `X` 存在时是它的附属
  （`preview` 不是 `view` 的钩子 —— 没有 `view` 这个 script 就留着，测试里有这一条）。
- **包管理器**：`packageManager` 字段（corepack 的约定）优先，其次从那个目录往上到项目根找锁文件，都没有就 npm。命令统一写 `<pm> run <script>`；
  名字带空格、引号的套单引号，`build:prod` 这种常见的不套。
- **找 package.json 只看根目录和一层子目录**，跳过 `node_modules` 和隐藏目录。`apps/web` 这种第二层的不认，第一版不管。
- **新建 `tasks.json` 用 `create_new`**：已经有了就不动它（rust.md「std API 会吃掉已有文件」）。模板里三个例子全注释掉，解析出来是空的。

## 13. 第 3 步落地时定的 / 撞见的（2026-10-10）

- **一次运行一格，格子里是 `RunView` → `LogPane`**，只挂当前那一格。卸载不碰进程（任务活在 Rust 那边）—— 和终端相反：终端的 xterm 卸了 shell 就没了，所以那边只藏不卸。
  跟随默认开；重跑时 Rust 把上一次挪成 `.1`、新开同名文件，日志视图本来就是 `tail -F` 的语义（按名重开、句柄不变）。
- **重跑**：前端先把旧格子标成「正在停」（Rust 那边要等它真退、端口空了才起新的，可能要几秒），回来后**原地换**那一格（位置不跳、id 换新）。
- **退出事件**只发给起它的窗口（`task-exit`，`emit_to(owner)`）；状态只认「是不是我们停的」，不认退出码（pnpm 被停后退出码非零）。
- **收尾**：关一格 / 关窗口 → 软停，宽限期在后台线程里等（线程攥着 `Arc<Task>`，不然最后一份引用一丢就是立刻 SIGKILL）；⌘Q / Dock 退出 → 退出流程一开始就软停全部、
  最多等 3 秒（`RUNS_WAIT`），`RunEvent::Exit` 里把剩下的强杀 —— **`process::exit` 不跑析构**，不在那儿杀，`Task::drop` 那道兜底一次都轮不到。
- **验收的一个坑**：第一版的验收服务每 0.5 秒打一行。把退出流程的软停和退出时的强杀都去掉，「退出后端口空了」照样绿 —— 应用一退，读输出的那端关了，
  它下一次 print 撞上管道断了，被 SIGPIPE 带走。**话多的进程替我们把孤儿收了，安静的不会**（起来后不怎么打日志的 Spring 就是安静的）。
  验收服务改成说 8 句就闭嘴，再验红：两条都红。
- **⌃R 归 key**（keymap.ts）：App 的 keydown 接，焦点在 `.xterm` 里放过。菜单里有这一项但不挂键位。
- **图标**：JetBrains 的 `expui/run/` 下 `run` / `stop` / `rerun` 三个（带色，走 `<img>`），来源记在 `public/icons/SOURCES.md`。
- **入口包 148,465 → 150,181 B（+1,716）**，设计稿估「1 KB 以内」估少了。挪走前后各量一次：导轨上的运行按钮 463、App 的 ⌃R 和面板判断 291，
  剩下约 960 是运行状态、浮层开关、Overlays 里的懒加载壳、图标名、布局类型 —— 都是外壳上「有个常驻入口」的接线。`runs.last` 挪出入口只省了 70
  （归因说那个文件占 1.1 KB —— 又一次高估小模块）。CI 取整 146，**过了 145 的告警线**（160 才红）。

## 14. 第 4 步落地时定的 / 撞见的（2026-10-10）

- **认端口号只认带 `in use` 的行**（外加 Tomcat 的 `Connector[HTTP/1.1-8080]`），从后往前找最后一次报的；认不出端口号的不猜 ——
  Python 自己的 `[Errno 48] Address already in use` 就不带端口，猜错了会去结束一个不相干的进程。认得的写法在 `tasksvc::port` 的测试里一条一条对着（Spring、vite、Flask、Node、Go、uvicorn、Tomcat）。
- **结束谁，动手前再查一次**：卡片出来到人点按钮可能隔了几分钟，原来那个进程早退了、号被复用了。前端不带 pid 回来，只带端口，Rust 重新 `lsof`。
- **是进程组组长 → 整组先软后硬；不是 → 只动它自己**（SIGTERM，宽限期后 SIGKILL）。它的组可能是你终端里的一整个作业，一起停就殃及了别人。
  测试里让一个 `nc` 和测试进程同组：错误的实现会把测试进程自己也停掉 —— 验红时把这条放在单独的进程组里跑，Python 自己吃了 `KeyboardInterrupt`，外面那层没事。
- **是我们起过的**（账本里对得上组号）主按钮；**别的程序** `danger`（ui.md 第七条）。卡片在运行窗里那一格的顶上，不在编辑区的确认条里：它说的是这一次运行。
- **账本 `<应用数据>/runs/live.json`**：起任务记、退出划掉；启动时读，组长启动时间对得上的才算「上次没停干净」，**人处理之前一直留在文件里**（处理前又崩一次，下次还得找得到）。
  卡片在那个项目的窗口里出（启动后 1.5 秒问，同替换的中断卡片），「结束它们」= 整组先软后硬，「留着」= 从账上划掉、不再提醒。
- **话多的任务跟着应用一起死，安静的才留下来**：验收里 `kill -9` 应用时服务还在打日志，管道一断它就被 SIGPIPE 带走，留不下来让卡片收。等它安静了再杀才复现。
  顺带：应用崩过之后它原来的输出管道是断的，软停时它的「收尾」日志写不出去 —— 「收尾跑了没」改用它落的一个文件作证据。
- **一个没坐实的间歇红**：`结束占端口的_组长整组停` 在 workspace 全量跑时红过一次（查到的进程组号和任务的对不上），单独跑 15 遍、全量跑 4 遍都没复现。
  组号原来是另调 `getpgid` 查的，改成直接用 lsof 给的（`-Fg`），少一个「查的那一刻进程状态变了」的口子；测试失败时把两个组的成员都打出来 —— 下次撞上有现场。
- 入口包 150,181 → 150,341（+160：运行状态的 `stale` 字段、启动后多问一句）。

## 15. 第 5 步落地时撞见的（2026-10-10）

拿真工具在真 `.app` 上走 issue 的三条验收（`scripts/accept/tasks-real.sh`），**抓到两个前四步的桩、单测、小服务验收都漏掉的 bug**：

- **运行窗里的过滤一行都不藏。** RunView 给日志视图的初始条件写的是 `onlyHits: false`（「全文 + 标出命中」），点 ERROR 那一级之后按钮变了、
  计数对了，INFO 行一行没少 —— 看着就是过滤坏了，而 issue 验收 1 写的就是「能过滤」。改成和日志标签页一样的 `true`，想看上下文过滤条上点「全文」。
  第 3 步的验收只验了「输出进了日志视图」，没点过一次级别。
- **退得快的任务停完一直是「正在停」。** `stopRun` 等 `task_stop` 回包之后才把格子标成「正在停」；vite 收到 SIGINT 几毫秒就退，
  `task-exit` 事件**比回包先到**，格子先成了「已停止」又被回包改回「正在停」，之后没有任何事件再来改它。小服务和 Spring 退得慢（几百毫秒），碰不上。
  回包回来只在它还「在跑」时才标；状态层测试用桩里一个「退得快」的任务（名字带 quick，先发退出事件再回包）复现，撤掉修复就红。
  **事件和命令回包是两条路，谁先到没有保证** —— 用回包去改一个事件也会改的状态，要先看它现在是什么。
- **pnpm 11 在 `run` 之前会自己 `install`。** 第一版想在临时目录里搭个小 vite 项目、`node_modules` 软链到本仓库那份，`pnpm run dev` 时
  pnpm 查依赖对不上，自己起了 `pnpm install`，要去删那个软链指向的目录（本仓库的 `node_modules`）—— 被它自己的检查拦下了
  （`ERR_PNPM_UNSAFE_MODULES_DIR`）。所以验收在已经装好依赖的本仓库里跑 vite（命令里 `cd` 过去）。这也是「任务」本身要知道的：
  `pnpm dev` 第一次跑可能先装依赖、走网络，日志里会看到。
- 验收自己的几个坑：日志视图只画屏幕上那几十行，「有没有某句输出」要读输出文件，不读视图（`@PreDestroy` 那行早滚出去了）；
  vite 默认只听 `localhost`（`::1`），拿 `127.0.0.1` 连不上；macOS 的 `sleep` 只收一个参数，`sleep 999 <标记>` 立刻报错退出 ——
  「软停不理的」那段第一版因此全是空断言绿，现在先断言它真的在跑。
- smoke 加了 ㉕（跑 → 输出进运行窗 → ⌘F2 停干净），86 条。

## 16. code review 之后改的（2026-10-10）

审第 1–5 步的代码找到 10 条，修了 4 条，剩下的在 #56。修的 4 条里，前三条是同一个形状：**一件事有好几个入口，只有一个入口做全了**。

- **停一个有 `Task` 的任务，只能走 `Task`**（`stop` / `stop_wait` / `kill`，都会先记下「是我们停的」）。重跑时停旧的那次、关格子的收尾、端口卡片结束自己的任务，原来各自直接 `stop_group(pgid)`，
  绕开了这个标记 —— 被停的那次退出码非零时就记成「失败」，还弹一句「退出了」。`stop_group` 只留给没有 `Task` 的（崩溃后收尸）。
  **review 里有一处说错了**：当时说 `mvn spring-boot:run` 每次 ⌃R 都会撞上，其实 Maven 收到 SIGINT 后照样 `BUILD SUCCESS`、退出码 0（第 10 节）；
  撞得上的是 `pnpm dev`（`Command failed`，非零）。验收那条第一版放在 Spring 那段，旧包上照样绿才发现，挪到了 vite 那段。
- **正在收尾的任务也要让退出流程看见**：关掉一格 / 关窗口 / 重跑时，任务一摘出运行表就只活在收尾线程里，5 秒内 ⌘Q，线程跟着进程没了，不理 SIGINT 的留成孤儿。
  现在有一张「收尾表」（`AppState::retiring`），退出时等它们、`RunEvent::Exit` 里一起强杀。
- **同一个任务正在起时，第二次被挡住**：重跑要等旧的软停（最多 5 秒），人按急了再按一次 ⌃R，原来会起两份、一份从界面上消失还占着端口。
  Rust 侧 `begin_run` 按（窗口, 项目, 名字）认领，前端 `runTask` 也挡一道（不弹「正在起」，人只是按急了）。
- 命令层（`commands/tasks.rs`）里的日志路径、从日志尾认端口挪进了 `tasksvc`（`log_file`、`port::from_log`），「先停旧的再起」挪进 `state.rs`（`begin_run`）——
  上面几条修复因此都能脱离 Tauri 测：tasksvc 2 条、state.rs 2 条、状态层 1 条，5 处改坏各自红。
  `state.rs` 那条「收尾中的也强杀」第一版是空断言：SIGINT 落在 zsh 执行 `trap` 之前，任务被软停自己打死了 —— 和第 1 步 `ARMED` 那条同一个坑，改成等它打出 `armed`。
- 真 .app：`tasks.sh` 补「关掉在跑的格子马上 ⌘Q，不理软停的也没了」（35/35），`tasks-real.sh` 补「pnpm 重跑时旧的那次不报『退出了』」（17/17）。两条都先在旧包上跑过，都红。

## 17. 收尾梳理之后改的（2026-10-10）

- **「结束占端口的组长」那个间歇红，根因大概率是测试取端口的空档**：测试先绑 0 拿个号、关掉、再让 `nc` 去绑；而 macOS 的 `nc` 开 `SO_REUSEPORT`
  （实测两个 `nc -lk` 同时听一个端口都不报错），两条并行的测试拿到同一个号时，第二个照样绑上，`lsof` 查到的是另一条测试的进程。
  测试改成监听者自己绑 0、把号写进文件（Python，默认不开 `SO_REUSEPORT`）。之后连跑 20 遍没红 —— 原来就很少红，这个数不算证明，证据是上面那个实验。
- **一个端口可以有好几个监听者**（同一个原因，产品也受影响）：`holders()` 返回全部，`free_port` 并排结束每一个（总共只等一个宽限期）；
  等它们退出改用 `kill(pid, 0)`，只在最后确认端口时调 lsof（原来每 100ms 起一个，最坏一次 70 个）。
- **任务输出的清理**（`tasksvc::prune`）：启动时在后台清掉 14 天没跑过任务的项目的输出，剩下的总量超过 5GB 从最旧的删起。
  只认自己生成的（`<16 位十六进制>/` 底下的 `*.log` / `*.log.1`）；直接删、不进废纸篓（应用自己的数据，同替换的撤销日志 —— rust.md 那条改准了）。
- 收尾记账抽成 `finish_retire` 一份；watcher 用不限时的 `Task::wait()`。
- 没做、开了 issue：缓存登录 shell 的环境（#57）、`"pty": true`（#58）、停完检查组外进程占的端口（#59）、Gradle 验证（#60）。
