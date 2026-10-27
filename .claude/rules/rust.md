---
paths:
  - "src-tauri/**"
---

# Rust 侧的规矩

下面每一条都是**已经踩过**的坑，不是预防性的规矩。

## 改前端后 Rust 侧要能感知

`src-tauri/build.rs` 里有 `cargo:rerun-if-changed=../dist`。少了它，
改完前端重新构建，产物里嵌的还是旧的。

## 进程级的资源要记着它属于哪个窗口（多窗口，2026-10-08）

`AppState` 里的日志句柄、终端、文件监听、替换扫描、任务、远程操作都登记了 `owner`（窗口 label），
窗口 `Destroyed` 时 `release_window(label)` 只收那个窗口的。**以后再加一张「活的资源」表，用 `Owned<K, V>`**
（`src/owned.rs`：owner 由表自己存，`take_owner` 一次摘出这个窗口的全部、交回来在锁外析构），在 `release_window` 里
`take_owner` 它、写它自己的收尾，`state.rs` 那条「关掉一个窗口只收它自己的东西」加一组断言。
**忘了在 `release_window` 里收，测试 `每张按窗口登记的表_关窗口时都收` 会红**（读源码比对）——
原来这条只写在这里、靠记得，六张表各手写一遍「过滤、收集、逐个删」（2026-10-10 整体审核时收成 `Owned`）。

owner 从命令参数里的 `window: tauri::Window` 取（Tauri 注入，前端不用传、也传不错）。
id 由前端发的表（远程操作）要按 `(owner, id)` 记 —— 每个窗口都从 1 数起。
⌘Q 不经过 `Destroyed`（docs/MULTIWINDOW.md 3.6），别指望在那里做退出时的收尾。

**发给前端的事件要两头都定向。** Rust 用 `emit_to(label, …)`，前端用 `listenHere`
（`ipc/commands.ts`）—— 裸的 `listen` 默认目标是 `Any`，发给别的窗口的它照收不误
（第 0 步实测：给 w-1 发的 `open-paths`，main 也开了）。该发给谁由 `windows.rs` 定：
菜单给前台，文件变动给开着那个项目的，打开文件按 `windows::route`。

## 命令层不写业务

`src-tauri/src/commands/`（按领域分文件，2026-09-24 从一个 `commands.rs` 拆开）只做三件事：解包参数、查句柄、转错误。
一行业务逻辑都不写。业务全在 `crates/` 里 —— 那几个 crate **都不依赖 Tauri**，
才能脱离 GUI 单测和跑 bench。

日志引擎是这个项目唯一的技术未知数，它必须能独立验证。别把它跟 Tauri 绑死。

## 同步命令跑在主线程上，会卡界面的活必须挪走

Tauri 的 `#[tauri::command] fn`（不带 `async`）**跑在主线程上** —— 官方文档原话：
「Commands without the async keyword are executed on the main thread」。
主线程就是 NSApplication 的事件循环，它一堵窗口就不响应：菜单点不开、拖不动、转菊花。

**判据只有一条：碰盘、碰子进程、碰网络的，一律 `commands::blocking`**（tokio 的阻塞池）。
第一版只挪「时长不可控」的那几条（钩子、检出、fetch / push、rg、落盘），理由是实测
`git status` 40ms「两帧半不值得」—— 后来两次都被打脸：09-17 合成 60k 文件的仓库 `status`
把主线程堵 81ms 且每次保存来一次（issue #12，git 读写全部挪走）；09-18 又发现 `read_text` /
`probe_path` / `list_dir` / `file_stamp` / `git_root` 还留在主线程 —— 会话恢复串行开 20 个标签，
每个三次同步 IPC，那一秒窗口不响应。「这条很快」在这台机器的热缓存上是对的，在网络卷、
冷缓存、别人的仓库上都不是。

