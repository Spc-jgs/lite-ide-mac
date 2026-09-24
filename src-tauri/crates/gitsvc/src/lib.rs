//! Git 状态与差异读取。
//!
//! # 为什么起 `git` 子进程，而不是链 libgit2 / gix
//!
//! 与 `searchsvc` 起 `rg` 是同一条路子，理由也一样：
//!
//! - **语义正确性打不赢**。`.gitignore` 的优先级规则、`core.excludesfile`、
//!   `info/exclude`、worktree、submodule、稀疏检出、rename 检测 —— 自己实现
//!   永远是在追一个移动靶。git 本身就是这些规则的定义。
//! - **体积**。libgit2 静态链进来约 2MB，整个 `.app` 现在才 4.6MB。
//! - **一定装了**。用 IDE 的人机器上没有 git 是不成立的假设；真没有时
//!   `discover()` 返回 None，界面上 Git 功能整体隐身，不报错不挡路。
//!
//! 代价是每次调用约 5–15ms 的进程启动开销。状态刷新是「窗口获得焦点时」
//! 和「动作之后」触发的，不是每帧，这个代价可以忽略。
//!
//! # 两条硬纪律
//!
//! 1. **绝不拼 shell 字符串**。全部走 `Command::arg`，且路径前一律加 `--`
//!    —— 否则一个叫 `-f` 的文件就能变成命令行开关。
//! 2. **绝不让 git 卡住等输入**。`GIT_TERMINAL_PROMPT=0` 关掉凭据提问，
//!    `GIT_OPTIONAL_LOCKS=0` 让 `status` 不去抢 index 锁（用户正在终端里
//!    跑 `git rebase` 时，我们的后台刷新不该把它顶失败 —— VSCode 同款处理）。

pub mod progress;
pub mod remote;
pub mod trust;

pub mod console;

// 按领域拆出去的（2026-09-24，纯搬家）。公开 API 一个字没变：下面 `pub use` 把它们
// 重新导出到 crate 根上，调用方照旧写 `gitsvc::status`、`gitsvc::Entry`。
mod blame;
mod branch;
mod changes;
mod commit;
mod history;
mod status;

pub use blame::*;
pub use branch::*;
pub use changes::*;
pub use commit::*;
pub use history::*;
pub use status::*;

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 一次 status 最多返回多少条。仓库处在病态状态（比如误 `git add` 了
/// node_modules）时，几十万条记录传到前端只会把界面拖死，不如明确截断。
pub const MAX_ENTRIES: usize = 5_000;

/// 展开未跟踪目录时，`git ls-files` 的输出最多收多少字节。
///
/// 一个几万文件的未跟踪目录（误建的 `node_modules`、没进 .gitignore 的
/// `target/`）能吐出好几 MB 路径，而列表最多只显示 [`MAX_ENTRIES`] 条。
/// 「跑子进程读它 stdout」一律先问一句：这东西的输出有上限吗。
const MAX_UNTRACKED_BYTES: usize = 1 << 20;

/// [`ignored_dirs`] 的输出上限。
///
/// 同一条纪律：跑子进程读它 stdout 之前先问「这东西有上限吗」。
/// `.gitignore` 逐个文件列（而不是列目录）的仓库能吐出很多条，
/// 而我们只留其中的目录。超了就用读到的那部分 —— 少跳几个目录只是多搜一点，
/// 而把内存吃穿是另一回事。
const MAX_IGNORED_BYTES: usize = 1 << 20;

/// 一条 git 命令的 stdout 最多收多少字节。
///
/// **AGENTS.md 那条「跑子进程读它 stdout，先问一句这东西的输出有上限吗」
/// 在这里漏了第三次。** 前两次是 `searchsvc::grep_rg` 和
/// `fsservice::read_text_detect`，都记在 rules/rust.md 里；这一次是
/// `run_raw` 自己 —— 它用 `.output()`，把整份 stdout 全缓冲进内存。
///
/// 走这条路的里头有两条输出真的没上界：
/// - `status`：改动文件数由仓库说了算。`MAX_ENTRIES` 只截**解析出来的条目**，
///   截不住已经读进内存的那几 MB。而它是全应用最高频的一条，还跑在主线程上。
/// - `commit`：pre-commit 钩子想打印多少打印多少（rules/rust.md 自己说
///   钩子能跑三十秒的 eslint）。
///
/// 4MB 是「正常情况下永远撞不到、病态情况下不至于把内存吃穿」那一档：
/// 本仓库 `git status` 全改动约 40KB，`for-each-ref` 全分支约 3KB。
const MAX_STDOUT_BYTES: usize = 4 << 20;

