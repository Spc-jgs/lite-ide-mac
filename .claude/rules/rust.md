---
paths:
  - "src-tauri/**"
---

# Rust 侧的规矩

下面每一条都是**已经踩过**的坑，不是预防性的规矩。

## 改前端后 Rust 侧要能感知

`src-tauri/build.rs` 里有 `cargo:rerun-if-changed=../dist`。少了它，
改完前端重新构建，产物里嵌的还是旧的。

## 命令层不写业务

`src-tauri/src/commands.rs` 只做三件事：解包参数、查句柄、转错误。
一行业务逻辑都不写。业务全在 `crates/` 里 —— 那五个 crate **不依赖 Tauri**，
才能脱离 GUI 单测和跑 bench。

日志引擎是这个项目唯一的技术未知数，它必须能独立验证。别把它跟 Tauri 绑死。

## 同步命令跑在主线程上，会卡界面的活必须挪走

Tauri 的 `#[tauri::command] fn`（不带 `async`）**跑在主线程上** —— 官方文档原话：
「Commands without the async keyword are executed on the main thread」。
主线程就是 NSApplication 的事件循环，它一堵窗口就不响应：菜单点不开、拖不动、转菊花。

**大部分命令不用管。** 实测（M 系列，1.1GB / 3518 文件的仓库，热缓存）：
`git status -uall` 0.04s、`rg --files` 0.02s、`rg` 全文搜高频词 0.07s、`git log -300` 0.02s。
40ms 是两帧半，不值得为它换来「命令之间不再串行」这个新变量。

**要挪走的是时长不可控的那几条**，走 `commands::blocking`（tokio 的阻塞池）：

| 谁 | 为什么 |
|---|---|
| `git_commit` | pre-commit 钩子跑什么是仓库说了算，跑一遍 eslint 三十秒 |
| `git_switch` / `worktree_add` / `worktree_remove` / `merge_upstream` | 检出几千个文件是秒级 |
| `git_fetch` / `git_push` | 走网络，本来就是「几十秒」那一档 |
| `grep_project` / `list_project_files` | 仓库多大是用户说了算 |
| `write_text` / `trash_entry` | 网络卷上是另一个数量级 |

两条容易搞错的：

- **别用 `#[tauri::command(async)]` 去解决它。** 那个宏对同步函数生成的是
  `async_runtime::spawn`，**活还是跑在 worker 上**（`sync_threadpool` 只是个
  tracing 标签，不是 `spawn_blocking`）。而 worker 只有 2 个
  （`lib.rs::install_runtime` 把 Tauri 默认的 18 个换掉了），两条并发的 git
  就能把它占满。阻塞池才是这类活该待的地方 —— 它按需长、空闲自己收。
- **`log_*` 和 `pty_write` 留在主线程上**，那是有意的：它们是常数时间的内存操作
  （首屏 50 行 0.008ms），挪到 runtime 上只会多一次调度延迟。

`commands::blocking` 有一条断言线程 id 的测试卡着。退化成「原地调用一下」的话
编译照过、返回值照对，而界面照样卡 —— 这种回归没有任何编译期信号。

## 后台跑起来的活，必须有人喊得停

`logengine::LogFile::open` 起两条后台线程（索引、级别探测），它们各攥着一份
`Arc<Mmap>`。原来**没有取消开关** —— `close_log` 只是把句柄从 `AppState`
的表里摘掉，线程照样把整个文件扫完。

同一个 crate 里的 `FilterTask` 明明有 `cancel()`（换过滤条件时旧任务立刻停），
所以这不是设计取舍，是漏了。

开关放在 `impl Drop for LogFile` 里，不是给 `close_log` 加一行调用 ——
**调用点会有人忘**，而 `Drop` 只要最后一个 `Arc<LogFile>` 没了就一定跑到。
两条线程各自的检查粒度：索引每块（16MB / 约 3ms）一次，级别每 64K 行一次。

**代价没有想象中大，如实记着**：级别扫描实测 51.8MB / **14.9ms**
（M 系列、热缓存，约 3.5GB/s），一个 1GB 的日志关掉之后大约还要跑 0.3 秒
（冷缓存更慢）。所以这条修的是「白烧一段 CPU、mmap 多吊一会儿」，
不是什么内存黑洞 —— 审查时按「还要扫完 1GB」写的那句夸大了。