| 谁 | 为什么 |
|---|---|
| 所有 `git_*` | 子进程；`status` 在大仓库上 80ms 且每次保存一次 |
| `read_text` / `probe_path` / `list_dir` / `file_stamp` / `write_text` / `trash_entry` | 碰盘；网络卷是另一个数量级 |
| `detect_encoding` / `create_entry` / `move_entry` / `rename_entry` / `open_log` / `log_refresh` | 同上（2026-10-10 整体审核时补挪的，原来漏在主线程上） |
| `grep_project` / `list_project_files` | 仓库多大是用户说了算 |
| `git_fetch` / `git_push` | 走网络，本来就是「几十秒」那一档 |

两条容易搞错的：

- **别用 `#[tauri::command(async)]` 去解决它。** 那个宏对同步函数生成的是
  `async_runtime::spawn`，**活还是跑在 worker 上**（`sync_threadpool` 只是个
  tracing 标签，不是 `spawn_blocking`）。而 worker 只有 2 个
  （`lib.rs::install_runtime` 把 Tauri 默认的 18 个换掉了），两条并发的 git
  就能把它占满。阻塞池才是这类活该待的地方 —— 它按需长、空闲自己收。
- **`log_stat` / `log_lines` 这类和 `pty_write` 留在主线程上**，那是有意的：它们是常数时间的内存操作
  （首屏 50 行 0.008ms），挪到 runtime 上只会多一次调度延迟。**`open_log` 和 `log_refresh` 不算**：
  一个 open + mmap，一个每 500ms stat 一次、长了重开重映射 —— 都是系统调用碰盘，网络卷上能卡几秒。
  这条原来写成「`log_*` 留在主线程」，两个碰盘的也跟着留下了，2026-10-10 整体审核时挪走
  （同一天挪的还有 `detect_encoding`、`create_entry` / `move_entry` / `rename_entry`）。

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

**这一节和「改盘的 std API」「删除只走废纸篓」三节的规矩，由 clippy 卡着**（2026-10-12，`src-tauri/clippy.toml` 的 `disallowed-methods`，
CI 只查这一条）：`Command::output / spawn / status`、`File::create`、`fs::write`、`fs::rename`、`fs::remove_file`、`fs::remove_dir_all` 直接用就报错，
报错里带着理由。有意要用的地方标 `#[allow(clippy::disallowed_methods)]`、同一行写为什么 —— 现在 34 处，都是写自己的文件、改名自己的临时文件、
只要退出码不建管道的、或者 procutil / 任务 / fetch 本身。**原来这些只写在这份文档里**，第一次拿 clippy 量就抓到 `gitsvc::discard` 直接删未跟踪文件；
三处读输出的 `.output()`（`rg --version`、两条 git 小查询）顺手改走了 procutil。

**起子进程、读它的输出，用 `crates/procutil`**（`spawn` / `run_capped`）：stdin 接空、stderr 一起来就在另一条线程里排空、
stdout 有上限（超了「杀掉」还是「读完不存」由调用方选）、丢掉就杀掉并收尸。下面几节讲的「两个管道要并发读」「读 cap+1 判截断」
「被掐掉的退出码没有意义」都在它里面、各有一条验过红的测试。**原来这些只写在这份文档里，四个模块各写一遍，
searchsvc 那份把并发读写反了** —— 读完 stdout 才读 stderr，rg 往 stderr 写满 64KB 就和我们互相等住，搜索永远不回来
（2026-10-10 整体审核时复现）。不走它的只有五种：任务（`tasksvc::Task`，长期跑、stdout + stderr 合进一根管道流式落盘）、
git fetch / push（`gitsvc::remote`，边读边解析进度、要能取消）、语言服务器（`lspsvc::Server`，2026-10-15）、AI 助手的 agent（`acpsvc::process`，2026-10-26，同语言服务器的形状：stdin 是说话的管道、一行一条消息）、数据库边车（`dbsvc::Sidecar`，2026-10-27，直接复用 `acpsvc` 的 process / conn，同一个形状）。
语言服务器的 stdin 是说话的管道、一直开着，stdout 是一条一条的消息 —— 纪律照守、答案不同：stdout 由 `conn` 的读线程读、
**单条消息**有上限（`conn::MAX_MESSAGE`，超了读掉丢弃、把「太大」和真字节数交给等它的那个请求）；stderr 另一条线程一直排空、只留尾巴；
等待线程攥着 `Child` 收尸、退了当场报；停的时候 `killpg` 整组。**服务器问我们的另起一条线程答**：在读线程上答的话，
读线程卡在写回话时没人排空服务器的 stdout，两边互相等住 —— 这里那个死锁的同一个形状（`conn` 有一条测试让答话卡 400ms、验通知照样到）。

