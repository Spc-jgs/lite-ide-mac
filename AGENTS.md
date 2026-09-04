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
可能差好几天 —— 而双击启动的正是后者。要更新它：`pnpm app:bundle`。

**报上来的 bug 复现不了时，先确认对方跑的是哪个构建**：标题栏悬停应用名会显示
构建时间。已经发生过一次「照着现象查了半天代码，最后发现 bug 早就修好了」。

**不要往 `~/Applications` 复制一份。** 试过，结果是 Spotlight 里出现两个
同名 `lite-ide.app`，而它们只要有一次「打包了没重装」就分叉 ——
从 Spotlight 点错的那次，调的是几天前的构建。盘上只留一份，就没有点错的可能。

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
一行业务逻辑都不写。业务全在 `crates/` 里 —— 那五个 crate **不依赖 Tauri**，
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

### 改盘的 std API 默认都会吃掉已有文件

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

### 删除只走废纸篓

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

### 子进程输出必须设闸

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

**这条规矩自己漏过两处**，都是 2026-09-03 补的 —— 立了规矩不等于全仓库都照做了：

| 漏的地方 | 数字 |
|---|---|
| `searchsvc::grep_rg` 用 `.output()` 全缓冲 | 本仓库搜一个 `e`：读完 5,663,558 字节，而读到第 60 条命中只要 **18,568** |
| `fsservice::read_text_detect` 用 `fs::read` | 无上限。`probe_path` 的体积判定只在首次打开时把关，三条重读路径全绕过它 |

顺带一条**同形状但更阴**的：给读取设上限时，要读 `cap + 1` 字节。
只读 `cap` 的话，超限的文件会**静默返回截断后的内容** —— 而调用方
看到的是 `Ok`。报错比悄悄少半截安全得多。

### 过 IPC 的 DTO 靠测试卡住，不是靠自觉

`src-tauri/tests/dto_sync.rs` 解析 `commands.rs` 的 `#[derive(serde::Serialize)]`
结构体和 `commands.ts` 的 `export interface`，逐字段比，并强制每个 DTO 都带
`#[serde(rename_all = "camelCase")]`。

新加一个过 IPC 的 DTO 必须在它的 `PAIRS` 表里登记一行 —— 忘了就红。
（ARCHITECTURE.md 曾经宣称这件事由 `ts-rs` 做，那套东西从来没落地过。）

### 新代码加在测试模块**之前**

用 `cat >>` 往文件尾追加过一次，结果生产代码落到了 `#[cfg(test)] mod tests` 后面，
读文件的人会以为测试模块之后就没东西了。

### ptysvc 的测试曾间歇性挂住（**根因已定位并修掉**：`child.wait()` 收不到尸）

**先说现在的状态**：根因在 2026-08-31 修掉了，`TRIES` 已经是 **1**（不再重试），
测试体外面只剩 `with_deadline(25, …)` 这道硬期限 —— 它的作用不是撞运气，
是让挂住时 25s 内失败，而不是安静吃光 CI 整个 job 的额度。

**下面整节是排查过程的留档**，其中带删除线的是当时得出、后来被推翻的结论。
读的时候按「结论在最后」看，不要照着中间任何一段改代码。
（这一节曾经自相矛盾：开头写着「最多试三次」，一百行后又写「重试去掉了」，
而代码里是 1 —— 2026-09-04 修正。）

**2026-08-28 复测，两个数字都变了：**

| | 当时写的 | 2026-08-28 量的 |
|---|---|---|
| 加重试后的成功率 | 连跑 8 次全绿 | **连跑 3 次挂 1 次**（三次重试全超时，白烧 75s） |
| 纯 Python `pty.fork()` 复现 | 一样会挂 → 判定与本仓库无关 | **`/usr` `/tmp` 临时目录 `$HOME` 各 10 次，40/40 全过** |

**2026-08-31 又量了一次，12 轮，这次把口径分清楚了**（macOS 26.6.2 / zsh 5.9）：

| 耗时 | 轮数 | 含义 |
|---|---|---|
| 0.5s | 7 | 第一次尝试就成功 |
| 25.4s | 2 | **第一次挂满 25s，第二次成功** |
| 75.2s | 3 | 三次全挂 → FAILED |

**每轮失败率 25%（3/12），每次尝试的挂率 55%（11/20）。**
引用时务必说清是哪个口径 —— 两个数差一倍多，而以前 AGENTS.md 记 33%、
`lib.rs` 的注释记 50%，很可能就是各记了一个口径，看着像互相矛盾。