/// 一次 `git diff` 最多收多少字节。
///
/// **为什么必须有这道闸**：一个 30MB 的新增文件，`git diff` 会原样吐出 30MB。
/// 实测这份文本过一趟 JSON IPC 再在前端解析成行对象，堆占用涨到 126MB ——
/// 而界面**最多只渲染 3000 行**。为三千行付两百多兆，纯亏。
///
/// 1MB 按差异行平均 100 字节算约合一万行，仍是渲染上限的三倍多，
/// 留足了余量：正常情况下先撞上前端的 3000 行截断，这道闸根本不会触发。
pub const MAX_DIFF_BYTES: usize = 1 << 20;

/// 单个文件在工作区里的处境。
///
/// 暂存区和工作区是**两个独立的位面**：同一个文件可以「已暂存的修改」+
/// 「未暂存的新修改」同时成立。所以这里是两个字段而不是一个状态枚举。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 相对仓库根的路径
    pub path: String,
    /// 暂存区相对 HEAD 的状态：`.MADRCU` 之一
    pub index: char,
    /// 工作区相对暂存区的状态：`.MADRCU` 之一
    pub work: char,
    /// 未跟踪
    pub untracked: bool,
    /// 冲突中（unmerged）
    pub conflicted: bool,
    /// rename/copy 的来源路径
    pub orig: Option<String>,
}

impl Entry {
    /// 有没有进暂存区的改动。
    ///
    /// 冲突中的条目一律返回 false：`UU` 的 index 和 work 都是 `U`，
    /// 按字面判会让同一个文件同时出现在「已暂存」和「改动」两组里 ——
    /// 而它其实哪一组都不属于，它属于「冲突中」，得先解决完才谈得上暂存。
    pub fn staged(&self) -> bool {
        !self.conflicted && !self.untracked && self.index != '.' && self.index != ' '
    }
    /// 有没有工作区里没暂存的改动。冲突条目同样不算（见 [`Entry::staged`]）
    pub fn unstaged(&self) -> bool {
        !self.conflicted && (self.untracked || (self.work != '.' && self.work != ' '))
    }
}

/// 仓库整体状态的一次快照。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// 分支名；detached HEAD 时是短 sha，形如 `(a1b2c3d)`
    pub branch: String,
    /// 上游分支名，没设就是空
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
    /// 是否处在 detached HEAD
    pub detached: bool,
    /// HEAD 的短 sha（7 位）。一个提交都没有时是空串。草稿锚点记它（M10）
    pub head: String,
    /// 仓库里一个提交都还没有
    pub unborn: bool,
    /// **只有文件。** 未跟踪的目录见 [`Status::untracked_dirs`]
    pub entries: Vec<Entry>,
    /// 整个未跟踪的目录，路径以 `/` 结尾。
    ///
    /// `--untracked-files=normal` 把这样的目录折叠成一条记录。它**不进
    /// `entries`** —— 改动列表要回答「我改了哪些文件」，一个目录点不开差异，
    /// 也说不清里面到底多了什么；里面的文件由 [`expand_untracked_dirs`]
    /// 摊开后进 `entries`。
    ///
    /// 目录名本身还是要留给文件树：它靠这个前缀给目录自己上「未跟踪」的色，
    /// 而不是只显示「里面有东西改了」的冒泡标记。别让前端去猜末尾的斜杠。
    pub untracked_dirs: Vec<String>,
    /// 条目被 MAX_ENTRIES 截断了
    pub truncated: bool,
}