**管道的 EOF 不等于子进程退出**（2026-10-12，读 Codex 时对出来的）：命令留下的后台进程继承了 stdout / stderr，子进程退了、管道还被它攥着。
原来 procutil 读到 EOF 才算完，等于等那个后台进程退出 —— `sleep 4 & echo hi` 实测 4.02 秒，不退就永远；入口是 `git commit` 跑仓库带来的钩子。
现在 stdout 也由一条线程读进有界通道，读的一方隔 50ms 问一次「退了没有」（**有数据一直来时也问** —— 只在没数据时问的话，后台 `yes &` 让通道永远不空，
永远发现不了子进程早退了），退出之后管道最多再等 `PIPE_GRACE`（2 秒），到点拿已经读到的、读线程撒手。不主动杀那个后台进程，但撒手之后两路都不再替它读：读线程拿到下一块就退、关掉读端，它之后再写会收到 SIGPIPE。（原来只有 stdout 这样、stderr 一直读到它退出 —— 2026-10-12 实测撒手后各写两次，写 stdout 的死了、写 stderr 的活着；统一成关掉：不读输出、却一直往里写的后台进程，不该由我们的线程和 fd 养着。）
四条测试（procutil 三条、`钩子留下后台进程不能把提交挂住`）各自验过红。正常命令没变慢：`true` 中位 845 → 738µs、1MB 输出 3.6 → 3.0ms。

**撒手要记账**（2026-10-12 code review）：撒手之后读线程还卡在 `read()` 上，各攥一个线程、一个 fd，直到那个后台进程退出或者再写一次。一直不出声的那种，读线程要一到点就退得改成带超时的 `poll`（procutil 要加 libc），没做；每撒手一条往 app.log 写一句 WARN、带当前总数（`procutil::stranded()`），线程后来退出了再减回去。

