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

## 数据面走二进制

日志行**不能**用默认的 `invoke`（JSON）。传 1000 行 JSON 约 15ms、二进制约 1ms，
而 60fps 单帧预算只有 16ms。用 `tauri::ipc::Response` 传 `Vec<u8>`，
线格式在 `crates/logengine/src/block.rs`。

## 起子进程时的两条硬纪律

`gitsvc` 和 `searchsvc` 都起子进程（`git` / `rg`）：

1. **绝不拼 shell 字符串**。全部走 `Command::arg`，路径前一律加 `--` ——
   否则一个叫 `-f` 的文件就能变成命令行开关。
2. **绝不让子进程卡住等输入**。`GIT_TERMINAL_PROMPT=0` 关掉凭据提问；
   `GIT_OPTIONAL_LOCKS=0` 让 `git status` 不抢 index 锁（用户正在终端里
   跑 rebase 时，后台刷新不该把它顶失败）。

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