#[derive(Debug)]
pub enum Error {
    /// 机器上没有 git，或者起不来
    NoGit(io::Error),
    /// git 跑了但报错，带上 stderr —— 直接给用户看，比我们转译得准
    Git(String),
    /// 切分支被本地改动挡住了，`files` 是挡路的那些文件。
    ///
    /// **这一档单独拆出来，是因为它不是「出错了」，是「你得先决定怎么办」。**
    /// 把 git 的原话（"Please commit your changes or stash them before you
    /// switch branches"）原样丢给用户，等于让他自己去开终端 —— 而提交和丢弃
    /// 这两条路界面上都有。分类出来，前端才给得出按钮。
    ///
    /// 照 `remote::RemoteError` 那套办：**在最贴近 git 的地方分类，
    /// 别让上层去 `contains("would be overwritten")`。**
    LocalChanges { files: Vec<String>, raw: String },
    /// 提交时 pre-commit 钩子把它拒了，`output` 是钩子说的话。
    ///
    /// **git 不给这个信号**，实测（2026-09-08）它的形状是：退出码 1、
    /// stdout **一个字都没有**、stderr 全是钩子自己的输出。而 git 自己的
    /// 失败（`nothing to commit`）反过来 —— 话在 stdout 里。
    /// 所以判据是「git 自己没说话 + 仓库里真有一个可执行的 pre-commit」。
    ///
    /// 分出来是因为它和别的失败不是一回事：**代码没问题，是检查没过**，
    /// 而钩子往往打印几十上百行，原样贴出来等于什么都没说。
    HookRejected { output: String },
    /// 点了提交，但暂存区是空的。
    ///
    /// git 的原话是 `nothing to commit, working tree clean` —— 那是给命令行
    /// 用户看的。界面上「怎么办」是明确的：去勾几个文件，或者点「全部暂存」。
    NothingStaged { raw: String },
    /// 删分支时 git 拒了：这条分支上还有没合进任何地方的提交（`branch -d` 的保护）。
    ///
    /// 和 `LocalChanges` 是同一档 ——「不是出错，是你得先决定」：界面上给
    /// 「仍然删除」（走 `-D`）一条出路，而不是把 "not fully merged" 贴出来。
    /// 判据是 git 自己那句 `is not fully merged`，`LC_ALL=C` 保证它不会被翻译。
    NotMerged { raw: String },
}

impl Error {
    /// 原始的 stderr。界面上「看 git 的原话」那种展开要用。
    pub fn raw(&self) -> &str {
        match self {
            Error::NoGit(_) => "",
            Error::Git(msg) => msg,
            Error::LocalChanges { raw, .. } => raw,
            Error::HookRejected { output } => output,
            Error::NothingStaged { raw } => raw,
            Error::NotMerged { raw } => raw,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoGit(e) => write!(f, "找不到 git 命令：{e}"),
            Error::Git(msg) => write!(f, "{msg}"),
            Error::LocalChanges { files, .. } => {
                write!(f, "有 {} 个文件的本地改动挡着", files.len())
            }
            Error::HookRejected { output } => {
                /*
                 * **只留最后几行。**
                 *
                 * 钩子动辄打印上百行（跑 eslint 的那种），而要紧的话几乎总在
                 * 最后 —— 前面是进度和通过项。整段贴到横幅上，人要滚很久才
                 * 看到那一句，等于没说。
                 *
                 * 完整的那份没丢，在 `raw()` 里。
                 */
                const TAIL: usize = 12;
                let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
                let skipped = lines.len().saturating_sub(TAIL);
                writeln!(f, "提交被钩子拒绝了。代码没提交上去，改动都还在。")?;
                if skipped > 0 {
                    writeln!(f, "\n钩子说（前面还有 {skipped} 行）：")?;
                } else {
                    writeln!(f, "\n钩子说：")?;
                }
                for l in lines.iter().skip(skipped) {
                    writeln!(f, "{l}")?;
                }
                Ok(())
            }
            Error::NothingStaged { .. } => write!(
                f,
                "暂存区是空的，没有东西可提交。\n先在改动列表里勾上要提交的文件，或者点「全部暂存」。"
            ),
            Error::NotMerged { .. } => write!(f, "这条分支上还有没合并的提交"),
        }
    }
}

impl std::error::Error for Error {}

type R<T> = Result<T, Error>;

