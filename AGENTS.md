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

### 过 IPC 的 DTO 靠测试卡住，不是靠自觉

`src-tauri/tests/dto_sync.rs` 解析 `commands.rs` 的 `#[derive(serde::Serialize)]`
结构体和 `commands.ts` 的 `export interface`，逐字段比，并强制每个 DTO 都带
`#[serde(rename_all = "camelCase")]`。

新加一个过 IPC 的 DTO 必须在它的 `PAIRS` 表里登记一行 —— 忘了就红。
（ARCHITECTURE.md 曾经宣称这件事由 `ts-rs` 做，那套东西从来没落地过。）

### 新代码加在测试模块**之前**

用 `cat >>` 往文件尾追加过一次，结果生产代码落到了 `#[cfg(test)] mod tests` 后面，
读文件的人会以为测试模块之后就没东西了。

### ptysvc 的测试会间歇性挂住（根因**未定**，别信旧结论）

`工作目录生效` 那条会卡住不返回。测试体外面套了 `with_deadline(25, …)`：
**每次尝试有硬期限，最多试三次，三次全挂才算失败**。

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

所以下面那句「根因不在本仓库」**现在没有证据支撑了**。可能是当时那次
Python 复现有别的干扰，也可能是这台机器的 shell 配置这段时间变过。
**在重新定位到根因之前，谁都别改 ptysvc 的代码** —— 理由见本节末尾那条教训。

时间戳插桩**已经做过了**，正常运行长这样（`spawn` 4.6ms、
起 shell 到吐提示符 186ms、`Drop` 54.7ms、整条链 0.19s）——
也就是说挂住时不是「慢了一点」，是**彻底不动**。

**但它从来没抓到挂住的那次**，原因在 `with_deadline` 的写法：超时之后那个线程
被丢掉了（注释里写明「故意不 join」），它攒着的 `eprintln!` 输出跟着一起没了。
每次挂住，恰恰是唯一有诊断价值的那次，什么都留不下。

**下一步是把探针改成边跑边往文件里追加并 flush**，而不是攒到最后一起打印。
挂住时文件最后一行就是它走到的最后一个阶段，一次就能分出是 spawn / read / drop。

**已经能推出一半**：`read_until` 自己的超时是 10s，外层 deadline 是 25s。
挂在读取上的话，测试会在 10s 左右**失败**（读不到 `/usr`）；
而实际观察到的是 3 × 25s = 75s，**deadline 每次都打满** ——
所以挂点大概率在 `Session::spawn` 或 `Drop`，不在读取。
**这仍然只是推理**，在探针真抓到一次失败现场之前不算数。

详见 [issue #2](https://github.com/Spc-jgs/lite-ide-mac/issues/2)。

排查记录（省得下一个人重走一遍）：

| 怀疑过 | 结论 |
|---|---|
| `child.wait()` 阻塞 | 否。portable-pty 的 `kill()` 发 SIGKILL，shell 即使 `trap '' HUP TERM INT` 也照样死，drop 只要 55ms |
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

**CI 会卡这条线**：入口包超过 160 KB 直接失败。本地想先看一眼：

```bash
pnpm build && ls -l dist/assets/$(grep -o 'assets/[^"]*\.js' dist/index.html | head -1 | cut -d/ -f2)
```

（当前 134 KB。崩溃屏 `Crash.svelte` 是刻意静态引入的 —— 需要它的时候，
正是模块加载可能已经不可信的时候。）

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

生产构建里 `import.meta.env.DEV` 为假，整个模块会被 tree-shake 掉。

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
