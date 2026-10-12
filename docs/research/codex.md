# 读 Codex：对照 lite-ide 的学习笔记

读的是 [openai/codex](https://github.com/openai/codex)（Apache-2.0）提交 `a9b0815`（2026-10-11）。主体是 `codex-rs`：117 个 crate、约 205 万行 Rust ——
lite-ide 的 Rust 约 2.7 万行，差 76 倍。**所以只按主题挑着读**，每个主题回答四件事：它怎么做、为什么、我们现在怎么做、学不学。
引用的路径都相对 `codex-rs/`。下载来的代码只读，不在里面跑任何构建、脚本、测试。

| # | 主题 | 状态 |
|---|---|---|
| 1 | 起子进程、进程组、杀进程、沙箱、进程加固 | ✅ 2026-10-12 |
| 2 | 编辑器和 agent 怎么通信（`app-server-protocol`）—— #46 的前提 | 未读 |
| 3 | 给模型看的输出怎么截断（`utils/output-truncation`） | 未读 |
| 4 | 配置分层、会话记录落盘 | 未读 |
| 5 | 测试和工程组织 | 未读 |

---

## 主题 1：子进程（2026-10-12）

读了：`process-hardening`（193 行，全读）、`utils/pty/src/{process_group,child_reaper,spawn_helper}.rs`、`core/src/exec.rs` 的常量和注释、
`core/src/shell_snapshot.rs` 的 `run_script_with_timeout`、`shell-command/src/shell_snapshot*.rs` 的结构、`sandboxing/src/seatbelt*.{rs,sbpl}` 的头部。

### 先说结论：我们的纪律和它的重合，这本身是证据

两个项目各自踩坑长出来的规矩，几乎一条条对得上：

| 规矩 | Codex | lite-ide |
|---|---|---|
| stdin 接空，绝不让子进程等输入 | 快照脚本 `stdin(/dev/null)` | `procutil::spawn` |
| 自己的进程组，停的时候整组 | `set_process_group` / `kill_process_group` | `tasksvc`、git fetch |
| 有时限，到点就杀 | 命令默认 10 秒、快照 10 秒 | 任务软停 5 秒宽限期；git 不设超时、靠取消（rust.md 有理由） |
| 丢掉就收尸 | `child_reaper` | `procutil::Running` 的 Drop、`Task` 的 Drop |
| 输出有上限 | `EXEC_OUTPUT_MAX_BYTES` | `run_capped` |

**不一样的地方才是要学的**，下面四条按对我们的价值排。

### 1. 后台孙进程攥着管道，我们会一直等下去（实测坐实，要修）

`core/src/exec.rs` 的 `IO_DRAIN_TIMEOUT_MS` 上面那段注释说的是：子进程派生的孙进程继承了 stdout / stderr，杀掉（或等到）子进程之后
管道还开着，读的那一方等不到 EOF，**整个 agent 卡死**。它的对策：子进程退出后，读管道最多再等 2 秒，到点放弃（不杀那个孙进程）。

我们的 `procutil::run_capped` 是「stdout 读到 EOF → wait → join stderr 线程」，三步都会等孙进程。临时加了一条测试实测（跑完删了）：

| 命令 | 多久回来 |
|---|---|
| `sleep 4 & echo hi`（后台的攥着 stdout、stderr） | **4.02 秒** |
| `sleep 4 >/dev/null & echo hi`（只攥着 stderr） | **4.02 秒** |
| `sleep 4 >/dev/null 2>&1 & echo hi`（都不攥） | 11 毫秒 |

后台那个不退，我们就永远不回来。**真实的入口是 `git commit` 的钩子**：钩子是仓库带来的（不是用户写的），一个
`post-commit` 里 `(做点什么) &` 没重定向输出，界面上的「提交」就一直转；git 写操作同一时间只许一个，之后所有 git 写操作都堵住，直到重启。
（git 自己的 `gc --auto` 后台化时会关掉 stdio，碰不上；碰得上的是写得随手的钩子。）

修法有两条，都记下来，还没定：
- **照 Codex：子进程退出后限时等管道**。`stdout` 也挪进 `drain` 线程，主线程 `wait()` 之后对两个线程各等一个上限，超了就拿已经读到的、
  把线程撒手（孙进程退了它自己会结束）。不杀孙进程 —— 钩子的后台活可能是用户想要的。
- 放进自己的进程组，子进程退出后 `killpg` 掉剩下的。更干净，但会杀掉钩子有意留下的后台活，**改变了用户的钩子的行为**，不选。

### 2. EPERM 不一定是「组里只剩僵尸」（实测，缺口真实但概率低）

`utils/pty/src/process_group.rs`：macOS 上 `killpg` 被拒（EPERM）时，用 `proc_listpgrppids` 列出组员、逐个发。
我们的 `tasksvc::group_alive` 把 EPERM 当「没有活的」，依据是第 0 步实测的「组里只剩没被回收的僵尸时 macOS 回 EPERM」。

实测（Python + ctypes，scratchpad 里）把几种组合摆出来：

| 组里有什么 | `killpg(组, 0)` | 信号送到了吗 |
|---|---|---|
| 活的组长 + 一个僵尸 | ok | 送到了，组长死了 |
| 活的 sh + `top`（setuid root，但**真实 uid 是我**）+ sleep | ok | 都死了 |
| 只剩一个僵尸 | **EPERM** | —— |

所以 EPERM 的来源是「组里有一个**真实 uid 不是我们**的进程」（典型是 `sudo` 正在以 root 跑命令），或者只剩僵尸。前一种情况下我们会误判成
「停干净了」、不再强杀。任务的 stdin 是空的，`sudo` 要密码会直接失败，所以**对我们是低概率**；Codex 的 agent 自己会跑 `sudo`，所以它必须处理。
要补的话照它：EPERM 时列组员，有不是僵尸的就算还活着。先记着，不排进优先级。

**测试手法值得学**：它把「发给组」「发给单个进程」做成参数（闭包）传进去，测试里直接注入一个 EPERM —— 不用真的造出一个 root 进程。
和我们把输出上限做成可注入（`run_raw_capped`）是同一个手法：**罕见的系统状态，注入，别去造**。

### 3. shell 快照：同一个问题，解法的规模跟调用频率走（对 #57 的判断）

Codex 每个会话跑一次用户的交互式 shell，把函数、`setopt`、别名、导出变量存成快照文件，之后每条命令加载快照、不再跑 rc。
`shell-command` 里这一块连测试 5,400 行，光是字面量转义 715 行，还有一份专门剔除凭据的。

我们每次起任务都 `$SHELL -ilc`（TASKS.md 第 10 节实测 0.33 秒）。#57 问的是要不要缓存。判断：

- Codex 一个会话跑几百条命令，每条省 0.3 秒值得 5,400 行；我们一天起几次任务，**0.33 秒不值得这么一套**。
- 真要做，做 VS Code 那种只缓存环境变量的：启动后跑一次 `$SHELL -ilc env`、带超时，缓存 PATH 那几样。函数、别名不管 ——
  任务命令要用 alias 的，本来就该写进脚本。
- #57 原来写的就是「先量再说」，这条读完更确定了：**量到 0.33 秒真成了问题再做**。

**通用经验**：看别人的方案先看它的调用频率和代价，再决定抄多少。直接抄一个为高频场景设计的东西，得到的是它的全部复杂度和一小部分收益。

### 4. 进程加固和沙箱：它防的人和我们不一样

`process-hardening` 在 `main` 之前做三件事：`ptrace(PT_DENY_ATTACH)` 不让调试器附加、`RLIMIT_CORE = 0` 不留 core 文件、删掉 `DYLD_*` 环境变量。
`sandboxing` 用 `/usr/bin/sandbox-exec` + 一份**默认全拒**的策略（照 Chrome 的写法）跑模型发起的命令，子进程继承同一个沙箱。

它防的是**同一个用户下、由模型发起的命令**：那些命令和 agent 同一个 uid，可以附加调试器读 agent 内存里的 API token、可以读 core 文件。
我们现在没有这种「替别人跑命令」的角色 —— 任务和终端都是用户自己写的命令，进程里也没有 token。**现在不学**；做 #46 时 agent 跑的命令在 agent 自己的沙箱里，
不在我们这边。

两个小细节是通用的：
- **调安全相关的程序写绝对路径**：它只认 `/usr/bin/sandbox-exec`，注释里说的理由是防有人在 PATH 前面放一个同名的。
- **默认全拒、逐条放行**，和我们 `gitsvc::trust` 的 config 白名单是同一个判据：黑名单永远在追下一个没想到的。

### 5. 还有两条，小，记着

- **procutil 的 Drop 是「kill 之后阻塞 wait」**。进程卡在不可中断的系统调用里（读一个断了线的网络卷）时，SIGKILL 要等那个调用返回才生效，
  `wait()` 跟着一直等。Codex 的 `child_reaper` 是一条全局线程轮询收尸，丢掉的那一下从不等。我们的调用点都在阻塞池里，影响是吃掉一条池线程；
  低概率，记着。
- **多线程进程里 fork 之后、exec 之前只能调「异步信号安全」的函数**（别的线程可能正攥着 malloc 的锁，子进程里只复制了当前线程，锁永远没人放）。
  Codex 在 Linux 上为此专门先起一个干净的小程序再 exec（`spawn_helper.rs`）。我们用 std 的 `process_group(0)`（走 `posix_spawn` 的属性，不在中间跑我们的代码），
  **别为了加功能写 `pre_exec`** —— 写了就得守这条。

### 对 lite-ide 的行动项

| | 做什么 | 排在哪 |
|---|---|---|
| ① | ~~procutil 子进程退出后限时等管道~~ ✅ 2026-10-12 修了（`PIPE_GRACE`，四条测试验过红；写的时候自己差点漏「数据一直来时也要问退了没有」那一半） | 做完 |
| ② | `group_alive` 的 EPERM 缺口 | 记着，不排 |
| ③ | #57 维持「先量再说」，要做只缓存环境变量 | #57 里补一句 |
| ④ | procutil Drop 不阻塞 | 记着，不排 |