被重试救回来的那两轮是新证据：同一个测试二进制里前一次挂满 deadline、
紧接着下一次 0.4s 就过 —— 这是**竞态**，不是「前置条件坏了就一直坏」。

还有一条：**11 次挂住的尝试，新增孤儿 `zsh -l` 0 个**。
`with_deadline` panic 时提示「先看 ps 有没有堆积的孤儿」，至少今天是条空线索。

顺带量的：`zsh -l -c pwd` 0.084s、`zsh -l -i -c pwd` 0.183s（各 10 次取中位，
与 2026-08-28 的 0.07 / 0.17 基本一致）—— 登录 shell 不慢。

## 2026-08-31：探针抓到现场，根因定位到了

探针改成**边跑边往文件里追加并 flush**（原来攒着最后 `eprintln!`，
而超时那个线程被丢掉了，攒的东西跟着一起没 —— 挂住的那次恰恰是唯一
有诊断价值的一次）。开关是 `PTYSVC_PROBE`：

```bash
PTYSVC_PROBE=/tmp/pty.log cargo test -p ptysvc --lib -- 工作目录生效
```

第一次跑就抓到了，三次尝试完全一致：

```text
    0.4ms  spawn 前
    3.9ms  spawn 回来了            ← spawn 没问题，每次都 4ms
  196.2ms  read_until 回来了       ← 读也没问题
  196.3ms  断言过了，开始 drop
  411.7ms  child.kill() 回来了     ← kill 回来了
           （child.wait() 一次都没回来）
25001.3ms  第 1/3 次尝试超时
```

**挂点是 `Drop` → `kill()` → `child.wait()`。** 因果链：

1. `read_until` 返回后，它的读线程在下一次 `read` 醒来时因为 `tx.send`
   失败而退出 —— **没人再排空 pty master**
2. `child.kill()` 先发 SIGHUP，5×50ms 宽限期轮询 `try_wait()`（正好是
   实测的 215ms），都没死才补 SIGKILL
3. 退出中的 shell 继续往 tty 写，master 缓冲区满且无人排空，卡在写上收不了尾
   （`ps` 显示 `?Es`、命令名带括号）
4. `child.wait()` 于是永远等不到

**A/B 对照坐实了第 1 步**：kill 期间另起线程持续排空 master，
15 次尝试挂 6 次 → **10 次尝试挂 0 次**，连一次 25s 的重试都没有。

### 两条被推翻的旧结论

**一、「`kill()` 发 SIGKILL」——错，是 SIGHUP。** portable-pty 0.9.0：

```rust
// On unix, we send the SIGHUP signal instead of trying to kill
libc::kill(self.id() as i32, libc::SIGHUP)
```

「shell 即使 trap 掉 HUP 也照样死」这个前提不成立，而排查表里
「`child.wait()` 阻塞 → 否」正是建立在它上面的 —— 挂点恰恰就是它。

**二、「纯 Python `pty.fork()` 复现也会挂 → 根因不在本仓库」——已作废**
（2026-08-28 复测 40/40 全过）。现在也说得通了：那份复现多半一直在读 master。

### 生产侧是同一个形状，而且更糟

`commands.rs` 的 pty 读线程也是 `on_data.send(..).is_err() → break`，
而 `send` 失败正是前端关掉终端标签的时候，紧接着就调 `pty_kill`。
更糟的是 `state.rs` 的 `kill_pty` **持着整张 pty 表的锁**就地析构 ——
`wait()` 一挂，那把锁再也不放，之后所有终端操作全部堵死。

### 修复（2026-08-31，同一天）

两处，都在 `Session::kill()` 和 `state.rs`：

1. **杀之前先接上排空线程**（`kill()` 里 `try_clone_reader` 起一个短命线程读到
   EOF）。这就是上面 A/B 里验证过的那条。**不 join 它** —— 万一有孙子进程
   攥着 slave 不放，EOF 就不会来，join 本身会变成第二个挂点。
2. **收尸有界**：`wait()` 换成 5s 上限的 `try_wait()` 轮询。宁可留一个僵尸到
   进程退出，也不能让界面永久卡死 —— 前者用户察觉不到，后者要重启应用。
3. **`kill_pty` / `kill_all_ptys` 先摘出来再在锁外析构**。原来
   `ptys.lock()….remove(&id).is_some()` 的临时值析构发生在语句末尾，
   那时 MutexGuard 还活着，于是整个 `kill()` 跑在全局锁里。