**杀掉之后不死等**（#66，2026-10-12）：丢掉 `Running`、或者 `kill()` 之后 `finish()`，原来都是「kill 再阻塞 wait」。卡在不可中断系统调用里的进程（读断线的网络卷）SIGKILL 要等那个调用返回才生效，`wait` 把阻塞池的一条线程跟着卡住。现在最多当场等 200ms（`KILL_WAIT`），等不到就交给一条全进程共用的收尸线程（照 Codex `child_reaper.rs`），退出状态按 SIGKILL 记 —— 被杀的退出码本来就没有意义。「杀了也不退」造不出来，测试把「怎么杀」做成参数、传一个什么都不做的。
收尸线程本身起不来（线程数打满）时孩子**交回给调用方**写一句 WARN，下一次再试着起（2026-10-13 code review：原来是 `OnceLock`，起不来那一次被记一辈子，之后每个孩子都被悄悄 drop、留成僵尸、日志里一个字没有）。

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

   `gitsvc::HARDENING` 给每条 git 都带上 `-c core.fsmonitor= -c diff.external= -c protocol.ext.allow=never`，
   产生差异的命令另外带 `DIFF_SAFE`（`--no-ext-diff --no-textconv`）。
   **加固放在 `git_cmd` 上而不是各个调用点** —— `--no-ext-diff` 当初只写在四个
   产生 diff 的地方里的两个上，同一条纪律写四遍就是迟早漏一遍。

   **名字任意的那几类点不着，要先查出来再关**：`filter.<任意名>.smudge`
   （检出时跑）由 `repo_filter_drivers` 查 `--local` 拿到名单再逐个 `-c` 关掉。
   只查 `--local` 是信任边界 —— 用户全局那份里躺着 `filter.lfs.*`，
   无差别关掉等于把 git-lfs 弄坏。
   **名单读不全就拒绝跑，不是少关几个**（2026-10-12）：查名单的输出设了闸，被截断或者查不出来时，
   `git_cmd` 加一个故意写错的 `-c` 键（`REFUSE_UNREADABLE_FILTERS`，错误信息就是原因），git 当场退 128。
   加固类的东西出错时要往「不做」那边倒 —— 放行等于漏关的那几个过滤器照跑仓库里写的命令。

   `ext::` 那条要注意 **git 的默认防线是仓库自己能掀掉的**：默认 `ext` 不允许，
   但仓库在自己的 `.git/config` 里写一句 `protocol.ext.allow = always` 就放开了，
   于是一条 `git fetch` 就执行任意命令。所以那句 `-c` 必须显式写上。

   四条都各有一条**验过红**的测试。但这个方式本身不收敛 ——
   它们是一个一个查出来的，下一个能让 git 执行命令的 config 项被发现之前，
   我们不知道它存在（2026-09-17 写白名单前又实测出两条黑名单外的：
   `gpg.program` + `commit.gpgsign` 在提交时执行、`merge.<x>.driver` 在合并冲突时执行）。

   **收敛的那一层是 `gitsvc::trust`（issue #24）**：开仓库前跑一条只读的
   `git config --list --local --show-origin -z`，每个键对一张**白名单**——git 只拿它当
   数据的键放行，不在名单上的一律可疑；有可疑项就进「受限」（Git 整块隐身，挂件位置一条
   「Git 未启用」，点开列出那几条键，用户点信任才启用）。信任按 config 内容记
   （`src-tauri/src/trust_store.rs`），内容一变作废。**按内容记是有意的**：Codex 对项目自带的配置按**路径**记信任，
   信任之后 `git pull` 带进来的新钩子直接生效（docs/research/codex.md 主题 4）；会执行命令的配置，「当时看过的那份」和「现在这份」不是一回事。威胁模型写在 `trust.rs` 头上：
   **`git clone` 不传输 `.git/config` 和 hooks**，会带来别人的 config 的是「整个目录拿到手」
   和「本机工具往里写」两条路。黑名单照留：它挡的是「跑了之后 git 自己会不会去执行」，
   白名单挡的是「跑不跑」。新加白名单项前先问一句：git 会不会把这个值当命令。

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

## 按字节截出来的样本，末尾会切在半个字符上

`commands::detect_encoding` 读 256KB 样本判编码。样本是按**字节数**截的，
末尾极可能正好落在一个多字节字符中间 —— 那时 `std::str::from_utf8` 会失败，
而这个失败**不是**「这不是 UTF-8」，是「这段还没读完」。

`encoding::decode` 分不清这两者，于是掉进第 3 步 chardetng 去猜，
而那一步 `allow_utf8 = false`，**猜出来的必然是别的编码**。

后果（2026-09-07 拿真 `.app` 验收时撞见）：一个 26MB、全中文的 UTF-8 日志，
边界正好落在 `，`（`ef bc 8c`）的第三个字节上，于是整份日志渲染成
`æœåŠ¡å¤„ç†å®Œæˆ`。**而这正是这个应用的主场景** —— 带中文的大日志，
撞上的概率约 2/3。行数、级别统计全对，只有正文是乱码，所以它不显眼。

样本判编码一律走 `encoding::detect_label`，它认
`Utf8Error::error_len() == None`（「结尾被切断」）。两条测试各自验过红。

**这一类的通则**：任何「读一段样本再做判断」的代码，都要先问一句
「截断会不会把判断本身弄反」。