**扫描循环要拎成函数**（`scan_levels` / `build_index`），不能留在线程闭包里：
3.5GB/s 意味着想靠「造一个大到来不及扫完的文件」去验证取消开关，得造到 GB 级，
那种测试又慢又不稳。拎出来之后把开关**预先置位**再调用，结果是确定的。
配套的三条测试（Drop 置位、级别扫描认账、索引推进认账）都各自验过红。

顺带一条测试陷阱：`scan_levels` 每 64K 行才查一次开关，**测试数据的行数
必须跨过 65536**，否则那个检查一次都不会执行，断言就成了永远绿的假断言。

## 数据面走二进制

日志行**不能**用默认的 `invoke`（JSON）。传 1000 行 JSON 约 15ms、二进制约 1ms，
而 60fps 单帧预算只有 16ms。用 `tauri::ipc::Response` 传 `Vec<u8>`，
线格式在 `crates/logengine/src/block.rs`。

## 起子进程时的三条硬纪律

`gitsvc` 和 `searchsvc` 都起子进程（`git` / `rg`）：

1. **绝不拼 shell 字符串**。全部走 `Command::arg`，路径前一律加 `--` ——
   否则一个叫 `-f` 的文件就能变成命令行开关。
2. **绝不让子进程卡住等输入**。`GIT_TERMINAL_PROMPT=0` 关掉凭据提问；
   `GIT_OPTIONAL_LOCKS=0` 让 `git status` 不抢 index 锁（用户正在终端里
   跑 rebase 时，后台刷新不该把它顶失败）。
3. **绝不让仓库自己的配置决定 git 去执行什么。** `.git/config` 是**仓库带来的
   文件**，不是用户写的。里面一句 `core.fsmonitor = ./脚本` 就能让一条普通的
   `git status` 去执行它 —— 实测确认，而 lite-ide 在项目根一变就自动跑 status
   （App.svelte 那条 effect），**用户一次都不用点**，会话恢复还让它每次启动都再跑一遍。
   也就是说：**别人给你一个目录 = 别人在你机器上跑代码**。

   `gitsvc::HARDENING` 给每条 git 都带上 `-c core.fsmonitor= -c diff.external=`，
   产生差异的命令另外带 `DIFF_SAFE`（`--no-ext-diff --no-textconv`）。
   **加固放在 `git_cmd` 上而不是各个调用点** —— `--no-ext-diff` 当初只写在四个
   产生 diff 的地方里的两个上，同一条纪律写四遍就是迟早漏一遍。

   挡不住的如实记着：`filter.*.smudge`（检出时跑）、`remote.*.url = ext::…`
   （fetch/push 时跑）—— 这两条都要用户主动动手才碰得到。真正的解法是
   「这个目录信不信得过」那一套，那是另一件事。

另外 `LC_ALL=C`：用户 locale 是中文时，别让 git 把机器格式翻译了。

## 改盘的 std API 默认都会吃掉已有文件

写文件树的新建/改名时实测过，两条都不是理论风险：

| 顺手的写法 | 它对已存在的目标做什么 |
|---|---|
| `File::create(p)` | **截断成 0 字节**。新文件手滑取成已有文件的名字，那份内容当场就没 |
| `fs::rename(a, b)` | **静默覆盖 b**。这是 rename(2) 的语义，不是 Rust 的选择 |
| `fs::create_dir_all(p)` | 对已存在的目录返回 `Ok`。于是「新建」一个早就在的文件夹会报成功 |

对应的写法是 `OpenOptions::create_new(true)` / 自己先查一次 / `fs::create_dir`。
`fsservice` 里那三条测试都**先把 std 的行为演示一遍**再断言我们拦住了 ——
不演示的话，下一个人会以为那几行是可有可无的防御性代码。

存在性检查一律用 `symlink_metadata`，不用 `exists()` / `try_exists()`：
后者跟随符号链接，于是一个指向已删除目标的坏链接被判成"不存在"，
然后被 rename 覆盖掉 —— 丢的是链接本身。

**保存一份已有文件，要抄回它的三样东西。** 「临时文件 + rename」这条路
默认全丢，实测（`write_text` 的探针，2026-09-07）：