/// 仓库自带的配置里，**能让 git 去执行一条命令**的那几项，一律就地掐掉。
///
/// 这不是理论风险。`.git/config` 是仓库自己带的文件 —— 别人给你一个目录
/// （clone 下来的、解压出来的、U 盘里的），里面就可以写：
///
/// ```text
/// [core]
///     fsmonitor = /path/to/仓库里的脚本
/// ```
///
/// 而 lite-ide 在项目根一变就自动 `git_root` → `git status`（App.svelte 里
/// 那条 effect），**不需要用户多点一下**，会话恢复还让它每次启动都再跑一遍。
/// 实测：那个脚本真的被执行了；加上 `-c core.fsmonitor=` 之后不再执行。
///
/// 放在这里而不是各个调用点，是因为**已经漏过一次**：`--no-ext-diff` 当初
/// 只写在四个产生 diff 的地方里的两个上（`--no-index` 那条和 `commit_diff`
/// 的 `show` 回退没有）。同一条纪律写四遍，就是迟早漏一遍。
///
/// **这张常量表只管键名固定的那几项。** 名字是任意的那几类挡在别处，
/// 一个都不能少，所以在这里点名：
/// - `filter.<名字>.smudge` / `.clean` / `.process` —— 检出、暂存时跑。
///   名字任意，`-c` 点不着，由 [`repo_filter_drivers`] 先查出来再逐个关。
/// - `diff.<名字>.textconv` —— 看差异时跑。同样任意，靠 [`DIFF_SAFE`] 的
///   `--no-textconv` 挡。
///
/// 真正的解法是「这个目录信不信得过」那一套（VS Code 的受限模式）——
/// 上面这几条是**一个一个查出来的**，这个方式本身不收敛：
/// 下一个能让 git 执行命令的 config 项被发现之前，我们不知道它存在。
/// 那是另一件事，还没做。
///
/// # `diff.external=` 是 fail-closed 的，这是有意的
///
/// 把它设成**空串**之后，一条没带 `--no-ext-diff` 的 `git diff` 不会「照常出
/// 差异」，而是**直接失败**：git 去执行那条空命令，报
/// `error: cannot run : No such file or directory`。
///
/// 留着这个行为，不换成 `/usr/bin/true` 之类「无害的真命令」——
/// 那样忘了 [`DIFF_SAFE`] 的新入口会**悄悄拿到一份空差异**，而现在它会当场炸。
/// 宁可报一句难看的错，也不能让「忘了加参数」变成一次静默的安全回退。
/// 有一条测试钉着这件事，别把它「修」成不报错。
const HARDENING: &[&str] = &[
    "-c",
    "core.fsmonitor=",
    "-c",
    "diff.external=",
    // `remote.<名字>.url = ext::<任意命令>` —— fetch / push 时把那条命令
    // 当传输层跑起来（issue #17 的第二个口子）。
    //
    // **不能指望 git 的默认值。** 这台机器上 git 2.50 默认确实拒绝 ext:,
    // 但 `protocol.ext.allow` 是可以写在**仓库自己的 `.git/config`** 里的 ——
    // 实测：仓库里加一句 `protocol.ext.allow = always`，一条 `git fetch`
    // 就执行了仓库指定的脚本。`-c` 在子命令之前，优先级高于 `.git/config`，
    // 这一句把那条路封死。
    "-c",
    "protocol.ext.allow=never",
];

/// 产生差异的命令统一带上这两个。
///
/// `--no-ext-diff` 关 `diff.external`，`--no-textconv` 关
/// `diff.<名字>.textconv`（`.gitattributes` 里挂一句 `*.txt diff=x` 就能触发）。
/// 实测只加 `--no-ext-diff` 挡不住 textconv。
pub(crate) const DIFF_SAFE: &[&str] = &["--no-ext-diff", "--no-textconv"];