## 删除只走废纸篓

`fsservice::move_to_trash` 是**删用户的文件**的唯一路径，**没有 `remove_file`**。
（2026-10-12 用 clippy 的 `disallowed-methods` 量这条规矩时抓到过一处违反：`gitsvc::discard` 丢弃未跟踪的文件时直接 `remove_file` / `remove_dir_all`、
错误被吞掉。已改成交给 `fsservice::move_all_to_trash`（批量，一次 AppleScript；整批失败逐个重试、报出哪几个没扔掉）。gitsvc 不依赖 fsservice，
废纸篓由调用方传进去，测试里换成只记账的 —— 「文件还在」恰好证明我们没自己删。）
它交给 Finder 去扔（`trash` crate 在 macOS 上的默认做法：起 `osascript` 让 Finder `delete`），
不是自己往 `~/.Trash` 里 rename —— Finder 的「放回原处」靠一份系统维护的元数据，
外部卷的废纸篓在卷自己的 `.Trashes` 里，同名冲突还要按 Finder 的规则改名。
判据和「.gitignore 的规则以 git 为准，所以起 git 子进程」是同一条。
（这里和 `move_to_trash` 的注释原来都写成「调 `NSFileManager` 的 `trashItemAtURL:`」—— 那是 crate 的另一种做法，不是默认的。
2026-10-12 读 crate 源码才发现。Finder 那条路的代价：可能弹一次「控制 Finder」的权限、有音效；换来「放回原处」。**依赖的默认行为读它的源码确认**，别照自己的印象写）

应用自己生成、放在应用数据目录里的东西（替换的撤销日志 `replacesvc::journal::remove`、任务输出 `tasksvc::prune`）直接删 ——
进了废纸篓照样占着那几个 GB；这时要把范围卡死在**自己生成的那几种文件名**上，别的一概不碰
（这句原来写成「应用里没有 `remove_file`」，读起来是全盘禁止，2026-10-10 加任务输出清理时改准）。

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

### pty 是唯一一个「设闸」的答案不是截断的子进程

**终端本来就该能吐很多** —— `cat` 一个 10MB 的文件、`yes` 跑一会儿，
都是正常用法。给它一条「超过 N 字节就截断」的闸，等于把终端变成一个会骗人的
终端。所以这里要的是**背压**（`ptysvc::Flow`，issue #18）：

```
读线程 → wait_room()      未确认的字节数到水位就停住
       → read()           停住 → 没人排空 pty master
       → Channel → 前端    → shell 自己被顶回去（真终端就是这么不撑爆内存的）
       ← pty_ack(n)       前端在 term.write(bytes, cb) 的**回调**里报
```

三条要记住的：

1. **ack 要在 xterm 的 write 回调里报，不是收到就报。** 要限的是
   「收到了但还没被消费」的那一段，收到就报等于没有背压。
2. **前端不回话时必须放弃背压，不能挂住。** 读线程一停就没人排空
   pty master，退出中的 shell 卡在写上，`child.wait()` 永远等不到 ——
   那正是 issue #2 那次界面永久卡死的形状。等满 `GIVE_UP`（2 秒）
   就把这个 pty 的闸永久关掉，退回没有背压的老行为，**并往 app.log
   写一条 WARN** —— 那是前端那半唯一的自证方式。
3. **不要「超时之后清零继续」**：那样每次涨到水位都停 2 秒，
   吞吐掉到 4KB/s，表现是「终端卡死了」，比没有背压糟得多。

实测（`yes`，600ms，同一台机器）：没有闸读出 **49,574,204 字节**，
有闸 **262,481 字节** —— 189 倍。那 47MB 在真实应用里是排在 webview 里
等着被 xterm 解析的队列，xterm 的 `scrollback` 管不着它。