### 重试去掉了（`TRIES` 3 → 1）

根因修掉之后再留着重试，就是在盖住回归 —— 它当初正是把每次尝试 55% 的挂率
盖成了每轮 25% 的可见失败率，两个数还各自进了两处文档，看着像互相矛盾。
**硬期限留着**：挂住时让测试在 25s 内失败，而不是安静吃光 CI 整个 job 的额度。

### 验证

回归测试 `关掉不排空的终端不能把kill卡死`：读到提示符后**故意不再排空**
（复现前端关标签那条路），再灌 `seq 1 100000` 填满缓冲区，然后断言 drop
在 2s 内返回。阈值卡 2s 不是 8s：两道措施任一条都能让它不挂，
只断言「最终返回」的话删掉排空线程照样绿，而那时 kill 要等满 5s 兜底 deadline
（实测有排空 55ms、无排空 5214ms）。

| | 原始 `kill()` | 修复后 |
|---|---|---|
| 回归测试单跑 ×10 | **10 红** | 0 红 |
| `工作目录生效` 单跑 ×10（TRIES=1） | 6 绿 / **4 次挂满 deadline** | **10 绿**，最大 1.3s |
| 全部 4 条并行 ×15（TRIES=1） | — | **0 失败** |

**灌注必须有界。** 第一版用 `yes`，无限流让排空线程全速空转烧掉一个核，
把并行跑的 `工作目录生效` 拖到 10s 读超时上红了 8/15 次 —— 一条会拖垮
兄弟测试的测试，和不稳定的测试一样坏。