/// 仓库**自己带的** filter 驱动名（`filter.<名字>.smudge` / `.clean` / `.process`）。
///
/// # 为什么不能像别的加固那样一句 `-c` 了事
///
/// `core.fsmonitor` / `diff.external` / `protocol.ext.allow` 的键名是固定的，
/// 一句 `-c 键=` 就压过去了。而 filter 的驱动名**是任意的** ——
/// `.gitattributes` 里写 `a.txt filter=随便什么`，config 里配上同名的
/// `filter.随便什么.smudge`，一次检出就执行。点不着的键没法关。
///
/// # 为什么只看 `--local`
///
/// 这是信任边界：`.git/config` 是**仓库带来的**文件，`~/.gitconfig` 是用户自己写的。
/// 用户全局那份里躺着 `filter.lfs.*`（git-lfs），无差别关掉等于把一个合法功能
/// 弄坏。`core.fsmonitor` 那条的注释里写着「会关掉用户自己配的 fsmonitor，
/// 这个交换是有意的」—— filter 这边**不能**这么换，所以多花这一次查询。
///
/// # 为什么不自己解析 `.git/config`
///
/// 仓库 config 可以 `[include] path = 别处`，文本解析会漏掉引进来的那一份。
/// 让 git 自己展开。
fn repo_filter_drivers(cwd: &Path) -> std::sync::Arc<Vec<String>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::SystemTime;

    /*
     * 缓存的 key 带上 `.git/config` 的 mtime。
     *
     * 只按路径缓存的话，「打开仓库之后 config 才变」这件事就永远看不见 ——
     * 而测试正是这个形状（`trapped_repo` 先跑几条 git 建仓库，之后才写入
     * filter 配置）。用 mtime 当 key，config 一变自动重查，测试不需要
     * 另开一个清缓存的后门，真实场景里用户中途配了 lfs 也能被认出来。
     *
     * 拿不到 mtime 时返回 `None` 并**每次都查**（工作树的 `.git` 是文件，
     * config 在主仓库那边，这里不去追）—— 宁可慢，不能漏。
     *
     * **这张表只增不减，是认过的账。** 每开一个新仓库加一条，config 每变
     * 一次再加一条，旧的不清。一条大约几十字节（一个路径 + 一个时间戳 +
     * 一个通常是空的 Vec），开一百个仓库也就几 KB —— 比起「为了清理去猜
     * 哪条还有用」，留着更简单也更安全。真要长起来，得是有人拿脚本反复
     * 改同一个仓库的 `.git/config`，那不是这个应用的用法。
     */
    fn config_mtime(cwd: &Path) -> Option<SystemTime> {
        let dot = cwd.join(".git");
        let meta = std::fs::metadata(&dot).ok()?;
        if !meta.is_dir() {
            return None; // 工作树：config 不在这儿，退化成每次查
        }
        std::fs::metadata(dot.join("config")).ok()?.modified().ok()
    }

    static CACHE: OnceLock<Mutex<HashMap<(std::path::PathBuf, SystemTime), Arc<Vec<String>>>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);

    let key = config_mtime(cwd).map(|m| (cwd.to_path_buf(), m));
    if let Some(k) = &key {
        if let Some(v) = cache.lock().ok().and_then(|c| c.get(k).cloned()) {
            return v;
        }
    }

    // **用不带 filter 关闭的那条**，否则这里会无限递归回 git_cmd
    let out = git_cmd_hardened(cwd, &[
        "config",
        "--local",
        "--name-only",
        "--get-regexp",
        r"^filter\..*\.(smudge|clean|process)$",
    ])
    .output();

    let mut names: Vec<String> = Vec::new();
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            // `filter.<名字>.smudge` → `<名字>`。名字里可以有点（`filter.a.b.smudge`），
            // 所以从两头切，不是按点分割
            if let Some(rest) = line.strip_prefix("filter.") {
                if let Some(i) = rest.rfind('.') {
                    let n = &rest[..i];
                    if !n.is_empty() && !names.iter().any(|x| x == n) {
                        names.push(n.to_string());
                    }
                }
            }
        }
    }

    let arc = Arc::new(names);
    if let Some(k) = key {
        if let Ok(mut c) = cache.lock() {
            c.insert(k, arc.clone());
        }
    }
    arc
}

/// 只带 [`HARDENING`] 的版本。**给 [`repo_filter_drivers`] 用** ——
/// 它自己要起一条 git 来查配置，走 [`git_cmd`] 就会转回来查它自己。
fn git_cmd_hardened(cwd: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.args(HARDENING)
        .args(args)
        .current_dir(cwd)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    c
}

/// 建一条环境干净的 git 命令。所有对外的调用都必须经过这里 ——
/// 少一条 `env_remove` 或少一个 `GIT_TERMINAL_PROMPT=0`，
/// 表现就是「某个仓库上偶发地查到别处去」或者「后台调用挂着等密码」。
pub(crate) fn git_cmd(cwd: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    // `-c` 必须在子命令之前，而且优先级高于仓库自己的 .git/config
    c.args(HARDENING);
    // 仓库自己带的 filter 驱动一律关掉（issue #17 的第一个口子）。
    // 三个键都要关：smudge 检出时跑、clean 暂存时跑、process 是长驻协议版
    for name in repo_filter_drivers(cwd).iter() {
        c.arg("-c").arg(format!("filter.{name}.smudge="));
        c.arg("-c").arg(format!("filter.{name}.clean="));
        c.arg("-c").arg(format!("filter.{name}.process="));
    }
    c.args(args)
        .current_dir(cwd)
        // 不继承父进程的 GIT_DIR / GIT_WORK_TREE —— 从终端里启动 lite-ide 时，
        // 这俩环境变量可能指向另一个仓库，会让所有查询串到别处去
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        // 输出必须是稳定的英文机器格式，用户 locale 是中文时不能让 git 翻译它
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    c
}