| | 修之前 | 为什么要命 |
|---|---|---|
| 权限位 | `0755` → `0644` | 编辑一个脚本，保存完它**不能执行了**，而界面报「已保存」 |
| 软链 | 链接被换成普通文件 | 改动写进了新文件，真身一字未动 —— `~/.zshrc` 指向 dotfiles 仓库这种用法正好踩中 |
| 硬链接 | inode 变了，链接组被拆 | 「同一个文件」悄悄变成两个，另一头再也收不到改动 |

现在的分法照 vim 的 `backupcopy=auto`：**软链先解引用，硬链接改成原地覆写，
其余走临时文件 + rename 并把权限抄过去**。三条各有一条会红的测试。

还有一条**测不出来**的：`rename` 之前必须 `fsync` 那个临时文件。
rename 只保证「要么旧的要么新的」，不保证新那份的内容已经落盘 ——
掉电之后可能拿到一个 0 字节文件盖掉了原文。进程崩溃没有这个问题
（内核已经收下数据），所以这条只能写在注释里，造不出测试。
（macOS 的 `fsync` 还刷不穿硬盘自己的写缓存，那要 `F_FULLFSYNC` 和一个
libc 依赖 —— 没引，我们要的只是「数据先于 rename」这个次序。）

**大小写要靠 inode 判，不能比路径字符串。** macOS 默认的 APFS 卷大小写不敏感，
`readme.md` → `README.md` 时目标"已存在"，而存在的正是源文件自己。
比 `dev + ino`（`MetadataExt`）就不用先问「这个卷敏不敏感」，
两种卷上都对。少了这条，「把 readme 改成 README」永远失败。

## 删除只走废纸篓

`fsservice::move_to_trash` 是应用里唯一的删除路径，**没有 `remove_file`**。
它调系统 API（macOS 上 `NSFileManager` 的 `trashItemAtURL:`，由 `trash` crate 包装），
不是自己往 `~/.Trash` 里 rename —— Finder 的「放回原处」靠一份系统维护的元数据，
外部卷的废纸篓在卷自己的 `.Trashes` 里，同名冲突还要按 Finder 的规则改名。
判据和「.gitignore 的规则以 git 为准，所以起 git 子进程」是同一条。

那条真去扔文件的测试标了 `#[ignore]`（它会往跑测试的人的废纸篓里扔东西）。
**它必须被手动跑过**，否则 `trash::delete` 换成 `Ok(())` 剩下的测试照样全绿：

```bash
cargo test -p fsservice -- --ignored 真的把文件移进废纸篓
```

顺带一条环境陷阱：终端没有完全磁盘访问权限时 `ls ~/.Trash` 会
`Operation not permitted`，但 `stat ~/.Trash/具体文件名` 读得到。
验证「东西真的进了废纸篓」要按具体路径查，别看目录列表为空就下结论。

## 子进程输出必须设闸

`git diff` 会为一个 30MB 的新增文件**原样吐 30MB**。这份文本走一趟 JSON IPC
再在前端解析成行对象，实测堆占用 +114MB、解析 57ms —— 而差异面板
**最多只渲染 3000 行**。为三千行付一百多兆，纯亏。

`gitsvc::MAX_DIFF_BYTES`（1MB）在 Rust 侧就掐断：piped stdout 读 `cap+1` 字节，
超了就 `child.kill()`，再切回最后一个完整换行（切在半行上，前端会把残行
当成一条真改动画出来）。截断后 2ms / +5MB，还剩 14,534 行，仍是渲染上限的四倍多。

两条配套的：**被掐掉的子进程退出码没有意义，不能当失败**；
`truncated` 必须一路传到界面（一份看着完整、其实少了后半截的差异，
比一句「显示不下」危险得多）。

新加任何「跑子进程读它 stdout」的功能，先问一句：这东西的输出有上限吗。

**读两个管道必须并发，顺序读会死锁。** 这条是 2026-09-07 真的挂住之后补的：
把 `git commit` 写成「先把 stdout 读完，再顺序读 stderr」，跑起来 git 和我们
互相等着，最后是手动 kill 掉的。根因是一个反直觉的事实 ——
**git 2.50 把 pre-commit 钩子的 stdout 转到了 stderr**（实测一个 200 行的钩子：
git 的 stdout 89 字节、stderr 2892 字节）。钩子一话多就写满 stderr 那几十 KB
缓冲卡在写上，而我们在等 stdout 的 EOF，那个 EOF 要等它退出才来。
界面上的表现是「点了提交，然后什么都不再发生」。

