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
        }
    }
}

impl std::error::Error for Error {}

type R<T> = Result<T, Error>;

/// 给一次失败的 `git commit` 分档。
///
/// **只有两种能可靠认出来**，别的照旧原样透出去 —— 猜错的分类比不分类更害人：
///
/// - `nothing to commit` 是 git 自己说的（在 stdout 里），而且 `LC_ALL=C`
///   保证了它不会被翻译成用户的语言，字符串是稳的。
/// - 钩子拒绝**没有信号**，只能看形状：git 自己一句话都没说，而仓库里确实
///   挂着一个可执行的 `pre-commit`。实测（2026-09-08）钩子拒绝时退出码 1、
///   stdout 一个字都没有、stderr 全是钩子的输出；而 `nothing to commit`
///   反过来 —— 话在 stdout 里，stderr 是空的。
/// 提交路径上**有没有装钩子**。
///
/// # 钩子在哪，由 git 说了算
///
/// 朴素拼 `root/.git/hooks/pre-commit` 有两处会错，都实测过（2026-09-08）：
///
/// - **工作树**：`.git` 是**文件**不是目录，那条路径根本不存在。于是在工作树里
///   这一档永远进不去 —— 而这个应用把工作树当一等功能。
/// - **`core.hooksPath`**：husky 正是靠它把钩子挪到 `.husky/`，
///   盯着 `.git/hooks/` 等于看错了地方。
///
/// `git rev-parse --git-path hooks/<名字>` 两种都认（工作树里返回主仓库的
/// 绝对路径，设了 hooksPath 就返回那个目录）。
///
/// # 为什么查三个
///
/// 一次 `git commit` 会跑 `pre-commit` → `prepare-commit-msg` → `commit-msg`，
/// 任何一个非零都会让提交失败。只查 pre-commit 的话，`commit-msg` 拒了也会
/// 被说成「pre-commit 拒绝」—— 文案也因此不写死是哪一个。
fn commit_hook_installed(root: &Path) -> bool {
    ["pre-commit", "prepare-commit-msg", "commit-msg"]
        .iter()
        .any(|name| {
            let arg = format!("hooks/{name}");
            let out = match git_cmd_hardened(root, &["rev-parse", "--git-path", &arg]).output() {
                Ok(o) if o.status.success() => o,
                _ => return false,
            };
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if raw.is_empty() {
                return false;
            }
            let p = PathBuf::from(&raw);
            // 普通仓库返回的是相对 cwd 的路径，工作树返回绝对路径
            let p = if p.is_absolute() { p } else { root.join(p) };
            std::fs::metadata(&p)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    m.is_file() && m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
        })
}

fn classify_commit_failure(root: &Path, stdout: &str, stderr: &str) -> Error {
    /*
     * **顺序就是判据的强弱，不能换。**
     *
     * ① stdout 里的 `nothing to commit` 是 git 自己说的，最确定。
     *    它必须排在钩子那一档**前面** —— husky / lint-staged 那类钩子
     *    成功时也往 stderr 打一行招呼（`husky > pre-commit`），
     *    只看 stderr 的话，「点了提交但一个文件都没勾」会被报成
     *    「pre-commit 钩子拒绝了这次提交」，而钩子明明是通过的。
     *
     * ② git 自己的报错以 `fatal:` / `error:` 开头。**要看每一行不是只看
     *    第一行**：钩子先打了招呼、git 随后失败时，那句 `error:` 在第二行
     *    （`commit.gpgsign` 失败就是这个形状）。
     *
     * ③ 剩下的才轮到钩子。
     */
    if stdout.contains("nothing to commit") || stdout.contains("no changes added to commit") {
        return Error::NothingStaged {
            raw: format!("{stdout}\n{stderr}").trim().to_string(),
        };
    }
    if stderr
        .lines()
        .any(|l| {
            let t = l.trim_start();
            t.starts_with("fatal:") || t.starts_with("error:")
        })
    {
        return Error::Git(stderr.to_string());
    }
    if commit_hook_installed(root) && !stderr.is_empty() {
        return Error::HookRejected {
            output: stderr.to_string(),
        };
    }
    // 两份都空的时候至少说清是哪条命令失败了
    let msg = if !stderr.is_empty() {
        stderr.to_string()
    } else if !stdout.is_empty() {
        stdout.to_string()
    } else {
        "git commit 失败".to_string()
    };
    Error::Git(msg)
}

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
/// （截断了标 `truncated`），`commit` 用 [`run_drained`]（钩子话多不算失败）。
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
/// [`run_drained`] 和提交的分类都建在它上面。分开是因为
/// **分类必须两份都看**：git 自己的话在 stdout，钩子的话在 stderr，
/// 而两边可以同时有话 —— husky / lint-staged 那类钩子成功时也往 stderr
/// 打一行招呼。只看 stderr 的话，「暂存区是空的」会被当成「钩子拒绝了」。
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