/// 起一条线程把 stderr 排空，最多**留下** `cap` 字节。
///
/// **不能先把 stdout 读完再去读 stderr。** 那是个真的会挂住的死锁：两个管道
/// 各有几十 KB 缓冲，子进程写满 stderr 就阻塞在写上，而我们正等着 stdout 的
/// EOF —— 那个 EOF 要等子进程退出才来。
///
/// 这不是理论风险，是**实测撞上的**：git 2.50 把 pre-commit 钩子的 stdout
/// **转到了 stderr**（实测一个 200 行的钩子：stdout 89 字节、stderr 2892 字节），
/// 于是一个话多的钩子就能让 `git commit` 和我们互相等到天荒地老 ——
/// 界面上表现为「点了提交，然后什么都不再发生」。
///
/// 原来的 `.output()` 反而没这个问题：它内部就是并发读两个管道的。
/// 手写顺序读的那一刻就得把这条一起写下来。
///
/// 线程里**超过 cap 也要继续读**，只是不再存 —— 停下来就是同一个死锁。
fn drain_stderr(mut src: std::process::ChildStderr, cap: usize) -> std::thread::JoinHandle<Vec<u8>> {
    use std::io::Read;
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 8 << 10];
        loop {
            match src.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if kept.len() < cap {
                        let room = cap - kept.len();
                        kept.extend_from_slice(&chunk[..n.min(room)]);
                    }
                }
            }
        }
        kept
    })
}

/// stderr 留多少字节 —— 够拼出一句能读的报错就行
const MAX_STDERR_BYTES: usize = 8 << 10;

/// 跑一条 git 命令，返回 stdout 的原始字节。**输出有上限**（[`MAX_STDOUT_BYTES`]）。
///
/// stdout 保持 `Vec<u8>` 不转 String：路径在 git 眼里是字节串，
/// macOS 上确实可能有非 UTF-8 的文件名，提前 `from_utf8` 会在这类仓库上直接崩。
///
/// 走这条路的命令，输出都该是**由构造决定的有界量**（一个 sha、一个分支名、
/// 一份分支列表）。所以超上限时**报错而不是截断**：一份少了后半截的分支列表
/// 看着和完整的一模一样，而它会让「这个分支存不存在」这类判断悄悄给出错答案。
/// 判据同差异那条 `truncated` —— 宁可说「我读不下」，不能给一份假的完整。
///
/// 输出**本来就可能很大**的两条不走这里：`status` 用 [`status_capped`]
/// （截断了标 `truncated`），`commit` 用 [`drain_both`]（钩子话多不算失败）。
fn run_raw(cwd: &Path, args: &[&str]) -> R<Vec<u8>> {
    run_raw_capped(cwd, args, MAX_STDOUT_BYTES)
}

/// 上限可注入的版本，**为了能测**。
///
/// 判据同 `fsservice::read_text_detect` 那个可注入上限：真造一个 4MB 输出的
/// 仓库来测这道闸不现实，而不测的话，把这道闸删掉所有测试照样绿。
fn run_raw_capped(cwd: &Path, args: &[&str], cap: usize) -> R<Vec<u8>> {
    let (out, truncated) = run_capped_raw(cwd, args, cap, &[])?;
    if truncated {
        return Err(Error::Git(format!(
            "git {} 的输出超过 {} KB —— 这条命令的输出本该是有界的，多半是仓库处在病态状态",
            args.first().copied().unwrap_or(""),
            cap / 1024
        )));
    }
    Ok(out)
}