**读两个管道必须并发，顺序读会死锁。** 这条是 2026-09-07 真的挂住之后补的：
把 `git commit` 写成「先把 stdout 读完，再顺序读 stderr」，跑起来 git 和我们
互相等着，最后是手动 kill 掉的。根因是一个反直觉的事实 ——
**git 2.50 把 pre-commit 钩子的 stdout 转到了 stderr**（实测一个 200 行的钩子：
git 的 stdout 89 字节、stderr 2892 字节）。钩子一话多就写满 stderr 那几十 KB
缓冲卡在写上，而我们在等 stdout 的 EOF，那个 EOF 要等它退出才来。
界面上的表现是「点了提交，然后什么都不再发生」。

排空 stderr 的线程原来是 `gitsvc::drain_stderr`，2026-10-10 收进了 `procutil::spawn`（一起来就挂上），
git 的 `run_capped_raw` / `drain_both`、搜索、剪贴板、lsof 都走它 —— 搜索那份原来把顺序写反了、复现过死锁。
**线程里超过上限也要继续读**，只是不再存 —— 停下来就是同一个死锁。
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
| `git commit` | `drain_both` | **截断，但不 kill 子进程**。掐掉进程会让退出码失去意义，那时「提交成功但钩子话多」和「提交失败」分不出来 —— 而把一次成功的提交报成失败，用户会照着那句话再提交一次 |

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

`src-tauri/tests/dto_sync.rs` 解析 `commands/dto.rs` 的 `#[derive(serde::Serialize)]`
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
- `commands/pty.rs` 那个 pty 读线程的 `send 失败即 break` **依赖上面那条排空线程**才不挂。
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

② 2026-10-28 做了（#62，`gitsvc::askpass`）：服务的是「第一次连一个新远程」。`GIT_ASKPASS` 指到**应用自己的可执行文件**
（`main.rs` 一开头认 `LITE_ASKPASS_SOCK`：带着它起来的不起界面，问一句、答案打到 stdout 就退），不往盘上写脚本；
通道是每次远程操作一个 Unix socket（目录 0700）+ 随机口令。问人的请求不设超时，操作结束时通道析构、还在问的那一头读到 EOF。
验收 `scripts/accept/git-auth.sh` **不碰你的钥匙串**：macOS 的 git 默认 `credential.helper=osxkeychain`（Xcode 那份系统级配置），
脚本设 `GIT_CONFIG_NOSYSTEM=1` 和临时的 `GIT_CONFIG_GLOBAL`，`open` 把这两个带给测试应用。

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

## macOS 上「打开文件」不是命令行参数（2026-09-15）

Finder 双击、拖到 Dock 图标、右键「打开方式」、`open -a lite-ide x.log` ——
全部经过 Launch Services，它给应用发一个 `odoc` Apple Event，**`argv` 里什么都没有**。
只有直接 exec 二进制才走 `argv`。原来只读 `std::env::args()`，四条路一条都不通，
而 USAGE 里还教人 `open -a lite-ide /var/log/system.log`（实测 `tabs=0`）。

Tauri 把它包成 `RunEvent::Opened { urls }`，冷启动和已在运行都走它；单实例是白捡的
（LS 发给已在运行的那个，不需要 single-instance 插件）。两条要记住：

- **事件可能比前端先到。** Finder 双击冷启动，`Opened` 在监听挂上之前就来。
  先攒在目标窗口的收件箱里（`windows.rs`，每个窗口一个），前端 `initial_paths` 取走并标记就绪；
  **前端必须先 `listen` 再调 `initial_paths`**，反过来中间那一拍就丢了。
- **`lite` 命令是 `exec open -a "<.app>" "$@"`，不能软链到二进制** —— 软链走
  `argv` 只在冷启动时有效，应用开着时会再起一个进程。

「打开方式」列表和 Dock 拖放接不接受，由 `Info.plist` 的 `CFBundleDocumentTypes`
决定（`tauri.conf.json` 的 `bundle.fileAssociations`），不是运行时能不能处理。
bundle 后 `plutil -p Info.plist` 看得到。