`gitsvc::drain_stderr` 起一条线程排空 stderr，`run_capped_raw` / `run_drained`
都走它。**线程里超过上限也要继续读**，只是不再存 —— 停下来就是同一个死锁。
顺带一句：原来的 `.output()` 反而没这个问题，它内部就是并发读两个管道的；
**手写顺序读的那一刻就得把这条一起写下来**。

回归测试是 `钩子话多不能把提交挂住`（5000 行的钩子，卡 20 秒）——
修好之后不到 1 秒，挂住的那一版走满 20 秒。

## 输出「本该有界」和「本来就可能很大」要分开处理

同样是设闸，两种命令的正确行为是相反的，`gitsvc` 里现在分成三条路：

| 命令 | 走哪条 | 超上限时 |
|---|---|---|
| 一个 sha、一个分支名、一份分支列表 | `run_raw` | **报错**。一份少了后半截的分支列表看着和完整的一模一样，而它会让「这个分支存不存在」悄悄给出错答案 |
| `git status` | `status_capped` | **截断 + 标 `truncated`**。改动多是仓库的正常状态，把整块 Git 功能变成一条报错，比少列几条改动糟得多 |
| `git commit` | `run_drained` | **截断，但不 kill 子进程**。掐掉进程会让退出码失去意义，那时「提交成功但钩子话多」和「提交失败」分不出来 —— 而把一次成功的提交报成失败，用户会照着那句话再提交一次 |

配套的两条：`status` 截断后要**砍到最后一个完整记录**（`trim_to_last_record`），
否则改动列表里会多出一个看着像真的、其实点不开的半截文件名；
界面那句提示写**实际条数**，不写死 5000 —— `truncated` 现在有两个来源。

上限一律做成**可注入**的（`run_raw_capped` / `status_capped` 多带一个 `cap`），
判据同 `fsservice::read_text_detect`：真造一份 4MB 输出的仓库来测不现实。
**但只测可注入的那个版本不够** —— 把 `run_raw` 改回 `.output()` 之后测试照样绿
（试过）。所以那条测试里还有一段真造了 5.9MB 输出去走 `run_raw` 本人。

2026-09-05 加「展开未跟踪目录」（`git ls-files --others`）时照这条办了：
掐子进程那一整套（读 `cap+1`、超了 `kill`、stderr 限量读、被掐的退出码不算失败）
从 `run_capped` 里抽成了 `run_capped_raw`，按字节返回。**记录边界由调用方切**——
差异切换行、`-z` 的路径列表切 NUL，抽的时候别把换行那条也一起抽进去。

**这条规矩自己漏过两处**，都是 2026-09-03 补的 —— 立了规矩不等于全仓库都照做了：

| 漏的地方 | 数字 |
|---|---|
| `searchsvc::grep_rg` 用 `.output()` 全缓冲 | 本仓库搜一个 `e`：读完 5,663,558 字节，而读到第 60 条命中只要 **18,568** |
| `fsservice::read_text_detect` 用 `fs::read` | 无上限。`probe_path` 的体积判定只在首次打开时把关，三条重读路径全绕过它 |

顺带一条**同形状但更阴**的：给读取设上限时，要读 `cap + 1` 字节。
只读 `cap` 的话，超限的文件会**静默返回截断后的内容** —— 而调用方
看到的是 `Ok`。报错比悄悄少半截安全得多。

## 改动列表里不放目录，但「哪些目录整个是新的」这个信息不能丢

`git status --untracked-files=normal` 会把一个完全未跟踪的目录折叠成一条
`dir/`。它**不该进改动列表** —— 一个目录点不开差异，也说不清里面到底多了
什么，那一行除了占位没有别的用。所以 `status()` 拿到折叠条目之后再起一次
`git ls-files --others --exclude-standard -z -- <dir>` 摊成具体文件。