详见 [issue #2](https://github.com/Spc-jgs/lite-ide-mac/issues/2)。

排查记录（省得下一个人重走一遍）：

| 怀疑过 | 结论 |
|---|---|
| `child.wait()` 阻塞 | ~~否~~ **就是它**。2026-08-31 探针抓到，见本节开头。当初那条排除建立在一个错事实上：portable-pty 的 `kill()` 发的是 **SIGHUP** 不是 SIGKILL |
| 登录 shell 启动慢 | 否。`zsh -l -c pwd` 在 /usr 和 /tmp 各 20 次，全是 0.0x 秒 |
| 并发跑测试 | 否。串行（`--test-threads=1`）失败率**更高**，6 次挂 5 次 |
| 孤儿 `zsh -l` 干扰 | 否。清干净之后照样挂 |

~~决定性的一条：用纯 Python 的 `pty.fork()` 写同样的复现（完全不碰这个 crate），
一样会挂。所以问题在「交互式 shell 挂在 pty 上」这件事本身，跟 lite-ide 无关。~~
**这条 2026-08-28 复测没复现出来（40/40 全过），已作废** —— 见本节开头的表。
上面那张「怀疑过」的表里的结论也都是那次排查得出的，同样需要重新验证。

**教训**：我提交过一版基于错误诊断的「修复」（把 `wait()` 改成非阻塞 + 后台收尸），
改完看着好了 —— 但把旧写法放回去也一样不挂，说明那次只是没触发。
写的回归测试在新旧两版下都通过，等于没测。**没复现出根因之前，
不要提交猜出来的修复；不会失败的测试比没有测试更糟。**

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

### effect 的依赖集是**第一次跑**时读出来的

```js
$effect(() => {
  if (!host || view) return;      // ← 只有这两个是「守卫」
  view = new EditorView({ state: build(initial), parent: host });
  baseText = baseline ?? initial;  // ← path / baseline / initial / build() 里的
});                                //   showMinimap 全都进了依赖集
```

写这段的人想的是「只在挂载时做一次」，但 effect 不认这个意图 ——
它记下第一次实际读过的每一个信号。于是 `baseline` 一变（保存成功把磁盘那份
换成新的），cleanup 就跑，`view.destroy()`，**整个编辑器被重建**。

表现是：**⌘S 之后光标跳回文件开头、撤销栈整个清空**。实测光标 120 → 0，
`view` 换了实例，⌘Z 撤不回刚才那次修改。而旁边「保存成功」那条 effect 的
注释一直写着「不换 state，光标与撤销栈都保住」—— 它自己没错，是被连累的。

**要真的「只做一次」，就得把读 prop 的那几行套进 `untrack`**，只留下真正
该触发重建的那个（这里是 `host`）。

同一个 bug 有第二层：另一条 effect 会 `view.setState(build(text))`，
`view` 是同一个但 state 被换掉了 —— 光标和撤销栈照样没。**两处都要堵**，
只堵一处的现象和没堵一模一样。判据是「文本真变了才换 state」。

### 累计计数器当 prop + `{#key}` 重建 = 挂载当场触发一次

`savedTick` 是 App 上只增不减的计数。组件被 `{#key active.id}` 包着，
切标签就是全新实例，而这条 effect 判的是「非零」：

```js
$effect(() => {
  savedTick;
  if (!view || savedTick === 0) return;   // ← 这个会话里存过一次，它就永远非零
  onChange(false);                         // ← 于是每次挂载都当成「刚保存过」
});
```

结果：**只要保存过一次，带未保存改动的标签切走再切回来，圆点就没了**
（字还在，但 ⌘W 不再拦你）—— 和 M25 修的那个形状一模一样。
判据要跟**上次见到的值**比，并且在挂载时先对齐一次。

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

**CI 会卡这条线**：入口包超过 150 KB 直接失败。本地想先看一眼：

```bash
pnpm build && ls -l dist/assets/$(grep -o 'assets/[^"]*\.js' dist/index.html | head -1 | cut -d/ -f2)
```

（**当前 125 KB，红线 150 KB**；超过 138 KB CI 会先告警。
这个数字每轮都要重量一次 —— 它在 M20/M22/M24/M25 里从 126 一路涨到 157，
而 README 和这里各记了一个旧值，看着像互相矛盾。
崩溃屏 `Crash.svelte` 是刻意静态引入的 —— 需要它的时候，
正是模块加载可能已经不可信的时候。）

**光看总数不够，要知道是谁在里面。** 开 sourcemap build 一次，
按 mappings 把字节归到源文件上，比盯着一个总数有用得多 ——
2026-09-03 那次就是这么找到两块该出去的：

| | 归因字节 | 怎么处理 |
|---|---|---|
| `editor/langs.ts` | 23,419 | 一半是 500 行 `loadLang` 的 switch 和 67 个 `import()` 桩，**只有编辑器用，而编辑器是懒的**。拆成 `langs.ts`（识别，入口要）+ `langs-load.ts`（加载，跟着编辑器走）→ **−11.8 KB** |
| `@tauri-apps/api` 的 webview/window/dpi | 15,572 | 静态引一个 `getCurrentWebview` 就把一整串拽进来了，而它只服务一个拖放监听。改成动态 `import()` → **−17.3 KB** |

判据是同一条：**入口包是首屏之前必须解析执行完的那一段**。
问一句「这东西在窗口出现之前有用吗」，没用就该出去。

### 窗口是半透明的，所以「填个底色」不再是安全操作

2026-09-04 起，窗口后面挂着一块 `NSVisualEffectView`（`Sidebar` 材质、
`BehindWindow` 混合），由 WindowServer 把桌面模糊后透上来。这不是配色，
是**材质**，webview 里的 `backdrop-filter` 顶不上 —— 它只能模糊页面自己的内容，
桌面在 webview 之外，写多少 blur 都一动不动。

于是 CSS 里每一处 `background:` 都要先回答一个问题：**这块面在第几层？**

| 层 | 谁 | 用什么 |
|---|---|---|
| 外壳 | 标题栏 · 竖条 · 侧边栏 · 标签栏 · 面板头 · 工具条 · 状态栏 | `--panel-bg`（transparent）/ `--chrome-scrim` |
| 内容 | 编辑器 · 日志 · 差异 · 终端 | `--content-bg`（透 6%） |
| 交互态 | hover / selected / pressed | `--hover` / `--selected` / `--pressed`，**一律白叠加** |
| 浮层 | 菜单 · 弹窗 · 输入框 · 徽章 | `--elevated` / `--elevated-hi`，**不透明** + `--shadow-pop` |

四条会咬人的：

1. **不透明色不能当 hover 用。** 原来 40 处写的是
   `:hover { background: var(--panel-bg-2) }`（`#212121` 实色）——
   在玻璃上那是**凿一个洞**，一块生硬的矩形浮在表面上。
   `--panel-bg-2` 因此被拆掉了（hover 归 `--hover`，浮层归 `--elevated`），
   **不留别名** —— 留一个 0 引用的别名就是下一个 `--surface-3`
   （那个 token 定义了从来没人用，2026-09-04 一起删了）。

2. **浮层必须不透明。** 这条最反直觉：菜单做成半透明的话，
   桌面在 webview 之外，`backdrop-filter` 模糊不到它 —— 壁纸会**清晰地**
   穿过菜单，字直接糊掉。「实心卡片摞在玻璃上」才是对的观感，
   抬起靠 `--shadow-pop` 不靠透明。

3. **要「挡住底下滚过去的东西」的地方用 `--content-solid`，不是 `--content-bg`。**
   CM6 的行号栏、差异视图那根吸在左边的行号列，背景不是装饰是遮挡。
   DiffView 里那段注释早就写着「背景必须不透明」，而它依赖的正是
   `--editor-bg` 曾经是实色 —— 内容层一透，这个依赖就断了，
   表现是横向滚动时正文从行号底下透出来。

4. **内容层只画一次。** 两层半透明叠起来，6% 透光被压成 0.36%，
   于是编辑器比旁边的日志视图明显"更实"，两块内容区的色调对不上。
   编辑器的底画在 `Editor.svelte` 的 `.editor` 上，CM6 主题里的 `BG`
   已经改成 `transparent`。

配套的三处，动了要一起动：

- `tauri.conf.json` 的 `transparent: true` 和 `Cargo.toml` 的
  `tauri = { features = ["macos-private-api"] }` **必须成对**。
  只写配置不加 feature，`build.rs` 直接把构建拦下来
  （报的是「features 与 allowlist 不匹配」，不是「窗口不透明」，
  第一次撞上会往错的方向查）。
- `tauri.conf.json` **不能再有 `backgroundColor`**。它原来是 `#1E1F22`
  （上上版的 IDEA 灰），透明窗口下就是糊在玻璃前面的一层实色。
- `html, body` 必须是 `transparent`。body 有一点不透明的底，
  NSVisualEffectView 就整块被盖住 —— 表现是「vibrancy 好像没生效」，
  而 Rust 侧一切正常，**从那头是查不出来的**。

### 界面写法定死了，新界面照这套来

2026-09-04 把 Git 栏、标签栏、随处搜索、分支面板四处统一到一套做法。
**新加任何界面先读这一节**，不要另起一套 —— 下面每一条都是从那四处里
挑出来的、真的出过问题的地方。

#### 一、「当前项」只有一种长相：内缩的圆角块

文件树、大纲、随处搜索、分支面板、标签栏，全部是
`margin: 0 6px` + `--r-md` + `--selected`。**不许用通栏色条，也不许用下划线。**

标签栏原来是 IDEA 那套（平角 + 底下 2px accent 线），换掉有两个理由：
一是那条线在半透明的外壳上飘着，二是标签栏和文件树紧挨着，
「当前项」在两边长得不一样，人会以为它们是两种东西。

#### 二、不要用分隔线表达「这是一格」

八个标签就是八道 1px 竖线 —— 那是**表格**的语言。当前项已经由一块底色
说清楚了，再画线就是同一件事说两遍，而线是常驻的、底色只有一块。

线只用来分**区**（标题栏／内容／状态栏之间），不用来分**项**。

#### 三、常驻的只留「不看会出错」的那些

判据就这一句。落到那四处：

| 常驻 | hover 才出 |
|---|---|
| 未保存改动的圆点（关掉要问你） | 标签的 ✕ |
| git 状态字母 M/A/?/! | 改动行的 ＋ − ↺ |
| 分支名、改动条数 | 「⋯」里那些 |

**圆点和 ✕ 占同一个格子**，hover 时原地互换 —— 点击目标不许跳位置。

#### 四、分组头要吸顶，而且每行还得能自证类别

分组头 `position: sticky; top: 0`，底色跟着所在层走
（外壳层用 `--panel-bg`，浮层用 `--elevated`；填死色会在滚动时拖出一条实心带）。

**但吸顶不够。** 分支面板最长几十行，滚起来只看得见一个分组头 ——
所以每行还要有自己的图标（本地分支／工作树／远程／操作四个形状）。
原来四种条目共用一个 9px 空心圆，只靠边框颜色区分，12px 的行里那点色差读不出来。

#### 五、浮层的骨架是固定的

搜索和分支面板现在是同一副骨架，以后的浮层照抄：

```
输入（16px 图标 + 15px 字，排第一）
范围／过滤（分段控件，紧贴输入）
分组结果（吸顶头 + 内缩圆角行 + 每行图标）
脚栏（键位提示，--chrome-scrim 打底）
```

**输入排第一**：面板打开之后的下一个动作永远是打字，
而范围十次里有九次不用改。键位提示是「忘了才看」的东西，放脚栏 ——
它顺带给了面板一个下边界，不然结果列表是直接切在圆角上的。

脚栏的 `↵` 说明**按当前项变**（分支说"切换"、远程分支说"检出"、工作树说"打开"）。

#### 六、同一个信息只印一遍

Git 栏原来「工作区干净」印了两遍（折叠的提交按钮里一遍、列表空态里一遍）。
随处搜索原来每行都印一个「文件」「操作」胶囊 —— 七行结果七个胶囊，
而分组头本来就说清了。

**空态要给下一步，不是给句号。** 干净时唯一还能做的事是看历史，就把它放出来。

#### 七、不可撤销的动作不许和可逆的并排同色

「全部丢弃」原来和「全部暂存」并排、同样大小、同样颜色，中间隔 2px。
现在它进了「⋯」菜单并带 `danger`。**误点的代价不该只隔着 2px。**

#### 八、图标只有一处定义

- `Icon.svelte` —— 界面图标（侧边栏、刷新、勾、加号、箭头…）
- `FileGlyph.svelte` —— 文件类型字形（目录／代码／文档／配置／纯文本）

`FileGlyph` 是 2026-09-04 从 `FileTree.svelte` 里收出来的，因为标签栏和
随处搜索也要用：**同一个文件在三个地方出现，不能长三个样。**
文件夹那一个形状走 `Icon` 的 `files`，因为导轨上画的是同一样东西。

#### 九、`direction: rtl` 只给路径，别的一概不给

长路径要从**左边**省略（有用的是尾巴），靠的是 `direction: rtl`。
但这个类曾经被快捷键共用，而 `⌘`(U+2318) 在 bidi 里是**中性字符** ——
RTL 段落里它跟着段落方向走，于是源码里的 `⌘1` 在界面上显示成 `1⌘`、
``⌃⇧` `` 显示成 `` `⇧⌃ ``。

**这类 bug 不报错、不崩溃**，而且看源码看不出来（源码一个字没错）。
快捷键、徽章、计数一律不带 `direction`。

### 透明窗口没有退路，所以有一个 `data-shell` 开关

`transparent: true` 之后窗口自己不画底。材质层没挂上（或者根本不在 Tauri 里跑），
外壳层留空透出来的就是**空**。而改 UI 的主循环恰恰是浏览器里的 `pnpm dev`，
那里一块 NSVisualEffectView 都没有。

`main.ts` 在 mount **之前同步**打一个 `data-shell`：

```js
document.documentElement.dataset.shell =
  "__TAURI_INTERNALS__" in window ? "tauri" : "web";
```

用这个判据而不是「深色还是浅色」，是因为要问的其实是「这个壳有没有材质层」。
用 `__TAURI_INTERNALS__` 而不是 `invoke` 一次，是因为它**同步、零成本**——
这行在 mount 之前跑，晚一帧就是一帧的白闪。
macOS 上 `apply_vibrancy` 万一失败，`lib.rs` 会 `eval` 把它打回 `web`。

回落那组值不是随便填的：**浏览器里调好的层级，装进 .app 里不能反过来。**
踩过一次 —— `--chrome-scrim` 在 web 下留成 `transparent`，标题栏就露出 body 的底，
而 body 是内容色，于是浏览器里标题栏比侧边栏**浅**，
装进 .app 里（材质 + 黑 20% scrim）又比侧边栏**深**，层级整个倒过来。

### CSP 写在两个地方，改一处要改两处

`src-tauri/tauri.conf.json` 的 `app.security.csp` 和 `vite.config.ts` 的
`server.headers`。后者是为了让 `pnpm dev` 也跑在同一条 CSP 下 —— 理由和
mock-ipc 一样：**CSP 挡下东西不会报错**，只表现为「某处不好使了」，
要是只有打包后的壳带 CSP，这类问题得等 45 秒的 Tauri 构建才撞得上。

两条不能动的：

- `style-src` 必须留 `'unsafe-inline'` —— CM6 和 xterm 都在运行时往 head 里
  插 `<style>`，去掉这条编辑器和终端直接白给。
- `main.ts` 里那个 `securitypolicyviolation` 监听不能删，它是唯一的线索来源。

改完 CSP 必须实测：起 `pnpm dev`，**开一个文件（CM6）+ 开终端（xterm）**，
看控制台有没有违规。只看首屏渲染正常是不够的，那两个组件是懒加载的。

### 异步 effect 里 `await` 之后要检查自己是不是已经被清理了

```js
$effect(() => {
  let tick = null;
  const timer = setTimeout(async () => {
    await something();          // ← cleanup 可能正好在这中间跑
    tick = setInterval(...);    // ← 装出一个再也没人清的轮询
  }, 180);
  return () => { clearTimeout(timer); if (tick) clearInterval(tick); };
});
```

cleanup 只能清掉它**当时看得见**的东西。`await` 回来时那次 cleanup 早过去了，
`tick` 是在它身后才被赋值的。加一个 `let dead = false`，cleanup 里置位，
每个 await 之后先判它。

（`LogPane` 的过滤轮询踩过：在 1GB 文件上连打十个字 = 十个 80ms 的轮询一起烧 IPC。）

### `{#key}` 重建组件之前，组件里的状态要先交回来

编辑器是 `{#key active.id}` 包着的 —— 切标签就**销毁重建**，而它只往外报
一个 `onChange(dirty: boolean)`，实时文本从来没回写过。于是：

> 在 A 里打几个字 → 切到 B → 切回 A：**字没了，标签上那个「有未保存改动」的
> 圆点也一起没了**。人完全不会察觉自己丢了东西。

（圆点也没的原因更要命：重建时新实例拿 `initial` 当 dirty 基线，而 `initial`
就是那份没更新过的旧内容，一比相等，于是报「不脏」。）

两条：

1. **销毁前把内部状态交出去。** `Editor` 在 mount effect 的 cleanup 里调
   `onStash(path, text)`。
2. **交出去的时候要带上自己的身份。** cleanup 跑在切标签**之后**，
   这时候读 `path` 这个 prop 拿到的是**新标签**的值 —— 组件必须在挂载时
   快照一份 `curPath`，交的是它自己那份。写成 `active` 就是把 A 的内容
   盖到 B 头上。

还有一条连带的：**「草稿」和「磁盘那份」必须分成两个字段**
（`tab.draft` / `tab.content`）。合成一个的话基线会被草稿顶掉，
dirty 就再也算不出来了。保存成功、选「用磁盘上的」时都要把草稿清掉。

### 「只藏不卸载」要一路贯彻到最外层

底部面板那块踩过：切「终端 ↔ Git 日志」时用的是 `class:hidden`，
注释也写清了「组件一销毁 Session 就 drop，shell 直接被 kill」——
但**外面还包着一层 `{#if panel}`**，⌘J 一收起，正在跑的 gradle build 当场就没。

判据很简单：**这个组件的销毁有没有副作用？** 有的话（起了子进程、占了句柄、
连着流），它的每一层可见性条件都得是 `class:hidden`，不能是 `{#if}`。
只做对里面那一层，等于没做。

### CM6 的扩展可能「装了不生效」

`rectangularSelection()` / `crosshairCursor()` 都在扩展列表里，但少了
`EditorState.allowMultipleSelections.of(true)`，CM6 会把每次事务的选区
`asSingle()` 压成一个 —— 矩形选择、⌥ 点加光标、「选中所有匹配」全部无效。

**扩展进了列表不等于功能可用。** 这类 bug 不报错不崩溃，只是「按了没反应」，
而人第一反应是自己按错了快捷键。加完 CM6 扩展要实际按一下，
或者在页面里 dispatch 一个构造好的 state 验证。

### 启动路径上的代码一律不许抛

会话恢复读的是 localStorage 里一份**上次**写的数据 —— 版本可能是旧的、
可能被手改过、可能只写了一半。这段代码抛一次，应用就打不开，
而用户手里没有任何办法清掉那份坏数据（界面都出不来）。

`session.parse` 因此对**任何**输入都只返回 `null`，绝不抛；
尺寸类字段读回来一定要夹（一个 4000px 的侧边栏会把内容区挤没，
而拖动手柄本身就在屏幕外，拉不回来）。

### 恢复现场时，先记状态再开东西

会话恢复踩过：位置记在 `openPath()` **之后**，结果整个不生效 ——
`openPath` 一把标签加进去 `activeId` 就变了，兑现位置的 effect 当场就跑，
而那时要读的 Map 还是空的。等写进去时 `activeId` 已经不会再变，
effect 也就不会再跑第二次。

**凡是「A 触发 B 去读 C」的结构，C 必须在 A 之前就位。**

### 同一件事写在三个地方，早晚有一处少一行

「磁盘那份成了准」这件事发生在三处：保存成功、外部改动后重读、冲突时选
「用磁盘上的」。每处都要做同样的三件事（换 content、**清 draft**、清 dirty），
而重读那一处漏了清 draft —— 另外两处不但做了，还各写了一条注释说明为什么必须做。

漏掉的后果：切走再切回来，陈草稿把刚读回来的内容顶掉，还被算成「有未保存改动」。

现在只有 `state/doc.ts` 的 `settled()` 一个出口，三处 `Object.assign(tab, settled(x))`。
**测这种函数要贴到一个已经带着旧值的对象上测**：它返回的是新对象，
「没有 draft 这个键」和 `draft: undefined` 读出来都是 `undefined`，
只看返回值的话，把那一行删掉测试照样绿（第一版测试就是这么写的，验红时才发现）。

### 会话快照存草稿，判据是 stamp

`session.ts` 原来明写「不存草稿」，理由是「下次打开时它和盘上的文件谁对？
这个问题没有好答案」。2026-09-03 推翻了 —— 但推翻老决定要给出新答案，
不能只说「用户要」：

**草稿不是「另一份真相」，它就是一次没保存的编辑**，和应用开着的时候一样。
运行中本来就同时有 `content`（盘上那份）和 `draft`（编辑器里的），
跨重启只是要连 `stamp` 一起存：stamp 一致就原样恢复；对不上就走
`conflict`（运行中早就有的那套「用磁盘上的 / 保留我的」）；
草稿和盘上现在一样就丢掉。当初「没有好答案」，是因为那时还没有
`draft`/`content` 分离 —— 那是 M25 才有的。

两条实现上的硬要求：

1. **上限 + 退路。** localStorage 写满会抛，而这段代码在启动路径旁边。
   超限只丢那一份草稿；真写不下时**退一步不带草稿再存一次** ——
   不能让新功能把「上次开了哪些文件」这个老保证一起赔进去。
2. **光靠响应式 effect 存不下来。** 它订阅的是布局/标签/项目根，
   **打字不动其中任何一个**。要另外挂一个「有脏标签时定时落盘」，
   否则「改了半天没切标签也没退出」一次都不会存。

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

**「严格对齐」包括排序这类不起眼的行为。** 桩的 `list_dir` 原来按写死的顺序返回，
而 Rust 侧是「目录在前、同名不区分大小写」—— 于是新建出来的文件在浏览器里
永远吊在列表最后，而真实现会把它排到该在的位置。「新建完滚过去」那段交互
就是在这种地方白验的。

生产构建里 `import.meta.env.DEV` 为假，整个模块会被 tree-shake 掉。

### 自动化浏览器里「验不了」的那几样，要认出来

它是个**后台标签页**（`document.hidden === true`），于是：

| 你想验的 | 实际发生的 |
|---|---|
| `scrollTo({behavior:"smooth"})` | **完全不动**。`scrollTop` 一直是 0，看着像功能坏了 |
| `await requestAnimationFrame` | **永远不回调**，整个 `javascript_tool` 调用挂到 45s 超时 |
| 按钮的原生激活（`↵`） | 不触发 —— 合成事件的 `keyCode` 是 0 |
| `⇧F10` 之类带修饰键的 | 修饰键传不下去，到手时 `shiftKey === false` |

前两条一次坑掉半小时：滚动位置算得对、`scrollTo` 也调了（monkey-patch
`Element.prototype.scrollTo` 能看到参数），就是不动。
**验滚动要验「传给 scrollTo 的目标值」，不要验 `scrollTop`**；
等待一律用 `setTimeout`，不要用 rAF。

后两条见 [issue #2](https://github.com/Spc-jgs/lite-ide-mac/issues/2) 那轮的记录 ——
在它上面验不了的东西，别写成「验过了」。

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
pnpm app:bundle                           # 打包（产物在 target/release/bundle/）
pnpm check                                # 类型检查
pnpm test                                 # 前端纯函数测试（tests/*.test.ts）
cd src-tauri && cargo test --workspace    # Rust 测试
```

发版和 CI 见 [docs/RELEASE.md](docs/RELEASE.md)。

日志引擎的 bench（需要先生成样本）：

```bash
cd src-tauri
cargo run -p logengine --example gen_log --release      # 造 1GB 样本
cargo run -p logengine --example bench --release
cargo run -p logengine --example bench_filter --release
```