/// 跑一条 git 命令，最多**留下** `cap` 字节 stdout，但把剩下的**读完再扔掉**。
///
/// 和 [`run_capped_raw`] 的区别是这里**不 kill 子进程**。给 `commit` 用：
/// 它的退出码必须是可信的 —— 掐掉进程会让「提交成功但钩子话多」和
/// 「提交失败」变得分不出来，而**把一次成功的提交报成失败，比截断一段输出
/// 危险得多**（用户会照着那句话再提交一次）。
///
/// 代价只是把超出的字节读完扔掉：内存仍然是有界的，省不掉的只有 I/O。
/// 跑完把**两份输出**和成败都交出来。
///
/// 只交原始输出、不替调用方判成败，是因为提交的**分类必须两份都看**：
/// git 自己的话在 stdout，钩子的话在 stderr，而两边可以同时有话 ——
/// husky / lint-staged 那类钩子成功时也往 stderr 打一行招呼。只看 stderr 的话，
/// 「暂存区是空的」会被当成「钩子拒绝了」。原来中间还有一层 `run_drained`
/// 替 `commit` 做「stderr 非空就报 stderr」，正是它造成了这个误判，
/// 抽出 `drain_both` 之后那层壳没人用了，已删。
fn drain_both(cwd: &Path, args: &[&str], cap: usize) -> R<(Vec<u8>, bool, String, bool)> {
    use std::io::Read;

    let mut child = git_cmd(cwd, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Error::NoGit)?;

    // 先把 stderr 挂到自己的线程上再读 stdout —— 次序反了就是死锁，见 drain_stderr
    let errs = drain_stderr(
        child.stderr.take().expect("stderr 已 piped"),
        MAX_STDERR_BYTES,
    );

    let mut out = Vec::new();
    // 数总量而不是「out 满没满」：正好读满 cap 而后面再没有了，那不算截断
    let mut total = 0usize;
    {
        let stdout = child.stdout.as_mut().expect("stdout 已 piped");
        let mut buf = [0u8; 16 << 10];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total += n;
                    if out.len() < cap {
                        let room = cap - out.len();
                        out.extend_from_slice(&buf[..n.min(room)]);
                    }
                }
            }
        }
    }
    let truncated = total > cap;
    let err = errs.join().unwrap_or_default();
    let status = child.wait().map_err(Error::NoGit)?;
    Ok((
        out,
        truncated,
        String::from_utf8_lossy(&err).trim().to_string(),
        status.success(),
    ))
}

pub(crate) fn run(cwd: &Path, args: &[&str]) -> R<String> {
    Ok(String::from_utf8_lossy(&run_raw(cwd, args)?).into_owned())
}

/// 一份差异文本，以及它是不是被 [`MAX_DIFF_BYTES`] 截断了。
///
/// `truncated` 必须一路传到界面上。少了这一位，用户看到的是一份**看起来完整**
/// 的差异，而后半截根本没来过 —— 一个会说谎的界面比一个说「我显示不下」的界面糟得多。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub text: String,
    pub truncated: bool,
}

/// 跑一条 git 命令，最多收 `MAX_DIFF_BYTES` 字节 stdout，超了就掐掉子进程。
///
/// `ok_codes` 是除 0 之外还算成功的退出码 —— `diff --no-index` 有差异时返回 1，
/// 那不是失败。
fn run_capped(cwd: &Path, args: &[&str], ok_codes: &[i32]) -> R<Diff> {
    let (mut out, truncated) = run_capped_raw(cwd, args, MAX_DIFF_BYTES, ok_codes)?;
    if truncated {
        // 切回最后一个完整行 —— 切在半行上，前端解析出来的末行是残缺的，
        // 会显示成一条看着像真的、其实少了半截的改动
        if let Some(i) = out.iter().rposition(|&c| c == b'\n') {
            out.truncate(i + 1);
        }
    }
    Ok(Diff {
        text: String::from_utf8_lossy(&out).into_owned(),
        truncated,
    })
}