**别顺手改成 `--untracked-files=all`。** 那样 git 就不再告诉你哪些目录是
*整个*未跟踪的了，而文件树要靠这个给目录**自己**上色 —— 少了它，
一个全新的目录只剩「里面有东西改了」的冒泡标记，和一个改了一行的老目录
长得一模一样。现在两样都留着：目录名在 `Status::untracked_dirs`，
里面的文件在 `entries`。

配套的两条：`Entry::is_dir` 跟着删了（`entries` 里恒为 false 的字段是个坑，
下一个人会照着写判断）；桩（`mock-ipc.ts`）也要照着摊 —— 它原来只喂一条
`scratch/`，不改的话浏览器里的改动列表和 `.app` 里长得不一样。

## 过 IPC 的 DTO 靠测试卡住，不是靠自觉

`src-tauri/tests/dto_sync.rs` 解析 `commands.rs` 的 `#[derive(serde::Serialize)]`
结构体和 `commands.ts` 的 `export interface`，逐字段比，并强制每个 DTO 都带
`#[serde(rename_all = "camelCase")]`。

新加一个过 IPC 的 DTO 必须在它的 `PAIRS` 表里登记一行 —— 忘了就红。
（ARCHITECTURE.md 曾经宣称这件事由 `ts-rs` 做，那套东西从来没落地过。）

## 新代码加在测试模块**之前**

用 `cat >>` 往文件尾追加过一次，结果生产代码落到了 `#[cfg(test)] mod tests` 后面，
读文件的人会以为测试模块之后就没东西了。

## ptysvc 的 `kill()` 里有一条排空线程，别删

`Session::kill()` 在杀 shell **之前**先起一个短命线程把 pty master 读到 EOF。
少了它，退出中的 shell 写满 master 缓冲区、没人排空，`child.wait()` 永远等不到 ——
而 `state.rs` 的 `kill_pty` 一挂，整张 pty 表的锁就再也不放，之后所有终端操作全堵死。

三条配套的，动其中任何一条都要一起看：

- **不 join 那条排空线程**。孙子进程攥着 slave 时 EOF 不会来，join 本身会变成第二个挂点。
- **收尸有界**：`wait()` 是 5s 上限的 `try_wait()` 轮询。宁可留个僵尸到进程退出，
  也不能让界面永久卡死。
- `commands.rs` 那个 pty 读线程的 `send 失败即 break` **依赖上面那条排空线程**才不挂。
  这是跨文件的隐式依赖，而回归测试在 ptysvc 里。

回归测试是 `关掉不排空的终端不能把kill卡死`，阈值卡 2s 不是 8s ——
只断言「最终返回」的话，删掉排空线程照样绿（那时 kill 要等满 5s 兜底 deadline，
实测有排空 55ms、无排空 5214ms）。