/// 跑一条 git，只要 stdout。失败就地转成 [`Error::Git`]。
fn run_drained(cwd: &Path, args: &[&str], cap: usize) -> R<(Vec<u8>, bool)> {
    let (out, truncated, err, ok) = drain_both(cwd, args, cap)?;
    if !ok {
        // **stderr 空的时候要退回去看 stdout。**
        // git 有一部分话是从 stdout 说的 —— `nothing to commit, working tree
        // clean` 就是。原来这里直接吐「git commit 失败」，把唯一说清原因的
        // 那句丢掉了。
        let msg = if err.is_empty() {
            String::from_utf8_lossy(&out).trim().to_string()
        } else {
            err
        };
        return Err(Error::Git(if msg.is_empty() {
            format!("git {} 失败", args.first().copied().unwrap_or(""))
        } else {
            msg
        }));
    }
    Ok((out, truncated))
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
fn run_capped_raw(cwd: &Path, args: &[&str], cap: usize, ok_codes: &[i32]) -> R<(Vec<u8>, bool)> {
    use std::io::Read;

    let mut child = git_cmd(cwd, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Error::NoGit)?;

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

/// 读取仓库状态。
///
/// 用 `--porcelain=v2 --branch -z`：
/// - v2 把分支名和 ahead/behind 一起带出来，省掉第二次进程启动；
/// - `-z` 用 NUL 分隔记录，路径不做 C 风格转义 —— v1 遇到带空格或中文的
///   路径会加引号并转义，解析端要反过来解一遍，纯属自找麻烦。
pub fn status(root: impl AsRef<Path>) -> R<Status> {
    status_capped(root.as_ref(), MAX_STDOUT_BYTES)
}

/// 上限可注入的版本，**为了能测**（判据同 [`run_raw_capped`]）。
///
/// status **不能像 [`run_raw`] 那样超上限就报错**：改动多是仓库的正常状态，
/// 把整块 Git 功能变成一条报错，比少列几条改动糟得多。所以这条路截断，
/// 而截断了要标 `truncated` —— 界面据此说「还有更多」，
/// 和 `MAX_ENTRIES` 截断走的是同一个出口。
fn status_capped(root: &Path, cap: usize) -> R<Status> {
    let (raw, capped) = run_capped_raw(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
        ],
        cap,
        &[],
    )?;
    // 掐点落在哪儿全看运气，末尾多半是半条路径 —— 丢掉它。
    // 留着就会在改动列表里多出一个看着像真的、其实是半截的文件名
    let raw = if capped { trim_to_last_record(&raw) } else { &raw[..] };
    let mut st = parse_status(raw);
    if capped {
        st.truncated = true;
    }
    expand_untracked_dirs(root, &mut st);
    Ok(st)
}

/// 把折叠出来的未跟踪目录摊成里面的具体文件，追加进 `entries`。
///
/// **为什么不直接用 `--untracked-files=all`**：那样 git 就不再告诉我们哪些
/// 目录是*整个*未跟踪的了。文件树要这个信息给目录本身上色 —— 少了它，
/// 一个全新的目录只剩「里面有东西改了」的冒泡标记，和一个改了一行的
/// 老目录长得一模一样。这里两样都留下：目录名在 `untracked_dirs`，
/// 里面的文件在 `entries`。
///
/// 代价是多起一个子进程，只在真有折叠目录时才起。
fn expand_untracked_dirs(root: &Path, st: &mut Status) {
    if st.untracked_dirs.is_empty() || st.entries.len() >= MAX_ENTRIES {
        return;
    }
    let mut args: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    args.extend(st.untracked_dirs.iter().map(String::as_str));

    // 读不动就算了：目录名已经在 untracked_dirs 里，文件树照样能标色，
    // 只是改动列表少了那几条。为这个把整次 status 判成失败不划算。
    let Ok((raw, capped)) = run_capped_raw(root, &args, MAX_UNTRACKED_BYTES, &[]) else {
        return;
    };
    if capped {
        st.truncated = true;
    }

    for path in split_nul_records(&raw, capped) {
        if st.entries.len() >= MAX_ENTRIES {
            st.truncated = true;
            break;
        }
        st.entries.push(Entry {
            path,
            index: '.',
            work: '?',
            untracked: true,
            conflicted: false,
            orig: None,
        });
    }
    // parse_status 排过一次，但那是在这些文件进来之前
    st.entries.sort_by(|a, b| a.path.cmp(&b.path));
}

/// 砍到最后一条**完整**记录为止（`-z` 的记录以 NUL 结尾）。
///
/// 一个 NUL 都没有 = 连一条完整记录都没读到，那就一条都不能要。
fn trim_to_last_record(raw: &[u8]) -> &[u8] {
    match raw.iter().rposition(|&b| b == 0) {
        Some(i) => &raw[..i + 1],
        None => &[],
    }
}

/// 把 `-z` 的输出切成一条条记录。
///
/// `capped` 为真时**末尾那条要丢掉** —— `run_capped_raw` 是按字节掐的，
/// 掐点落在哪儿全看运气，末尾多半是半条路径。留着它，改动列表里就会多出一个
/// **看着像真的、其实是半截的**文件名（`src/OrderServ`），点开报「文件不存在」。
/// 这和差异截断要切回最后一个完整换行是同一条：宁可少一条，不能多一条假的。
fn split_nul_records(raw: &[u8], capped: bool) -> Vec<String> {
    let head = if capped { trim_to_last_record(raw) } else { raw };
    head
        .split(|&b| b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| String::from_utf8_lossy(r).into_owned())
        .collect()
}

/// v2 + `-z` 的记录解析。
///
/// 格式（`git status` 手册 "Porcelain Format Version 2"）：
/// - `# branch.head <name>` / `# branch.ab +N -M` 等表头
/// - `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`        普通变更
/// - `2 <XY> ... <X><score> <path>` + 独立一条 `<origPath>` 改名/复制
/// - `u <XY> ...  <path>`                                   冲突中
/// - `? <path>` / `! <path>`                                未跟踪 / 已忽略
///
/// 关键陷阱：改名条目在 `-z` 下**占两条记录** —— 新路径一条，源路径一条。
/// 按 NUL 切完后必须让解析器有状态地把下一条吃掉，否则源路径会被
/// 当成一条独立的畸形记录。
fn parse_status(raw: &[u8]) -> Status {
    let mut st = Status::default();
    let mut records = raw.split(|&b| b == 0).filter(|r| !r.is_empty());

    while let Some(rec) = records.next() {
        let line = String::from_utf8_lossy(rec);
        let line = line.as_ref();

        if let Some(rest) = line.strip_prefix("# ") {
            parse_branch_header(rest, &mut st);
            continue;
        }

        if st.entries.len() >= MAX_ENTRIES {
            st.truncated = true;
            // 不 break：还得把剩下的表头读完（表头其实在最前面，
            // 但依赖顺序是脆的，扫完更省心）
            continue;
        }

        let mut chars = line.chars();
        let kind = chars.next().unwrap_or(' ');
        match kind {
            '?' => {
                if let Some(p) = line.get(2..) {
                    // 整个未跟踪的目录被 git 折叠成一条 `dir/`，它走另一个口子 ——
                    // 见 Status::untracked_dirs
                    if p.ends_with('/') {
                        st.untracked_dirs.push(p.to_string());
                        continue;
                    }
                    st.entries.push(Entry {
                        path: p.to_string(),
                        index: '.',
                        work: '?',
                        untracked: true,
                        conflicted: false,
                        orig: None,
                    });
                }
            }
            // 已忽略的不进列表：用户要的是「我改了什么」，不是「git 不管什么」
            '!' => {}
            '1' | '2' | 'u' => {
                // 字段以单空格分隔；路径本身可能含空格，所以按固定字段数切
                let field_count = if kind == 'u' { 10 } else if kind == '1' { 8 } else { 9 };
                let Some((meta, path)) = split_fields(line, field_count) else {
                    continue;
                };
                let xy: Vec<char> = meta.get(1).map(|s| s.chars().collect()).unwrap_or_default();
                let (x, y) = (
                    xy.first().copied().unwrap_or('.'),
                    xy.get(1).copied().unwrap_or('.'),
                );
                // 改名条目的源路径是紧随其后的独立记录，必须在这里吃掉
                let orig = if kind == '2' {
                    records
                        .next()
                        .map(|r| String::from_utf8_lossy(r).into_owned())
                } else {
                    None
                };
                st.entries.push(Entry {
                    path: path.to_string(),
                    index: x,
                    work: y,
                    untracked: false,
                    conflicted: kind == 'u',
                    orig,
                });
            }
            _ => {}
        }
    }

    st.entries.sort_by(|a, b| a.path.cmp(&b.path));
    st
}

/// 从 `line` 里切出前 `n` 个空格分隔字段，剩下的整段当作路径。
///
/// 不能用 `splitn(n+1, ' ')` 一把梭 —— 那样返回的最后一段类型不同、
/// 还得再判长度；这里显式一点更好读，也更好在字段数不足时安全退出。
fn split_fields(line: &str, n: usize) -> Option<(Vec<&str>, &str)> {
    let mut fields = Vec::with_capacity(n);
    let mut rest = line;
    for _ in 0..n {
        let idx = rest.find(' ')?;
        fields.push(&rest[..idx]);
        rest = &rest[idx + 1..];
    }
    if rest.is_empty() {
        return None;
    }
    Some((fields, rest))
}

fn parse_branch_header(rest: &str, st: &mut Status) {
    let mut it = rest.splitn(2, ' ');
    let key = it.next().unwrap_or("");
    let val = it.next().unwrap_or("").trim();
    match key {
        "branch.head" => {
            if val == "(detached)" {
                st.detached = true;
            } else {
                st.branch = val.to_string();
            }
        }
        "branch.oid" => {
            // 一个提交都没有时 git 给的是字面量 "(initial)"
            if val == "(initial)" {
                st.unborn = true;
            } else if st.branch.is_empty() && st.detached {
                st.branch = format!("({})", &val[..val.len().min(7)]);
            }
        }
        "branch.upstream" => st.upstream = val.to_string(),
        "branch.ab" => {
            // 形如 "+3 -1"
            for tok in val.split_whitespace() {
                let (sign, num) = tok.split_at(1);
                let n: u32 = num.parse().unwrap_or(0);
                match sign {
                    "+" => st.ahead = n,
                    "-" => st.behind = n,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// detached HEAD 时 `branch.oid` 可能排在 `branch.head` 前面，
/// 上面的赋值就落空了 —— 补一趟。
fn fill_detached_name(root: &Path, st: &mut Status) {
    if st.detached && st.branch.is_empty() {
        if let Ok(sha) = run(root, &["rev-parse", "--short", "HEAD"]) {
            st.branch = format!("({})", sha.trim());
        }
    }
}

/// 完整状态：解析 + 补齐 detached 名字。命令层用这个。
pub fn status_full(root: impl AsRef<Path>) -> R<Status> {
    let root = root.as_ref();
    let mut st = status(root)?;
    fill_detached_name(root, &mut st);
    if st.branch.is_empty() && st.unborn {
        // 空仓库：HEAD 指向的分支还不存在，但名字是有的
        if let Ok(n) = run(root, &["symbolic-ref", "--short", "HEAD"]) {
            st.branch = n.trim().to_string();
        }
    }
    Ok(st)
}

/// 取一个文件的 unified diff。
///
/// `staged` 为真时比的是「暂存区 ↔ HEAD」，否则是「工作区 ↔ 暂存区」。
/// 未跟踪文件两边都没有记录，走 `--no-index` 跟 /dev/null 比，
/// 效果是整份文件显示成新增 —— 这正是用户想看的。
pub fn diff(root: impl AsRef<Path>, path: &str, staged: bool, untracked: bool) -> R<Diff> {
    let root = root.as_ref();
    // 关掉分页器：pager 会让子进程等一个永远不来的终端
    let common = ["--no-pager", "-c", "core.pager=cat"];

    if untracked {
        if path.ends_with('/') {
            return Ok(Diff::default());
        }
        let full = root.join(path);
        let full = full.to_string_lossy().into_owned();
        let mut args: Vec<&str> = common.to_vec();
        args.extend_from_slice(&["diff", "--no-index", "--no-color"]);
        args.extend_from_slice(DIFF_SAFE);
        args.extend_from_slice(&["--", "/dev/null", &full]);
        // 退出码 0 = 无差异（空文件），1 = 有差异，≥2 才是真出错
        return run_capped(root, &args, &[1]);
    }

    let mut args: Vec<&str> = common.to_vec();
    args.extend_from_slice(&["diff", "--no-color"]);
    args.extend_from_slice(DIFF_SAFE);
    if staged {
        args.push("--cached");
    }
    args.extend_from_slice(&["--", path]);
    run_capped(root, &args, &[])
}

pub fn stage(root: impl AsRef<Path>, paths: &[String]) -> R<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(root.as_ref(), &args).map(|_| ())
}

/// 取消暂存。
///
/// 空仓库（还没有 HEAD）上 `restore --staged` 和 `reset HEAD` 都会失败，
/// 那种情况下正确的命令是 `rm --cached`。判一下 HEAD 在不在，别让第一次
/// 提交前的用户撞一脸 "fatal: could not resolve HEAD"。
pub fn unstage(root: impl AsRef<Path>, paths: &[String]) -> R<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let root = root.as_ref();
    let has_head = run(root, &["rev-parse", "--verify", "--quiet", "HEAD"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let mut args: Vec<&str> = if has_head {
        vec!["restore", "--staged", "--"]
    } else {
        vec!["rm", "--cached", "-r", "-q", "--"]
    };
    args.extend(paths.iter().map(String::as_str));
    run(root, &args).map(|_| ())
}

/// 丢弃工作区改动。**不可撤销**，调用方必须先让用户确认过。
///
/// 未跟踪文件不在 git 的管辖里，`restore` 对它们无效，得直接删。
pub fn discard(root: impl AsRef<Path>, paths: &[String], untracked: &[String]) -> R<()> {
    let root = root.as_ref();
    if !paths.is_empty() {
        let mut args = vec!["restore", "--worktree", "--"];
        args.extend(paths.iter().map(String::as_str));
        run(root, &args)?;
    }
    for p in untracked {
        let full = root.join(p);
        // 只删 root 之下的东西。路径来自 git 自己的输出，理论上安全，
        // 但删除是不可逆操作，多一道校验不亏
        if !full.starts_with(root) {
            continue;
        }
        let _ = if full.is_dir() {
            std::fs::remove_dir_all(&full)
        } else {
            std::fs::remove_file(&full)
        };
    }
    Ok(())
}

/// 提交暂存区。`amend` 为真时改写上一条提交。
pub fn commit(root: impl AsRef<Path>, message: &str, amend: bool) -> R<String> {
    if message.trim().is_empty() {
        return Err(Error::Git("提交信息不能为空".into()));
    }
    let mut args = vec!["commit", "-m", message];
    if amend {
        args.push("--amend");
    }
    // 走 run_drained 而不是 run：pre-commit 钩子想打印多少打印多少，
    // 而**掐掉子进程会让退出码失去意义** —— 那时「提交成功但钩子话多」和
    // 「提交失败」就分不出来了，而把一次成功的提交报成失败，
    // 会让用户照着那句话再提交一次。
    // 走 `drain_both` 而不是 `run_drained`：分档要**两份输出都看**，
    // 只看 stderr 会把「暂存区是空的」误报成「钩子拒绝了」
    let (out, truncated, err, ok) = drain_both(root.as_ref(), &args, MAX_STDOUT_BYTES)?;
    if !ok {
        let so = String::from_utf8_lossy(&out);
        return Err(classify_commit_failure(root.as_ref(), so.trim(), &err));
    }
    let mut s = String::from_utf8_lossy(&out).into_owned();
    if truncated {
        // 前端只取第一行显示，但这句得留在里头 —— 万一以后有人整段展示
        s.push_str("\n（钩子输出太长，已截断）");
    }
    Ok(s)
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



// ─────────────────── 历史 · 分支 · 工作树 ───────────────────

/// 提交历史里的一条。比 [`Commit`] 多带画图和跳转需要的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub sha: String,
    pub short: String,
    pub author: String,
    pub email: String,
    /// 相对时间（"3 hours ago"），git 自己算
    pub when: String,
    /// 绝对日期 YYYY-MM-DD
    pub date: String,
    pub subject: String,
    /// 父提交的完整 sha。合并提交有两个及以上 —— 画泳道图全靠它
    pub parents: Vec<String>,
    /// 指向这条提交的引用名（分支、标签、HEAD）
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// 短名，如 `main` 或 `origin/main`
    pub name: String,
    pub sha: String,
    /// 上游分支，没有就是空
    pub upstream: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub when: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    pub sha: String,
    /// 检出的分支短名；游离头指针时为空
    pub branch: String,
    pub detached: bool,
    /// 裸仓库
    pub bare: bool,
    /// 被锁定（`git worktree lock`），不能直接删
    pub locked: bool,
    /// 就是当前打开的这个
    pub current: bool,
}

/// 字段分隔符：ASCII Unit Separator。
/// 提交标题里出现制表符完全可能，出现 US 几乎不可能。
const US: char = '\x1f';

/// 提交历史。
///
/// `all` 为真时把所有分支都算进来（IDEA 的「全部分支」），否则只看当前 HEAD 这条线。
/// `path` 非空时只看某个文件的历史。
pub fn log_entries(
    root: impl AsRef<Path>,
    limit: usize,
    all: bool,
    path: &str,
) -> R<Vec<LogEntry>> {
    let n = format!("-{limit}");
    // %H sha · %h 短 sha · %an 作者 · %ae 邮箱 · %ar 相对时间 · %ad 日期
    // %s 标题 · %P 父提交们 · %D 引用名
    let fmt = format!(
        "--format=%H{US}%h{US}%an{US}%ae{US}%ar{US}%ad{US}%s{US}%P{US}%D"
    );
    // --topo-order 不是可选项，是泳道图的前提：
    // 默认的提交时间序里，父提交完全可能排在子提交前面（两条提交时间戳相同时
    // 就会这样，合并操作尤其常见）。一旦父先于子出现，泳道算法「认领正在等我的
    // 那条泳道」的前提就不成立，主线会莫名其妙断掉、跳到别的泳道去。
    // gitk 和 IDEA 用拓扑序也是为这个。
    let mut args = vec![
        "--no-pager",
        "log",
        &n,
        &fmt,
        "--date=short",
        "--topo-order",
    ];
    if all {
        args.push("--all");
    }
    if !path.is_empty() {
        args.push("--");
        args.push(path);
    }
    let out = match run(root.as_ref(), &args) {
        Ok(s) => s,
        // 空仓库没有历史，这不是错误
        Err(Error::Git(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(out.lines().filter(|l| !l.is_empty()).filter_map(parse_log_line).collect())
}

fn parse_log_line(l: &str) -> Option<LogEntry> {
    let f: Vec<&str> = l.split(US).collect();
    if f.len() < 9 {
        return None;
    }
    Some(LogEntry {
        sha: f[0].to_string(),
        short: f[1].to_string(),
        author: f[2].to_string(),
        email: f[3].to_string(),
        when: f[4].to_string(),
        date: f[5].to_string(),
        subject: f[6].to_string(),
        parents: f[7].split_whitespace().map(str::to_string).collect(),
        // %D 形如 "HEAD -> main, origin/main, tag: v1.0"
        refs: f[8]
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_start_matches("HEAD -> ").to_string())
            .collect(),
    })
}

/// 一次提交里动了哪些文件。`--name-status` 给出状态字母 + 路径。
///
/// 合并提交默认什么都不输出（git 认为差异有歧义），加 `-m --first-parent`
/// 让它按「相对第一个父提交」算 —— 这也是人看合并提交时想看的东西。
pub fn commit_files(root: impl AsRef<Path>, sha: &str) -> R<Vec<Entry>> {
    let raw = run_raw(
        root.as_ref(),
        &[
            "--no-pager",
            "show",
            "--name-status",
            "--format=",
            "-m",
            "--first-parent",
            "-z",
            sha,
        ],
    )?;
    let mut out = Vec::new();
    // -z 下记录是 `<状态>\0<路径>\0`，改名则是 `<状态>\0<旧>\0<新>\0`
    let mut it = raw.split(|&b| b == 0).filter(|r| !r.is_empty());
    while let Some(st) = it.next() {
        let st = String::from_utf8_lossy(st);
        let code = st.chars().next().unwrap_or('M');
        let Some(p1) = it.next() else { break };
        let p1 = String::from_utf8_lossy(p1).into_owned();
        let (path, orig) = if code == 'R' || code == 'C' {
            match it.next() {
                Some(p2) => (String::from_utf8_lossy(p2).into_owned(), Some(p1)),
                None => (p1, None),
            }
        } else {
            (p1, None)
        };
        out.push(Entry {
            path,
            index: code,
            work: '.',
            untracked: false,
            conflicted: false,
            orig,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 某次提交里某个文件的差异。`path` 为空则给整次提交的差异。
pub fn commit_diff(root: impl AsRef<Path>, sha: &str, path: &str) -> R<Diff> {
    let spec = format!("{sha}^!");
    let mut args = vec!["--no-pager", "-c", "core.pager=cat", "diff", "--no-color"];
    args.extend_from_slice(DIFF_SAFE);
    args.push(&spec);
    if !path.is_empty() {
        args.push("--");
        args.push(path);
    }
    // 首次提交没有父，`sha^!` 会失败 —— 退回与空树比
    match run_capped(root.as_ref(), &args, &[]) {
        Ok(s) => Ok(s),
        Err(Error::Git(_)) => {
            let mut a2 = vec!["--no-pager", "show", "--no-color", "--format=", "--root"];
            a2.extend_from_slice(DIFF_SAFE);
            a2.push(sha);
            if !path.is_empty() {
                a2.push("--");
                a2.push(path);
            }
            run_capped(root.as_ref(), &a2, &[])
        }
        Err(e) => Err(e),
    }
}

/// 分支列表（本地 + 远程），一次 `for-each-ref` 搞定。
///
/// 不用 `git branch`：它的输出是给人看的，前缀空格、`*` 标记、颜色都要再剥一层。
/// `for-each-ref` 的 `--format` 是给机器看的，要什么给什么。
pub fn branches(root: impl AsRef<Path>) -> R<Vec<Branch>> {
    let fmt = format!(
        "--format=%(refname:short){US}%(objectname:short){US}%(upstream:short){US}%(HEAD){US}%(committerdate:relative){US}%(contents:subject){US}%(refname)"
    );
    let out = run(
        root.as_ref(),
        &["for-each-ref", &fmt, "refs/heads", "refs/remotes"],
    )?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split(US).collect();
            if f.len() < 7 {
                return None;
            }
            /*
             * `refs/remotes/origin/HEAD` 是个符号引用（指向远程的默认分支），
             * 不是能检出的东西，列出来只会碍事。
             *
             * **必须按全名判断**：git 缩写远程 HEAD 时会把 `/HEAD` 一起吃掉，
             * `refs/remotes/origin/HEAD` 的 `%(refname:short)` 是 **`origin`**，
             * 不是 `origin/HEAD`。所以按短名过滤永远匹配不上，
             * 界面上就会多出一条叫「origin」的假分支，点了必然报错。
             */
            if f[6].ends_with("/HEAD") {
                return None;
            }
            Some(Branch {
                name: f[0].to_string(),
                sha: f[1].to_string(),
                upstream: f[2].to_string(),
                is_head: f[3] == "*",
                is_remote: f[6].starts_with("refs/remotes/"),
                when: f[4].to_string(),
                subject: f[5].to_string(),
            })
        })
        .collect())
}

/// 切分支。
///
/// - `create` 为真：新建并切过去（`switch -c`）
/// - 名字是**远程分支全名**（如 `origin/foo`）时走 `--track`，
///   建一个跟踪它的同名本地分支
///
/// 关于远程分支有个坑：`git switch origin/foo` 会直接失败 ——
/// `fatal: a branch is expected, got remote branch 'origin/foo'`。
/// git 的 DWIM（自动建跟踪分支）只对**短名**生效：本地没有 `foo` 而
/// `origin/foo` 存在时，`git switch foo` 才会自动建。传全名反而不行。
/// 界面上列出来的是全名（要区分 origin/foo 和 upstream/foo），
/// 所以这里得把这层翻译做掉。
///
/// 工作区脏的时候 git 会自己拒绝。**「本地改动会被覆盖」这一类单独分出来**
/// （[`Error::LocalChanges`]，带上挡路的文件名），前端才给得出「去提交 /
/// 丢弃这些改动」两个按钮；其余的错误照旧原样上抛，git 的措辞比我们能写的准。
pub fn switch_branch(root: impl AsRef<Path>, name: &str, create: bool) -> R<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Git("分支名不能为空".into()));
    }
    let root = root.as_ref();
    /// 所有出口都要过这一遍分类 —— 这个函数有五个 `run(...)` 的返回点，
    /// 少包一个的表现就是「大部分时候给按钮，某个分支上突然给英文报错」。
    fn classify(r: R<String>) -> R<String> {
        match r {
            Err(Error::Git(msg)) => match parse_local_changes(&msg) {
                Some(files) => Err(Error::LocalChanges { files, raw: msg }),
                None => Err(Error::Git(msg)),
            },
            other => other,
        }
    }
    if create {
        return classify(run(root, &["switch", "-c", name]));
    }

    let exists = |r: &str| {
        run(root, &["rev-parse", "--verify", "--quiet", r])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    };

    // 本地就有同名分支：直接切，最常见的情形
    if exists(&format!("refs/heads/{name}")) {
        return classify(run(root, &["switch", name]));
    }
    // 是个远程分支：建跟踪分支切过去
    if exists(&format!("refs/remotes/{name}")) {
        let short = name.split_once('/').map(|(_, b)| b).unwrap_or(name);
        // 本地已经有同名短分支了（跟踪的可能是别的远程），就切到那个，
        // 别再建一个重名的
        if exists(&format!("refs/heads/{short}")) {
            return classify(run(root, &["switch", short]));
        }
        return classify(run(root, &["switch", "--track", name]));
    }
    // 既不是本地也不是远程：交给 git 自己判断（可能是 tag 或 sha）
    classify(run(root, &["switch", name]))
}

/// 从 git 的 stderr 里认出「本地改动挡着切分支」，并把挡路的文件名切出来。
///
/// git 的原话长这样（`LC_ALL=C` 保证是英文，见 `git_cmd`）：
///
/// ```text
/// error: Your local changes to the following files would be overwritten by checkout:
///         src/a.rs
///         src/b.rs
/// Please commit your changes or stash them before you switch branches.
/// Aborting
/// ```
///
/// 未跟踪文件挡路时是另一句（"The following untracked working tree files
/// would be overwritten by checkout"），**格式一样**，所以按「would be
/// overwritten by」这一句认，两种都收。
///
/// 认不出来就返回 `None` —— 那时照旧把原话上抛，它的措辞比我们能写的准。
fn parse_local_changes(stderr: &str) -> Option<Vec<String>> {
    let mut lines = stderr.lines();
    lines.find(|l| l.contains("would be overwritten by"))?;
    // 文件名是缩进的，一行一个；碰到第一行不缩进的（"Please commit …"）就到头了
    let files: Vec<String> = lines
        .take_while(|l| l.starts_with('\t') || l.starts_with("  "))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    // 认出了句子却一个文件都没切出来，说明格式和预期不一样 —— 那就别装懂
    if files.is_empty() {
        return None;
    }
    Some(files)
}

/// 工作树列表。`--porcelain` 的记录以空行分隔，每行是 `键 值`。
pub fn worktrees(root: impl AsRef<Path>) -> R<Vec<Worktree>> {
    let root = root.as_ref();
    let out = run(root, &["worktree", "list", "--porcelain"])?;
    let here = root.to_string_lossy().into_owned();
    let mut list = Vec::new();
    let mut cur: Option<Worktree> = None;

    let flush = |cur: &mut Option<Worktree>, list: &mut Vec<Worktree>| {
        if let Some(w) = cur.take() {
            list.push(w);
        }
    };

    for line in out.lines() {
        if line.is_empty() {
            flush(&mut cur, &mut list);
            continue;
        }
        let (key, val) = match line.split_once(' ') {
            Some((k, v)) => (k, v),
            None => (line, ""),
        };
        match key {
            "worktree" => {
                flush(&mut cur, &mut list);
                cur = Some(Worktree {
                    current: val == here,
                    path: val.to_string(),
                    sha: String::new(),
                    branch: String::new(),
                    detached: false,
                    bare: false,
                    locked: false,
                });
            }
            "HEAD" => {
                if let Some(w) = cur.as_mut() {
                    w.sha = val.chars().take(7).collect();
                }
            }
            "branch" => {
                if let Some(w) = cur.as_mut() {
                    w.branch = val.trim_start_matches("refs/heads/").to_string();
                }
            }
            "detached" => {
                if let Some(w) = cur.as_mut() {
                    w.detached = true;
                }
            }
            "bare" => {
                if let Some(w) = cur.as_mut() {
                    w.bare = true;
                }
            }
            "locked" => {
                if let Some(w) = cur.as_mut() {
                    w.locked = true;
                }
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut list);
    Ok(list)
}

/// 新建工作树。
///
/// `branch` 已存在就检出它，不存在就顺带新建（`-b`）—— 这个判断放在这里而不是
/// 前端：它是「git 里分支存不存在」的业务判断，而且省掉一次 IPC 往返。
/// 用户在意的只是「我要一个跑着这个分支的目录」，不该关心加不加 `-b`。
///
/// 返回新工作树的绝对路径，调用方可以直接把它当项目根打开。
pub fn worktree_add(root: impl AsRef<Path>, path: &str, branch: &str) -> R<String> {
    if path.trim().is_empty() {
        return Err(Error::Git("工作树路径不能为空".into()));
    }
    let root_p = root.as_ref();
    let refspec = format!("refs/heads/{branch}");
    let exists = !branch.is_empty()
        && run(root_p, &["rev-parse", "--verify", "--quiet", &refspec])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    let mut args: Vec<&str> = vec!["worktree", "add"];
    if branch.is_empty() {
        args.push(path);
    } else if exists {
        args.push(path);
        args.push(branch);
    } else {
        args.push("-b");
        args.push(branch);
        args.push(path);
    }
    run(root.as_ref(), &args)?;
    // git 接受相对路径，但前端要的是能直接打开的绝对路径
    let p = Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.as_ref().join(p)
    };
    Ok(std::fs::canonicalize(&abs)
        .unwrap_or(abs)
        .to_string_lossy()
        .into_owned())
}

/// 移除工作树。**会删掉那个目录**，调用方必须先让用户确认。
///
/// `force` 对应 `--force`：里面有未提交改动时 git 会拒绝，除非强制。
pub fn worktree_remove(root: impl AsRef<Path>, path: &str, force: bool) -> R<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path);
    run(root.as_ref(), &args).map(|_| ())
}

/// 一个远程的 URL。用来判协议 —— HTTPS 和 SSH 拿不到凭据时，
/// 该给的提示完全不同（一个是去存钥匙串，一个是 ssh-add）。
///
/// 判协议而不是让前端猜：URL 在 `.git/config` 里，可能是别人克隆时写的。
pub fn remote_url(root: impl AsRef<Path>, remote: &str) -> R<String> {
    Ok(run(root.as_ref(), &["remote", "get-url", remote])?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析器的输入是字节，用 `\0` 拼真实格式，不走 git
    fn rec(parts: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for p in parts {
            v.extend_from_slice(p.as_bytes());
            v.push(0);
        }
        v
    }

    #[test]
    fn 表头带出分支与领先落后() {
        let raw = rec(&[
            "# branch.oid abc123",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +3 -1",
        ]);
        let st = parse_status(&raw);
        assert_eq!(st.branch, "main");
        assert_eq!(st.upstream, "origin/main");
        assert_eq!(st.ahead, 3);
        assert_eq!(st.behind, 1);
        assert!(!st.detached);
    }

    #[test]
    fn 普通变更条目的xy与路径() {
        let raw = rec(&[
            "# branch.head main",
            "1 M. N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 .M N... 100644 100644 100644 ccc ddd README.md",
        ]);
        let st = parse_status(&raw);
        assert_eq!(st.entries.len(), 2);
        // 排序后 README 在前
        assert_eq!(st.entries[0].path, "README.md");
        assert_eq!(st.entries[0].index, '.');
        assert_eq!(st.entries[0].work, 'M');
        assert!(st.entries[0].unstaged() && !st.entries[0].staged());
        assert_eq!(st.entries[1].path, "src/main.rs");
        assert!(st.entries[1].staged() && !st.entries[1].unstaged());
    }

    /// 这是 -z 格式最容易写错的地方：改名占两条记录
    #[test]
    fn 改名条目要吃掉紧随其后的源路径记录() {
        let raw = rec(&[
            "# branch.head main",
            "2 R. N... 100644 100644 100644 aaa bbb R100 新名字.rs",
            "旧名字.rs",
            "1 .M N... 100644 100644 100644 ccc ddd z.txt",
        ]);
        let st = parse_status(&raw);
        // 源路径不能变成第三条畸形条目
        assert_eq!(st.entries.len(), 2, "源路径被误当成独立条目了");
        let renamed = st.entries.iter().find(|e| e.path == "新名字.rs").unwrap();
        assert_eq!(renamed.orig.as_deref(), Some("旧名字.rs"));
        assert_eq!(renamed.index, 'R');
        assert!(st.entries.iter().any(|e| e.path == "z.txt"));
    }

    #[test]
    fn 带空格的路径不能被切断() {
        let raw = rec(&[
            "# branch.head main",
            "1 .M N... 100644 100644 100644 aaa bbb docs/my notes/a b.md",
            "? 未跟踪 的文件.txt",
        ]);
        let st = parse_status(&raw);
        assert!(st.entries.iter().any(|e| e.path == "docs/my notes/a b.md"));
        let u = st.entries.iter().find(|e| e.untracked).unwrap();
        assert_eq!(u.path, "未跟踪 的文件.txt");
    }

    #[test]
    fn 未跟踪与冲突与忽略() {
        let raw = rec(&[
            "# branch.head main",
            "? new.txt",
            "! ignored.log",
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc conflict.rs",
        ]);
        let st = parse_status(&raw);
        // 已忽略的不进列表
        assert!(!st.entries.iter().any(|e| e.path == "ignored.log"));
        assert!(st.entries.iter().any(|e| e.untracked && e.path == "new.txt"));
        let c = st.entries.iter().find(|e| e.conflicted).unwrap();
        assert_eq!(c.path, "conflict.rs");
    }

    /// 折叠的未跟踪目录不进 `entries`，只留下目录名给文件树
    #[test]
    fn 折叠的未跟踪目录走另一个口子() {
        let raw = rec(&["# branch.head main", "? scratch/", "? notes.txt"]);
        let st = parse_status(&raw);
        assert_eq!(st.untracked_dirs, vec!["scratch/".to_string()]);
        assert_eq!(
            st.entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["notes.txt"],
            "目录不该出现在改动列表里：{:?}",
            st.entries
        );
    }

    /// 被字节上限掐掉时，末尾那条半截路径必须丢掉。
    ///
    /// 留着它，改动列表里会多出一个**看着像真的、其实是半截的**文件名 ——
    /// 而一个说谎的界面比一句「显示不下」危险得多（和差异截断切回换行同一条）。
    #[test]
    fn 截断的路径列表要丢掉末尾那条半截的() {
        let raw = b"a/one.txt\0b/two.txt\0c/thre".to_vec();

        // 没截断：末尾那条是完整的，三条都要
        assert_eq!(
            split_nul_records(&raw, false),
            vec!["a/one.txt", "b/two.txt", "c/thre"],
        );
        // 截断了：`c/thre` 是半截的
        assert_eq!(split_nul_records(&raw, true), vec!["a/one.txt", "b/two.txt"]);
        // 连一个 NUL 都没有 —— 一条完整记录都没读到，一条都不能要
        assert!(split_nul_records(b"c/thre", true).is_empty());
    }

    /// 「本地改动挡着切分支」要能认出来并切出文件名。
    ///
    /// 这一条卡的是**界面上有没有按钮**：认出来才给「去提交 / 丢弃这些改动」，
    /// 认不出来就退回原样上抛一段英文。
    #[test]
    fn 认得出本地改动挡着切分支() {
        let tracked = "error: Your local changes to the following files would be overwritten by checkout:\n\tsrc/a.rs\n\tsrc/b 有空格.rs\nPlease commit your changes or stash them before you switch branches.\nAborting\n";
        assert_eq!(
            parse_local_changes(tracked),
            Some(vec!["src/a.rs".to_string(), "src/b 有空格.rs".to_string()]),
        );

        // 未跟踪文件挡路是另一句，格式一样，也要收
        let untracked = "error: The following untracked working tree files would be overwritten by checkout:\n\tnew.txt\nPlease move or remove them before you switch branches.\n";
        assert_eq!(parse_local_changes(untracked), Some(vec!["new.txt".to_string()]));

        // 不是这一类的错误一律不认 —— 装懂比不懂糟
        assert_eq!(parse_local_changes("fatal: invalid reference: nope\n"), None);
        // 认出了句子却一个文件都没切出来，说明格式变了，也不装懂
        assert_eq!(
            parse_local_changes("error: files would be overwritten by checkout:\nAborting\n"),
            None,
        );
    }

    /// 冲突条目不能既算「已暂存」又算「改动」—— 那会让它在界面上出现三次
    #[test]
    fn 冲突条目既不算暂存也不算未暂存() {
        let raw = rec(&[
            "# branch.head main",
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc both.rs",
            "1 M. N... 100644 100644 100644 aaa bbb staged.rs",
            "1 .M N... 100644 100644 100644 ccc ddd dirty.rs",
        ]);
        let st = parse_status(&raw);
        let c = st.entries.iter().find(|e| e.conflicted).unwrap();
        assert!(!c.staged(), "冲突条目不该算已暂存");
        assert!(!c.unstaged(), "冲突条目不该算未暂存");
        // 其余两条不受影响
        assert!(st.entries.iter().find(|e| e.path == "staged.rs").unwrap().staged());
        assert!(st.entries.iter().find(|e| e.path == "dirty.rs").unwrap().unstaged());
    }

    #[test]
    fn detached与空仓库的表头() {
        let d = parse_status(&rec(&["# branch.oid a1b2c3d4e5", "# branch.head (detached)"]));
        assert!(d.detached);
        let u = parse_status(&rec(&["# branch.oid (initial)", "# branch.head main"]));
        assert!(u.unborn);
        assert_eq!(u.branch, "main");
    }

    #[test]
    fn 超过上限要截断而不是撑爆前端() {
        let mut parts: Vec<String> = vec!["# branch.head main".into()];
        for i in 0..(MAX_ENTRIES + 10) {
            parts.push(format!("? f{i}.txt"));
        }
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        let st = parse_status(&rec(&refs));
        assert_eq!(st.entries.len(), MAX_ENTRIES);
        assert!(st.truncated);
    }

    /// 端到端：真起 git 建个临时仓库跑一遍。
    /// 没装 git 的机器上直接跳过，不让 CI 假红。
    #[test]
    fn 真仓库上的状态与暂存往返() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = std::env::temp_dir().join(format!("gitsvc-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();

        // 空仓库：discover 能找到根，status 报 unborn
        assert!(discover(&dir).is_some());
        let st = status_full(&dir).unwrap();
        assert!(st.unborn, "刚 init 的仓库应该是 unborn");

        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        let st = status_full(&dir).unwrap();
        assert!(st.entries.iter().any(|e| e.path == "a.txt" && e.untracked));

        // 空仓库上取消暂存必须走 rm --cached，不能崩
        stage(&dir, &["a.txt".into()]).unwrap();
        assert!(status_full(&dir).unwrap().entries[0].staged());
        unstage(&dir, &["a.txt".into()]).unwrap();
        assert!(status_full(&dir).unwrap().entries[0].untracked, "取消暂存后应变回未跟踪");

        stage(&dir, &["a.txt".into()]).unwrap();
        commit(&dir, "首次提交", false).unwrap();
        let st = status_full(&dir).unwrap();
        assert!(st.entries.is_empty(), "提交后工作区应该是干净的");
        assert!(!st.unborn);
        assert_eq!(st.branch, "main");

        // 改一行，diff 里应该同时有加和减
        std::fs::write(dir.join("a.txt"), "world\n").unwrap();
        let d = diff(&dir, "a.txt", false, false).unwrap().text;
        assert!(d.contains("-hello") && d.contains("+world"), "diff 不对：{d}");

        // 丢弃改动
        discard(&dir, &["a.txt".into()], &[]).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "hello\n");

        let l = log_entries(&dir, 10, false, "").unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].subject, "首次提交");
        assert!(l[0].parents.is_empty(), "首次提交没有父");

        // 带空格和中文的路径要能完整往返。整个目录都是未跟踪时 git 折叠成
        // 一条 "有 空格/"，我们把它摊开：目录名进 untracked_dirs，
        // 里面的文件进 entries。
        std::fs::create_dir_all(dir.join("有 空格")).unwrap();
        std::fs::write(dir.join("有 空格/中 文.md"), "x\n").unwrap();
        let st = status_full(&dir).unwrap();
        assert_eq!(st.untracked_dirs, vec!["有 空格/".to_string()]);
        let f = st
            .entries
            .iter()
            .find(|e| e.path.starts_with("有 空格"))
            .unwrap_or_else(|| panic!("带空格的中文路径没解析对：{:?}", st.entries));
        assert_eq!(f.path, "有 空格/中 文.md", "目录不该出现在改动列表里");
        assert!(f.untracked);

        // 目录里的单个文件被跟踪之后，git 自己就报完整路径，不再折叠
        stage(&dir, &["有 空格/中 文.md".into()]).unwrap();
        let st = status_full(&dir).unwrap();
        assert!(st.untracked_dirs.is_empty());
        let f = st.entries.iter().find(|e| e.path.contains("中 文")).unwrap();
        assert_eq!(f.path, "有 空格/中 文.md");
        commit(&dir, "加个带空格的中文路径", false).unwrap();

        // 改名要能带出源路径
        std::fs::rename(dir.join("a.txt"), dir.join("b.txt")).unwrap();
        stage(&dir, &["a.txt".into(), "b.txt".into()]).unwrap();
        let st = status_full(&dir).unwrap();
        let r = st.entries.iter().find(|e| e.path == "b.txt").unwrap();
        assert_eq!(r.orig.as_deref(), Some("a.txt"), "改名源路径丢了：{r:?}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 泳道图的前提：log 必须是拓扑序 —— 任何一条提交的父，都要排在它**后面**。
    ///
    /// 这条测试是冲着 `--topo-order` 去的。默认的提交时间序在「父子提交时间戳
    /// 相同」时会把父排到子前面，图就画歪了。造仓库时刻意把所有提交压在同一个
    /// 时间戳上，正是为了让默认序必然出错、而拓扑序必然正确。
    #[test]
    fn 提交历史必须是拓扑序() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = std::env::temp_dir().join(format!("gitsvc-topo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 所有提交同一个时间戳：这样提交时间序完全无法区分先后
        let stamp = "2026-01-01T00:00:00+00:00";
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .env("GIT_AUTHOR_DATE", stamp)
                .env("GIT_COMMITTER_DATE", stamp)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t.t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t.t")
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .output()
                .unwrap()
        };

        git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a"), "1").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "base"]);

        // 分出一条支线，各提交一次，再合并回来
        git(&["switch", "-q", "-c", "side"]);
        std::fs::write(dir.join("b"), "1").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "side-1"]);

        git(&["switch", "-q", "main"]);
        std::fs::write(dir.join("c"), "1").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "main-1"]);

        git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);

        let es = log_entries(&dir, 100, true, "").unwrap();
        assert!(es.len() >= 4, "应该有至少 4 条提交，实得 {}", es.len());

        // 核心断言：每条提交的父，位置都必须比它自己靠后
        let pos: std::collections::HashMap<&str, usize> = es
            .iter()
            .enumerate()
            .map(|(i, e)| (e.sha.as_str(), i))
            .collect();
        for (i, e) in es.iter().enumerate() {
            for p in &e.parents {
                if let Some(&j) = pos.get(p.as_str()) {
                    assert!(
                        j > i,
                        "拓扑序被破坏：{} 的父 {} 排在了它前面（{i} vs {j}）\n完整顺序：{:?}",
                        e.subject,
                        &p[..7],
                        es.iter().map(|x| &x.subject).collect::<Vec<_>>()
                    );
                }
            }
        }

        // 顺带确认合并提交确实带出了两个父，泳道图才有岔路可画
        let merge = es.iter().find(|e| e.subject == "merge side").unwrap();
        assert_eq!(merge.parents.len(), 2, "合并提交该有两个父");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 未跟踪文件必须整份显示成新增 —— 这是 VS Code / IDEA 的一致行为，
    /// 也是唯一有意义的显示：它没有「旧版本」可比，左栏本来就该是空的。
    #[test]
    fn 未跟踪文件的差异是整份新增() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = std::env::temp_dir().join(format!("gitsvc-untracked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/tasks")).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir.join("docs/tasks/old.md"), "old\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        run(&dir, &["commit", "-qm", "base"]).unwrap();

        // 目录本身已被跟踪，所以新文件会以完整路径出现，不会被折叠成 "docs/tasks/"
        let rel = "docs/tasks/2026-08-27-new.md";
        std::fs::write(dir.join(rel), "# 标题\n\n第一行\n第二行\n").unwrap();

        let st = status_full(&dir).unwrap();
        let e = st.entries.iter().find(|e| e.path == rel).unwrap();
        assert!(e.untracked, "应该是一条未跟踪的文件条目：{e:?}");

        let d = diff(&dir, rel, false, true).unwrap();
        assert!(!d.truncated, "这么小的文件不该触发截断");
        let d = d.text;
        assert!(!d.trim().is_empty(), "未跟踪文件的差异不能是空的");
        assert!(d.contains("new file mode"), "应标成新增文件：{d}");
        assert!(
            d.contains("+# 标题") && d.contains("+第一行") && d.contains("+第二行"),
            "整份内容都该是新增行：{d}"
        );
        assert!(
            !d.lines().any(|l| l.starts_with('-') && !l.starts_with("---")),
            "新增文件不该有删除行：{d}"
        );

        // 空的未跟踪文件：git 退出码 0、没有输出。这是合法情形，不能报错
        let empty_rel = "docs/tasks/empty.md";
        std::fs::write(dir.join(empty_rel), "").unwrap();
        assert!(
            diff(&dir, empty_rel, false, true).is_ok(),
            "空的未跟踪文件不该报错"
        );

        // 被折叠的未跟踪目录：目录名只出现在 untracked_dirs 里，
        // 改动列表拿到的是里面的文件。真给 diff 传一个目录路径也不能炸 ——
        // 没有单文件差异可言，返回空串而不是报错。
        std::fs::create_dir_all(dir.join("brand-new/sub")).unwrap();
        std::fs::write(dir.join("brand-new/a.txt"), "x\n").unwrap();
        std::fs::write(dir.join("brand-new/sub/b.txt"), "y\n").unwrap();
        let st = status_full(&dir).unwrap();
        assert_eq!(st.untracked_dirs, vec!["brand-new/".to_string()]);
        let inside: Vec<&str> = st
            .entries
            .iter()
            .filter(|e| e.path.starts_with("brand-new/"))
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(
            inside,
            vec!["brand-new/a.txt", "brand-new/sub/b.txt"],
            "折叠的目录要摊成里面的每个文件：{:?}",
            st.entries
        );
        assert_eq!(diff(&dir, "brand-new/", false, true).unwrap(), Diff::default());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 真仓库上跑一次「被本地改动挡住的切分支」。
    ///
    /// 上面那条纯函数测试卡的是**解析**，这条卡的是**接线** ——
    /// `switch_branch` 有五个 `run(...)` 的返回点，少包一个 classify 的表现是
    /// 「大部分时候给按钮，某个分支上突然给英文报错」，而纯函数测试看不见这个。
    #[test]
    fn 切分支被本地改动挡住时要分类而不是原样上抛() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "gitsvc-blocked-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();

        std::fs::write(dir.join("a.txt"), "main 这边的内容\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "base", false).unwrap();

        // other 分支上把同一个文件改掉并提交
        switch_branch(&dir, "other", true).unwrap();
        std::fs::write(dir.join("a.txt"), "other 这边的内容\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "other 改了 a.txt", false).unwrap();
        switch_branch(&dir, "main", false).unwrap();

        // 回到 main，在工作区里改同一个文件但不提交 —— 这时切 other 必被拒
        std::fs::write(dir.join("a.txt"), "没提交的改动\n").unwrap();
        match switch_branch(&dir, "other", false) {
            Err(Error::LocalChanges { files, raw }) => {
                assert_eq!(files, vec!["a.txt".to_string()], "挡路的文件没切对");
                assert!(!raw.is_empty(), "原话要留着，界面上「看 git 的原话」要用");
            }
            other => panic!("应该分类成 LocalChanges，实际是：{other:?}"),
        }

        // 改动提交掉之后同一次切换要成功 —— 证明上面那条不是「永远失败」
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "提交掉挡路的改动", false).unwrap();
        // 现在两边都改过同一个文件，切过去会是快进不了的普通切换（git 允许）
        switch_branch(&dir, "other", false).expect("提交之后应该切得过去");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 大文件的差异必须被掐在上限内，而且要如实说自己被截断了。
    ///
    /// 这条挡的是一个实测出来的内存问题：一个 30MB 的新增文件，`git diff`
    /// 原样吐 30MB，过一趟 JSON IPC 再在前端解析成行对象，堆占用涨到 126MB ——
    /// 而界面最多只渲染 3000 行。
    #[test]
    fn 大差异要截断且如实上报() {
        let dir = std::env::temp_dir().join(format!("gitsvc-bigdiff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();

        // 造一份稳超 1MB 的未跟踪文件
        let mut body = String::new();
        while body.len() < MAX_DIFF_BYTES * 3 {
            body.push_str("这一行有点长，重复很多遍就能把差异撑过上限 0123456789\n");
        }
        std::fs::write(dir.join("big.txt"), &body).unwrap();

        let d = diff(&dir, "big.txt", false, true).unwrap();
        assert!(d.truncated, "超过上限的差异必须标成截断");
        assert!(
            d.text.len() <= MAX_DIFF_BYTES,
            "截断后不该还超上限：{} > {MAX_DIFF_BYTES}",
            d.text.len()
        );
        // 切在半行上，前端会把残行当成一条真改动显示出来
        assert!(d.text.ends_with('\n'), "必须切在完整行的边界上");
        assert!(d.text.contains("new file mode"), "开头那段该原样保留");

        // 小文件走同一条路径，不能被误报成截断
        std::fs::write(dir.join("small.txt"), "一行\n").unwrap();
        let s = diff(&dir, "small.txt", false, true).unwrap();
        assert!(!s.truncated, "小文件不该报截断");
        assert!(s.text.contains("+一行"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 远程分支要能检出。
    ///
    /// `git switch origin/foo` 会直接失败，必须翻译成 `--track origin/foo`。
    /// 这条造一个真的「远程」（用本地目录当 remote），走完整流程。
    #[test]
    fn 检出远程分支要建跟踪分支() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let base = std::env::temp_dir().join(format!("gitsvc-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let origin = base.join("origin");
        let clone = base.join("clone");
        std::fs::create_dir_all(&origin).unwrap();

        let cfg = |d: &Path| {
            run(d, &["config", "user.email", "t@t.t"]).unwrap();
            run(d, &["config", "user.name", "t"]).unwrap();
        };
        run(&origin, &["init", "-q", "-b", "main"]).unwrap();
        cfg(&origin);
        std::fs::write(origin.join("a.txt"), "1").unwrap();
        run(&origin, &["add", "-A"]).unwrap();
        run(&origin, &["commit", "-qm", "base"]).unwrap();
        // 在 origin 上再造一条分支
        run(&origin, &["switch", "-q", "-c", "feature/x"]).unwrap();
        std::fs::write(origin.join("b.txt"), "2").unwrap();
        run(&origin, &["add", "-A"]).unwrap();
        run(&origin, &["commit", "-qm", "feature"]).unwrap();
        run(&origin, &["switch", "-q", "main"]).unwrap();

        run(
            &base,
            &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()],
        )
        .unwrap();
        cfg(&clone);

        // 克隆之后本地只有 main，feature/x 只存在于 origin/ 下
        let bs = branches(&clone).unwrap();
        assert!(
            bs.iter().any(|b| b.name == "origin/feature/x" && b.is_remote),
            "没列出远程分支：{:?}",
            bs.iter().map(|b| &b.name).collect::<Vec<_>>()
        );
        // refs/remotes/origin/HEAD 的短名就是 "origin"，它不是分支，不能出现在列表里
        assert!(
            !bs.iter().any(|b| b.name == "origin"),
            "远程 HEAD 混进分支列表了：{:?}",
            bs.iter().map(|b| &b.name).collect::<Vec<_>>()
        );
        assert!(
            !bs.iter().any(|b| b.name == "feature/x" && !b.is_remote),
            "本地不该已经有 feature/x"
        );

        // 关键：传全名也必须能切过去
        switch_branch(&clone, "origin/feature/x", false)
            .unwrap_or_else(|e| panic!("检出远程分支失败：{e}"));
        let st = status_full(&clone).unwrap();
        assert_eq!(st.branch, "feature/x", "应该切到了本地跟踪分支");
        assert_eq!(st.upstream, "origin/feature/x", "上游没设对");
        assert!(clone.join("b.txt").exists(), "工作区内容没跟着切过来");

        // 再切回去，然后用全名切第二次 —— 这次本地已有同名分支，不该重复新建
        switch_branch(&clone, "main", false).unwrap();
        switch_branch(&clone, "origin/feature/x", false).unwrap();
        assert_eq!(status_full(&clone).unwrap().branch, "feature/x");

        std::fs::remove_dir_all(&base).unwrap();
    }

    /// 不是仓库的目录必须安静地返回 None，不能报错
    #[test]
    fn 非仓库目录返回none() {
        let dir = std::env::temp_dir().join(format!("gitsvc-norepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 临时目录本身可能落在某个仓库里（少见但可能），只在确实不在仓库时断言
        if discover(std::env::temp_dir()).is_none() {
            assert!(discover(&dir).is_none());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 一个干净的临时目录。名字带 pid —— 失败时不清理，
    /// 而残留目录会让下一次 `git init` 撞上（remote 那边踩过这个坑）
    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gitsvc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个「配置里挂着一条可执行脚本」的仓库。
    ///
    /// 返回 `(仓库路径, 痕迹文件)` —— 脚本一旦被 git 执行，痕迹文件就会出现。
    /// 痕迹刻意放在**仓库外面**：放里面的话它自己会变成一个未跟踪文件，
    /// 后面几步的差异就跟着变了。
    fn trapped_repo(name: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("gitsvc-trap-{name}-{}", std::process::id()));
        let marker = std::env::temp_dir().join(format!("gitsvc-trap-{name}-{}.痕迹", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&marker);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();

        let hook = dir.join("hook.sh");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, marker)
    }

    /// **打开一个别人给的仓库，不许因此执行仓库自带的脚本。**
    ///
    /// `core.fsmonitor` 是这一族里最凶的一条：它由 `git status` 触发，
    /// 而 lite-ide 在项目根一变就自动跑 status（App.svelte 里那条 effect），
    /// 用户一次都不用点；会话恢复还让它每次启动都再跑一遍。
    ///
    /// 这条测试是照着真的能打中的形状写的 —— 先在裸 git 上验证过它确实
    /// 会被执行，再加的 `-c core.fsmonitor=`。
    #[test]
    fn 仓库自带的_fsmonitor_不许被执行() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let (dir, marker) = trapped_repo("fsmonitor");
        let hook = dir.join("hook.sh");
        run(&dir, &["config", "core.fsmonitor", hook.to_str().unwrap()]).unwrap();
        std::fs::write(dir.join("a.txt"), "x\n").unwrap();

        let _ = status(&dir);

        assert!(
            !marker.exists(),
            "git status 执行了仓库 .git/config 里挂的脚本 —— 打开一个别人的目录就等于让他在这台机器上跑代码"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&marker).ok();
    }

    /// issue #17 的第二个口子：`remote.<名字>.url = ext::<命令>` ——
    /// fetch / push 时 git 会把那条命令当传输层**跑起来**。
    ///
    /// **测试里必须自己先把协议放开**（`protocol.ext.allow = always`）。
    /// git 2.50 默认就拒绝 ext:，不放开的话这条断言永远绿 —— 它验的会是
    /// git 的默认值，而不是我们的加固。而放开这件事**恶意仓库自己就能做**，
    /// 因为 `protocol.ext.allow` 可以写在仓库的 `.git/config` 里：
    /// 实测不带加固时，一条 `git fetch` 就执行了仓库指定的脚本。
    #[test]
    fn 仓库自带的_ext_传输不许被执行() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let (dir, marker) = trapped_repo("ext");
        let hook = dir.join("hook.sh");
        // 仓库自己放开这个协议 —— 这一句正是加固要压过去的东西
        run(&dir, &["config", "protocol.ext.allow", "always"]).unwrap();
        run(&dir, &[
            "config",
            "remote.evil.url",
            &format!("ext::{}", hook.display()),
        ])
        .unwrap();

        // 走真实的拉取路径（`remote.rs` 也经过 `git_cmd`），不是通用的 run
        let cancel: crate::remote::Cancel = Default::default();
        let _ = crate::remote::fetch(&dir, "evil", &cancel, &mut |_| {});

        assert!(
            !marker.exists(),
            "fetch 执行了仓库 .git/config 里挂的脚本 —— 点一下「拉取」就等于让仓库的作者在这台机器上跑代码"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&marker).ok();
    }

    /// issue #15 缺口 2：提交失败要说人话。
    ///
    /// 两档各验一次。**第三档（认不出来的）故意原样透出去** ——
    /// 猜错的分类比不分类更害人。
    #[test]
    fn 提交失败要分档说人话() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = tmpdir("commit-classify");
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();

        // ① 暂存区空的
        match commit(&dir, "空提交", false) {
            Err(Error::NothingStaged { .. }) => {}
            other => panic!("该分成 NothingStaged，实际是：{other:?}"),
        }
        let msg = format!("{}", commit(&dir, "空提交", false).unwrap_err());
        assert!(msg.contains("暂存区是空的"), "说的还是 git 的原话：{msg}");
        assert!(msg.contains("全部暂存"), "没给出下一步该点哪儿：{msg}");

        // ② pre-commit 钩子拒绝
        std::fs::write(dir.join("a.txt"), "x\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        let hook = dir.join(".git/hooks/pre-commit");
        // 20 行噪声 + 最后一句真话 —— 验「只留最后几行」确实把真话留下了
        let mut sh = String::from("#!/bin/sh\n");
        for i in 1..=20 {
            sh.push_str(&format!("echo '通过检查 {i}' >&2\n"));
        }
        sh.push_str("echo 'lint: a.txt 第 3 行有问题' >&2\nexit 1\n");
        std::fs::write(&hook, sh).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let e = commit(&dir, "会被拒", false).unwrap_err();
        match &e {
            Error::HookRejected { .. } => {}
            other => panic!("该分成 HookRejected，实际是：{other:?}"),
        }
        let msg = format!("{e}");
        assert!(msg.contains("提交被钩子拒绝"), "没说清是谁拒的：{msg}");
        assert!(msg.contains("改动都还在"), "没告诉人代码还在：{msg}");
        assert!(
            msg.contains("lint: a.txt 第 3 行有问题"),
            "最后那句真话被截没了：{msg}"
        );
        assert!(msg.contains("前面还有"), "截断了却没说截了多少：{msg}");
        assert!(!msg.contains("通过检查 1\n"), "前面的噪声没被截掉：{msg}");
        // 完整的那份不能丢
        assert!(e.raw().contains("通过检查 1"), "raw() 里也没有完整输出");

        // ③ **暂存区空 + 会说话的钩子** —— grok review 逮到的那条。
        // husky / lint-staged 成功时也往 stderr 打招呼，只看 stderr 就会
        // 把「一个文件都没勾」报成「钩子拒绝了」，而钩子明明通过了
        let dir3 = tmpdir("commit-classify-3");
        run(&dir3, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir3, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir3, &["config", "user.name", "t"]).unwrap();
        let hook3 = dir3.join(".git/hooks/pre-commit");
        std::fs::write(&hook3, "#!/bin/sh\necho 'husky > pre-commit' >&2\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook3, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        match commit(&dir3, "没勾文件", false) {
            Err(Error::NothingStaged { .. }) => {}
            other => panic!("会说话的钩子把「暂存区是空的」盖住了：{other:?}"),
        }

        // ④ **钩子先打招呼，git 随后失败** —— 那句 error: 在第二行，
        // 只看第一行会漏，于是又赖到钩子头上
        std::fs::write(dir3.join("b.txt"), "y\n").unwrap();
        run(&dir3, &["add", "-A"]).unwrap();
        run(&dir3, &["config", "commit.gpgsign", "true"]).unwrap();
        run(&dir3, &["config", "gpg.program", "/nonexistent-gpg"]).unwrap();
        match commit(&dir3, "签不了名", false) {
            Err(Error::Git(_)) => {}
            other => panic!("git 自己的 error: 在第二行时被赖到钩子头上：{other:?}"),
        }
        std::fs::remove_dir_all(&dir3).ok();

        // ⑥ **core.hooksPath（husky 的做法）** —— 真钩子在 .husky/，
        // 盯着 .git/hooks/ 就等于看错了地方，这一档会漏
        let dir6 = tmpdir("commit-classify-6");
        run(&dir6, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir6, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir6, &["config", "user.name", "t"]).unwrap();
        std::fs::create_dir_all(dir6.join(".husky")).unwrap();
        run(&dir6, &["config", "core.hooksPath", ".husky"]).unwrap();
        let h6 = dir6.join(".husky/pre-commit");
        std::fs::write(&h6, "#!/bin/sh\necho 'lint 没过' >&2\nexit 1\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&h6, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(dir6.join("c.txt"), "z\n").unwrap();
        run(&dir6, &["add", "-A"]).unwrap();
        match commit(&dir6, "会被 husky 拒", false) {
            Err(Error::HookRejected { output }) => {
                assert!(output.contains("lint 没过"), "钩子的话没带出来：{output}")
            }
            other => panic!("core.hooksPath 下的钩子没认出来：{other:?}"),
        }
        std::fs::remove_dir_all(&dir6).ok();

        // ⑦ **工作树** —— .git 是文件不是目录，朴素拼 .git/hooks/ 那条路径
        // 根本不存在，于是工作树里这一档永远进不去
        let dir7 = tmpdir("commit-classify-7");
        run(&dir7, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir7, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir7, &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir7.join("a.txt"), "x\n").unwrap();
        run(&dir7, &["add", "-A"]).unwrap();
        commit(&dir7, "首次提交", false).unwrap();
        let h7 = dir7.join(".git/hooks/pre-commit");
        std::fs::write(&h7, "#!/bin/sh\necho '工作树里也该认出来' >&2\nexit 1\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&h7, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let wt = dir7.join("../wt-classify-7");
        run(&dir7, &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wtb"]).unwrap();
        assert!(wt.join(".git").is_file(), "工作树的 .git 该是文件");
        std::fs::write(wt.join("b.txt"), "y\n").unwrap();
        run(&wt, &["add", "-A"]).unwrap();
        match commit(&wt, "工作树里提交", false) {
            Err(Error::HookRejected { output }) => {
                assert!(output.contains("工作树里也该认出来"), "钩子的话没带出来：{output}")
            }
            other => panic!("工作树里的钩子没认出来：{other:?}"),
        }
        std::fs::remove_dir_all(&wt).ok();
        std::fs::remove_dir_all(&dir7).ok();

        // ⑤ **钩子在，但失败原因跟钩子无关** —— 不许赖到钩子头上。
        // `--amend` 在还没有提交的仓库上报 `fatal: You have nothing to amend.`，
        // 形状（stdout 空、stderr 有话）和钩子拒绝一模一样
        let dir2 = tmpdir("commit-classify-2");
        run(&dir2, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir2, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir2, &["config", "user.name", "t"]).unwrap();
        let hook2 = dir2.join(".git/hooks/pre-commit");
        std::fs::write(&hook2, "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook2, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        match commit(&dir2, "改上一条", true) {
            Err(Error::Git(m)) => assert!(m.contains("nothing to amend"), "原话没留住：{m}"),
            other => panic!("git 自己的 fatal 被赖到钩子头上了：{other:?}"),
        }
        std::fs::remove_dir_all(&dir2).ok();

        std::fs::remove_dir_all(&dir).ok();
    }

    /// issue #17 的第一个口子：`filter.<名字>.smudge` —— **检出时**跑。
    ///
    /// 这一条不能像别的加固那样一句 `-c` 压过去：驱动名是任意的，
    /// 点不着的键关不掉。所以 `git_cmd` 会先查一次仓库**自己带的**驱动名
    /// （`--local`，用户全局那份里的 git-lfs 不碰），再逐个关。
    #[test]
    fn 仓库自带的_filter_不许被执行() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let (dir, marker) = trapped_repo("filter");
        let hook = dir.join("hook.sh");

        std::fs::write(dir.join("a.txt"), "内容\n").unwrap();
        // 一句 .gitattributes 就把驱动挂到这个文件上
        std::fs::write(dir.join(".gitattributes"), "a.txt filter=随便什么名字\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "首次提交", false).unwrap();

        // **配置要在建仓库之后写** —— 这正好也验到了缓存的 key 带 mtime：
        // 只按路径缓存的话，这次写入之后那份空名单还会被用上，测试就假绿了
        run(&dir, &[
            "config",
            "filter.随便什么名字.smudge",
            hook.to_str().unwrap(),
        ])
        .unwrap();

        // 触发检出：把文件删掉再让 git 写回来
        std::fs::remove_file(dir.join("a.txt")).unwrap();
        let _ = run(&dir, &["checkout", "--", "a.txt"]);

        assert!(
            !marker.exists(),
            "检出执行了仓库 .git/config 里挂的 smudge 脚本 —— 切一下分支就等于让仓库的作者在这台机器上跑代码"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&marker).ok();
    }

    /// 同一族的另一半：看差异不许执行仓库自带的脚本。
    ///
    /// 四条产生差异的路要一起验 —— `--no-ext-diff` 当初**只写在其中两条上**，
    /// 而 `--no-textconv` 一条都没有。挨个点一遍才发现漏了哪几个。
    #[test]
    fn 仓库自带的_diff_驱动不许被执行() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let (dir, marker) = trapped_repo("diff");
        let hook = dir.join("hook.sh");
        let hook_s = hook.to_str().unwrap().to_string();
        run(&dir, &["config", "diff.external", &hook_s]).unwrap();
        run(&dir, &["config", "diff.ev.textconv", &hook_s]).unwrap();
        // 一句 .gitattributes 就够把 textconv 挂上去。
        // **逐个文件写，不写 `*`** —— 写 `*` 时实测第一步打不中（git 对通配
        // 属性的处理和显式路径不一样），于是那一步会变成一条永远绿的断言
        std::fs::write(dir.join(".gitattributes"), "a.txt diff=ev\nb.txt diff=ev\n").unwrap();

        std::fs::write(dir.join("a.txt"), "第一版\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "首次提交", false).unwrap();
        let first = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
        std::fs::write(dir.join("a.txt"), "第二版\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "第二次提交", false).unwrap();
        let second = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();

        // ① 工作区里已跟踪文件的差异
        std::fs::write(dir.join("a.txt"), "改了一行\n").unwrap();
        let _ = diff(&dir, "a.txt", false, false);
        assert!(!marker.exists(), "已跟踪文件的差异执行了仓库自带的脚本（textconv 那条路）");

        // ② 未跟踪文件走的是 diff --no-index，另一条路
        std::fs::write(dir.join("b.txt"), "全新的\n").unwrap();
        let _ = diff(&dir, "b.txt", false, true);
        assert!(!marker.exists(), "未跟踪文件的 --no-index 差异执行了仓库自带的脚本");

        // ③ 历史提交的差异（有父提交，走主路径）
        let _ = commit_diff(&dir, &second, "a.txt");
        assert!(!marker.exists(), "历史提交的差异执行了仓库自带的脚本");

        // ④ 首次提交没有父，`sha^!` 会失败而回退到 git show —— 那条当初完全没设防
        let _ = commit_diff(&dir, &first, "a.txt");
        assert!(!marker.exists(), "首次提交的 show 回退执行了仓库自带的脚本");

        // ⑤ **忘了 DIFF_SAFE 的新入口必须当场炸，不能悄悄拿到一份空差异。**
        //
        // `-c diff.external=` 让 git 去执行一条空命令，于是这种调用直接失败。
        // 这是有意的 fail-closed，理由见 HARDENING 的注释。
        // 2026-09-07 拿真仓库验收时正是从这条错误消息上发现这个行为的。
        let e = run_raw(&dir, &["diff", "--", "a.txt"]).expect_err("忘了 DIFF_SAFE 就该失败");
        assert!(
            format!("{e}").contains("cannot run"),
            "失败的理由要能看出是外部 diff 驱动，实得：{e}"
        );
        assert!(!marker.exists(), "忘了 DIFF_SAFE 竟然把仓库自带的脚本跑了");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&marker).ok();
    }

    /// **输出本该有界的命令，超上限要报错，不能闷头收下。**
    ///
    /// `run_raw` 原来用 `.output()` —— 把整份 stdout 全缓冲进内存，没有任何上限。
    /// 这是 AGENTS.md 那条「跑子进程读它 stdout，先问一句有没有上限」
    /// 第三次漏在同一个形状上。
    ///
    /// 上限可注入正是为了这条测试：真造一个 4MB 输出的仓库不现实，
    /// 而不测的话，把这道闸删掉所有测试照样绿。
    #[test]
    fn 输出超过上限的命令要报错而不是给一份半截的() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = tmpdir("cap-run");
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir.join("a.txt"), "x\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "首次提交", false).unwrap();

        // ① 接线：走真正的 `run_raw`（用真正的 MAX_STDOUT_BYTES）。
        //
        // 这一段是有代价的 —— 要真造一份 5MB 的输出。**但省不掉**：
        // 只测可注入上限的那个版本，把 `run_raw` 改回 `.output()` 之后
        // 测试照样绿（试过），那就成了一条测不到接线的断言。
        let big = "这一行是用来把输出撑到 4MB 以上的噪声\n".repeat(100_000);
        assert!(big.len() > MAX_STDOUT_BYTES, "造出来的得比上限大");
        std::fs::write(dir.join("big.txt"), &big).unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "一个大文件", false).unwrap();

        let e = run_raw(&dir, &["show", "HEAD:big.txt"]).expect_err("超上限必须报错");
        let msg = format!("{e}");
        assert!(
            msg.contains("输出超过"),
            "报错要说清是被上限拦下的，实得：{msg}"
        );

        // ② 上限本身：可注入的版本，两边界各验一次
        let args = ["for-each-ref", "--format=%(refname)"];
        assert!(
            run_raw_capped(&dir, &args, 16 << 10).is_ok(),
            "正常大小不该被拦"
        );
        assert!(
            run_raw_capped(&dir, &args, 4).is_err(),
            "4 字节的上限必须拦下分支列表"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// **status 的输出被掐断时：标 `truncated`，而且不许留下半截路径。**
    ///
    /// status 不能像别的命令那样超上限就报错 —— 改动多是仓库的正常状态，
    /// 把整块 Git 功能变成一条报错，比少列几条改动糟得多。所以它截断，
    /// 而截断必须说出来，且末尾那条半截记录必须丢掉：
    /// 留着就是改动列表里一个**看着像真的、其实点不开**的文件名。
    #[test]
    fn status_被掐断时要标出来且不留半截路径() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = tmpdir("cap-status");
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();
        // 名字取长一点，好让「掐在半条路径上」真的发生
        let names: Vec<String> = (0..40)
            .map(|i| format!("一个名字相当长的未跟踪文件-{i:02}.txt"))
            .collect();
        for n in &names {
            std::fs::write(dir.join(n), "x\n").unwrap();
        }

        let full = status_capped(&dir, MAX_STDOUT_BYTES).unwrap();
        assert!(!full.truncated, "这点输出不该被截断");
        assert_eq!(full.entries.len(), 40);

        let cut = status_capped(&dir, 512).unwrap();
        assert!(cut.truncated, "掐断了必须标 truncated");
        assert!(cut.entries.len() < 40, "掐断了条目就该变少");
        for e in &cut.entries {
            assert!(
                names.contains(&e.path),
                "留下了一条半截路径：{:?} —— 它在改动列表里点不开",
                e.path
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// **钩子话多，不能把提交挂住。**
    ///
    /// 这条是并发排空 stderr（`drain_stderr`）存在的全部理由，而且是**真的挂过**：
    /// 第一版把 `run_drained` 写成「先把 stdout 读完，再顺序读 stderr」，
    /// 跑这条测试时 `git commit` 和测试进程互相等着，最后是手动 kill 掉的。
    ///
    /// 根因是一个反直觉的事实：**git 2.50 把 pre-commit 钩子的 stdout 转到了
    /// stderr** —— 实测一个 200 行的钩子，git 的 stdout 只有 89 字节，
    /// stderr 有 2892 字节。于是钩子一话多就写满 stderr 那几十 KB 缓冲，
    /// 卡在写上；而我们在等 stdout 的 EOF，那个 EOF 要等它退出才来。
    ///
    /// 界面上的表现是「点了提交，然后什么都不再发生」。
    ///
    /// 卡 20 秒：修好之后实测不到 1 秒，挂住的那一版会走满 20 秒。
    #[test]
    fn 钩子话多不能把提交挂住() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir("noisy-hook");
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();

        // 5000 行约 150KB，稳稳超过管道那几十 KB 的缓冲
        let hook = dir.join(".git/hooks/pre-commit");
        std::fs::write(
            &hook,
            "#!/bin/sh\nfor i in $(seq 1 5000); do echo \"eslint: 一切正常，这行纯属噪声 $i\"; done\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

        std::fs::write(dir.join("a.txt"), "x\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let d = dir.clone();
        std::thread::spawn(move || {
            let _ = tx.send(commit(&d, "钩子话很多", false).is_ok());
        });
        match rx.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(ok) => assert!(ok, "钩子退出码是 0，提交不该失败"),
            Err(_) => panic!("提交挂住了 —— stderr 没有被并发排空（见 drain_stderr）"),
        }
        assert!(
            status_full(&dir).unwrap().entries.is_empty(),
            "提交应该真的发生了，工作区该是干净的"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `run_drained` 的另一半：**stdout 超上限时留下的字节要被管住，但不算失败。**
    ///
    /// 和 `run_raw` 那条相反 —— 那边超上限是报错（输出本该有界），
    /// 这边是截断（钩子话多是正常的，报错等于把成功的提交说成失败）。
    #[test]
    fn run_drained_超上限只截断不报错() {
        if !available() {
            eprintln!("跳过：机器上没有 git");
            return;
        }
        let dir = tmpdir("drain-cap");
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        run(&dir, &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir.join("a.txt"), "x\n").unwrap();
        run(&dir, &["add", "-A"]).unwrap();
        commit(&dir, "首次提交", false).unwrap();

        let args = ["for-each-ref", "--format=%(refname)"];
        let (out, truncated) = run_drained(&dir, &args, 4).expect("截断不是失败");
        assert!(truncated, "输出比 4 字节长，应该报截断");
        assert!(out.len() <= 4, "留下的字节要被上限管住，实得 {}", out.len());

        let (out, truncated) = run_drained(&dir, &args, 16 << 10).unwrap();
        assert!(!truncated, "正常大小不该报截断");
        assert!(String::from_utf8_lossy(&out).contains("refs/heads/main"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