/// 跑一条 git 命令，最多收 `cap` 字节 stdout，超了就掐掉子进程。
///
/// 返回 `(stdout, 是否被截断)`。**截断后留给调用方的是一份半截的字节流** ——
/// 记录边界（换行、NUL）由调用方自己切齐，这里不猜。
/// 把一条已经建好的 `Command` 的完整 argv 读回来（程序名 + 所有参数）。
///
/// **要的就是「完整」** —— 加固参数（`-c core.fsmonitor=` 那一串）和逐个关掉的
/// filter 驱动都在里面，而 issue #29 的第二条缺口正是「界面上看不到跑的是什么」。
/// 从调用方传进来的 `args` 拼是不行的：那份没有加固参数，
/// 而加固参数恰恰是最可能把一个正常仓库弄坏的东西。
fn argv_of(c: &Command) -> Vec<String> {
    std::iter::once(c.get_program())
        .chain(c.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn run_capped_raw(cwd: &Path, args: &[&str], cap: usize, ok_codes: &[i32]) -> R<(Vec<u8>, bool)> {
    use std::io::Read;

    // Git 控制台（issue #29）。**记在这一处，不在各个调用点** ——
    // 同一条纪律写四遍就是迟早漏一遍，HARDENING 当初就是因为这个才挪到
    // `git_cmd` 上的
    let mut cmd = git_cmd(cwd, args);
    let argv = argv_of(&cmd);
    let t0 = std::time::Instant::now();

    let mut child = match cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            // 起不来也要记 —— 「git 不在」是最该被看见的一种失败，
            // 而它连退出码都没有
            console::record(cwd, &argv, None, t0.elapsed(), e.to_string().as_bytes());
            return Err(Error::NoGit(e));
        }
    };

    /*
     * stderr 先挂到自己的线程上，再读 stdout。
     *
     * 原来是「读完 stdout 再顺序读 stderr」，注释写的是「stderr 也要限量读：
     * 管道写满时 git 会阻塞」—— 限量是对的，**顺序是错的**：子进程写满 stderr
     * 就卡在写上，而我们在等 stdout 的 EOF，那个 EOF 要等它退出才来。
     * 2026-09-07 在 `git commit` 上真的挂住了一次（见 drain_stderr）。
     */
    let errs = drain_stderr(
        child.stderr.take().expect("stderr 已 piped"),
        MAX_STDERR_BYTES,
    );

    let mut out = Vec::new();
    {
        let stdout = child.stdout.as_mut().expect("stdout 已 piped");
        // 多读一个字节：正好读满 cap 和「后面还有」是两回事，
        // 差这一个字节就分不清，会给一份完整的输出误报截断
        stdout
            .take(cap as u64 + 1)
            .read_to_end(&mut out)
            .map_err(Error::NoGit)?;
    }

    let truncated = out.len() > cap;
    if truncated {
        out.truncate(cap);
        // 别让 git 为一份没人要看的输出继续跑完
        let _ = child.kill();
    }

    let err = errs.join().unwrap_or_default();
    let status = child.wait().map_err(Error::NoGit)?;

    /*
     * 记进控制台。**掐掉的那次记 `None` 而不是它的退出码** ——
     * 被 kill 的进程退出码没有意义，照着记会让控制台报一个假的失败
     * （这条和下面那个 `!truncated` 的守卫是同一个判断，只是那边决定
     * 「要不要报错给调用方」，这边决定「照实说什么」）。
     */
    console::record(
        cwd,
        &argv,
        if truncated { None } else { status.code() },
        t0.elapsed(),
        &err,
    );

    // 被我们掐掉的进程，退出码没有意义，不能当成失败
    if !truncated && !status.success() && !ok_codes.contains(&status.code().unwrap_or(-1)) {
        let msg = String::from_utf8_lossy(&err).trim().to_string();
        return Err(Error::Git(if msg.is_empty() {
            format!("git {} 失败", args.first().copied().unwrap_or(""))
        } else {
            msg
        }));
    }

    Ok((out, truncated))
}

/// 字段分隔符：ASCII Unit Separator。
/// 提交标题里出现制表符完全可能，出现 US 几乎不可能。
const US: char = '\x1f';

/// 找到 `path` 所属仓库的根。不是仓库（或没有 git）时返回 `None` —— 
/// 这是正常情况，不是错误：界面据此让整块 Git 功能隐身。
pub fn discover(path: impl AsRef<Path>) -> Option<PathBuf> {
    let p = path.as_ref();
    let dir: &Path = if p.is_dir() { p } else { p.parent()? };
    let out = run(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    let root = out.trim();
    if root.is_empty() {
        return None;
    }
    Some(PathBuf::from(root))
}

/// git 在不在。不在就整块功能隐身。
pub fn available() -> bool {
    Command::new("git")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 一个远程的 URL。用来判协议 —— HTTPS 和 SSH 拿不到凭据时，
/// 该给的提示完全不同（一个是去存钥匙串，一个是 ssh-add）。
///
/// 判协议而不是让前端猜：URL 在 `.git/config` 里，可能是别人克隆时写的。
pub fn remote_url(root: impl AsRef<Path>, remote: &str) -> R<String> {
    Ok(run(root.as_ref(), &["remote", "get-url", remote])?.trim().to_string())
}

#[cfg(test)]
mod tests;