排查全过程见 [docs/JOURNAL.md](../../docs/JOURNAL.md) 与 [issue #2](https://github.com/Spc-jgs/lite-ide-mac/issues/2)。

**pty 测试要先等提示符，再敲命令。** 原来三条测试都是 spawn 完立刻
`write_input`，而登录 shell 那时还在 source rc 文件 —— tty 的行规程会把敲进去的
字**回显**出来（所以输出里看得到 `pwd\r\n`），但 rc 里只要有一处清输入队列的
动作（instant prompt、`zle` 复位、`stty`），**那条命令就永远不会被执行**，
不是「晚一点执行」。表现就是 issue #16 抓到的形状：只读到回显，10s 内等不到结果，
机器一忙就更容易撞上，所以它是间歇的。

真人不会这么用 —— 人是看见提示符才敲的，测试也照做（`Output::wait_prompt`），
外加一层「写不中就再写一遍」的兜底（`send_until`，最多 20 次、每次等 1s）。
**重写不会把「cwd 错了」磨绿**：断言的是输出里有没有那个路径，真错的话
写多少遍都等不到。

## 走网络的 git：三条硬约束的答案

M7（拉取推送）加了 `crates/gitsvc/src/{progress,remote}.rs`。三条判据：

**一、凭据一个开关都不用动。** issue #8 里写的两难（`GIT_TERMINAL_PROMPT=0`
留着就拿不到凭据、去掉就静默挂死）**是假的** —— 那三个开关是**有次序**的：

```text
① credential.helper（钥匙串）→ ② GIT_ASKPASS → ③ 回退到终端提问
```

`=0` 关掉的是 ③，而 ② 在它前面。实测（2026-09-05，模拟从访达启动的精简环境）：
`GIT_TERMINAL_PROMPT=0` 的 fetch **退出码 0**。`SSH_AUTH_SOCK` 在 launchd 里，
和终端是同一个 socket。

② 还没做 —— 它服务的是「第一次连一个新远程」，VS Code 和 IDEA 都是生成一个
临时脚本让它 IPC 回来弹框。**口子留在 `git_cmd` 上**：加的时候多设一个
`GIT_ASKPASS` 环境变量就行，`remote.rs` 一行不用动。

**二、进度是 `\r` 分隔的，不是 `\n`。** 同一份真实 stderr：按 `\n` 分得到
**7 段**（全在结尾），按 `\r` 分得到 **411 段**。**按行读等于读不到进度** ——
进度条会一直 0% 然后突然 100%。`progress.rs` 两个都切，而且缓冲区设了闸
（单段实测最长 86 字节，但那不是保证）。

推给前端之前还要**节流**：411 段是本地 clone 1248 个对象的量，真去网上拉是
几千段。判据是「阶段变了或收尾了 → 立刻推；同阶段的百分比 → 100ms 一次」。
**阶段变了必须放行**，被吃掉的话界面会停在上一阶段的 100% 上，看着像卡死。

**三、不设超时，靠取消。** 主流（VS Code / IDEA）都不给 git 设超时。
取消靠一个看门线程 kill —— **不能在读循环里判 flag**，`read()` 是阻塞的，
连接阶段可能几秒钟一个字节都没有。

抄 `ptysvc` 的两条教训：`stdout` 设 `Stdio::null()`（留一个没人读的管道
就是那个挂点的形状）、收尸用有上限的 `try_wait()` 轮询而不是 `wait()`。

**杀的是进程组，不是子进程。** `git fetch` 会派生 `git-remote-https` 当孙进程，
而**孙子攥着 stderr 管道** —— 只 kill 直接子进程的话，读循环等不到 EOF，
只能干等 TCP 超时。用 `process_group(0)` 把它放进自己的组，
取消时 `killpg` 一锅端。

数字（同一份代码，只差杀不杀组）：

| | 本地 | CI |
|---|---|---|
| 只 kill 子进程 | **5.15s** | **75s** |
| 杀整个进程组 | **0.22s** | — |

**这个 bug 是 CI 抓到的，本地漏了** —— 因为那条测试的阈值当初卡的是 10s，
而本地 5.15s 照样绿。阈值现在收到 **2s**：修好之后是 0.22s，留了近十倍余量，
同时把 5s 那档挡在外面。

顺带一条测试卫生：那条测试原来用固定的目录名，而**失败时会在清理之前 panic**，
于是残留目录让下一次 `git clone` 立刻失败（0.06s）——
看起来像「取消变快了」，**一个测完全测错了东西还绿着的形状**。
目录名现在带 pid + 时间戳，而且开跑前先清一次。

## 「操作 id 由谁发」——发错了，取消按钮永远点不动

`git_fetch` / `git_push` 的 `op_id` **是前端给的**，不是 Rust 生成后
跟着返回值给出去的。

反过来写过一版，编译过、类型对、界面看着也对，但那个取消按钮
**一次都点不动**：返回值要等操作跑完才到前端，而那时已经没什么可取消的了。

```js
const id = await gitFetch(...);   // ← 跑完才回来
syncing = { ...syncing, id };     // ← 太晚了
```

**这个 bug 只能靠点一下发现。** 是在浏览器里点了取消、发现 `git_cancel`
压根没进 invoke 才抓到的 —— 类型检查、单测、编译一个都拦不住。

同一族的判据：**凡是「先开始、后返回句柄」的异步操作，句柄必须在开始之前
就定下来。** 谁发号不重要，重要的是发号早于用号。

## git 的短名会吃掉 `/HEAD`

`refs/remotes/origin/HEAD` 的 `%(refname:short)` 是 **`origin`**，不是 `origin/HEAD`。
过滤这类引用要按**全名**判。同理 `git switch origin/foo` 会直接失败 ——
DWIM 只对短名生效，传全名要走 `--track`。

---
