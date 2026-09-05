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
    /// 这条其实是**整个未跟踪的目录**（路径以 `/` 结尾）。
    ///
    /// `--untracked-files=normal` 会把一个完全未跟踪的目录折叠成一条记录，
    /// 而不是列出里面每个文件 —— 这是对的：新建一个目录不该在 Git 面板里
    /// 炸出几百行。但文件树要靠这个标记做**前缀匹配**，才能把目录里的文件
    /// 也标成未跟踪。别让前端去猜末尾的斜杠。
    pub is_dir: bool,
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
    pub entries: Vec<Entry>,
    /// 条目被 MAX_ENTRIES 截断了
    pub truncated: bool,
}

#[derive(Debug)]
pub enum Error {
    /// 机器上没有 git，或者起不来
    NoGit(io::Error),
    /// git 跑了但报错，带上 stderr —— 直接给用户看，比我们转译得准
    Git(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoGit(e) => write!(f, "找不到 git 命令：{e}"),
            Error::Git(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

type R<T> = Result<T, Error>;

/// 跑一条 git 命令，返回 stdout 的原始字节。
///
/// stdout 保持 `Vec<u8>` 不转 String：路径在 git 眼里是字节串，
/// macOS 上确实可能有非 UTF-8 的文件名，提前 `from_utf8` 会在这类仓库上直接崩。
/// 建一条环境干净的 git 命令。所有对外的调用都必须经过这里 ——
/// 少一条 `env_remove` 或少一个 `GIT_TERMINAL_PROMPT=0`，
/// 表现就是「某个仓库上偶发地查到别处去」或者「后台调用挂着等密码」。
pub(crate) fn git_cmd(cwd: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
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

fn run_raw(cwd: &Path, args: &[&str]) -> R<Vec<u8>> {
    let out = git_cmd(cwd, args).output().map_err(Error::NoGit)?;

    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::Git(if msg.is_empty() {
            format!("git {} 失败", args.first().copied().unwrap_or(""))
        } else {
            msg
        }));
    }
    Ok(out.stdout)
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
    use std::io::Read;

    let mut child = git_cmd(cwd, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Error::NoGit)?;

    let mut out = Vec::new();
    {
        let stdout = child.stdout.as_mut().expect("stdout 已 piped");
        // 多读一个字节：正好读满 cap 和「后面还有」是两回事，
        // 差这一个字节就分不清，会给一份完整的差异误报截断
        stdout
            .take(MAX_DIFF_BYTES as u64 + 1)
            .read_to_end(&mut out)
            .map_err(Error::NoGit)?;
    }

    let truncated = out.len() > MAX_DIFF_BYTES;
    if truncated {
        out.truncate(MAX_DIFF_BYTES);
        // 切回最后一个完整行 —— 切在半行上，前端解析出来的末行是残缺的，
        // 会显示成一条看着像真的、其实少了半截的改动
        if let Some(i) = out.iter().rposition(|&c| c == b'\n') {
            out.truncate(i + 1);
        }
        // 别让 git 为一份没人要看的差异继续跑完
        let _ = child.kill();
    }

    // stderr 也要限量读：管道写满时 git 会阻塞，而我们已经不读 stdout 了
    let mut err = Vec::new();
    if let Some(stderr) = child.stderr.as_mut() {
        let _ = stderr.take(8 << 10).read_to_end(&mut err);
    }
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

    Ok(Diff {
        text: String::from_utf8_lossy(&out).into_owned(),
        truncated,
    })
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
    let root = root.as_ref();
    let raw = run_raw(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
        ],
    )?;
    Ok(parse_status(&raw))
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
                    st.entries.push(Entry {
                        path: p.to_string(),
                        index: '.',
                        work: '?',
                        untracked: true,
                        is_dir: p.ends_with('/'),
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
                    is_dir: false,
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
    // 统一关掉外部 diff 驱动和分页器：pager 会让子进程等一个永远不来的终端
    let common = ["--no-pager", "-c", "core.pager=cat"];

    if untracked {
        if path.ends_with('/') {
            return Ok(Diff::default());
        }
        let full = root.join(path);
        let full = full.to_string_lossy().into_owned();
        let mut args: Vec<&str> = common.to_vec();
        args.extend_from_slice(&["diff", "--no-index", "--no-color", "--", "/dev/null", &full]);
        // 退出码 0 = 无差异（空文件），1 = 有差异，≥2 才是真出错
        return run_capped(root, &args, &[1]);
    }

    let mut args: Vec<&str> = common.to_vec();
    args.extend_from_slice(&["diff", "--no-color", "--no-ext-diff"]);
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
    run(root.as_ref(), &args)
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
            is_dir: false,
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
    let mut args = vec![
        "--no-pager",
        "-c",
        "core.pager=cat",
        "diff",
        "--no-color",
        "--no-ext-diff",
        &spec,
    ];
    if !path.is_empty() {
        args.push("--");
        args.push(path);
    }
    // 首次提交没有父，`sha^!` 会失败 —— 退回与空树比
    match run_capped(root.as_ref(), &args, &[]) {
        Ok(s) => Ok(s),
        Err(Error::Git(_)) => {
            let mut a2 = vec![
                "--no-pager",
                "show",
                "--no-color",
                "--format=",
                "--root",
                sha,
            ];
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
/// 工作区脏的时候 git 会自己拒绝并说清楚原因，我们把 stderr 原样上抛 ——
/// 它的措辞比我们能写的更准。
pub fn switch_branch(root: impl AsRef<Path>, name: &str, create: bool) -> R<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Git("分支名不能为空".into()));
    }
    let root = root.as_ref();
    if create {
        return run(root, &["switch", "-c", name]);
    }

    let exists = |r: &str| {
        run(root, &["rev-parse", "--verify", "--quiet", r])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    };

    // 本地就有同名分支：直接切，最常见的情形
    if exists(&format!("refs/heads/{name}")) {
        return run(root, &["switch", name]);
    }
    // 是个远程分支：建跟踪分支切过去
    if exists(&format!("refs/remotes/{name}")) {
        let short = name.split_once('/').map(|(_, b)| b).unwrap_or(name);
        // 本地已经有同名短分支了（跟踪的可能是别的远程），就切到那个，
        // 别再建一个重名的
        if exists(&format!("refs/heads/{short}")) {
            return run(root, &["switch", short]);
        }
        return run(root, &["switch", "--track", name]);
    }
    // 既不是本地也不是远程：交给 git 自己判断（可能是 tag 或 sha）
    run(root, &["switch", name])
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

        // 带空格和中文的路径要能完整往返。
        // 注意：整个目录都是未跟踪时，git 折叠成一条 "有 空格/" —— 这是它的
        // 默认行为，也是我们要的，所以断言的是折叠后的形态。
        std::fs::create_dir_all(dir.join("有 空格")).unwrap();
        std::fs::write(dir.join("有 空格/中 文.md"), "x\n").unwrap();
        let st = status_full(&dir).unwrap();
        let d = st
            .entries
            .iter()
            .find(|e| e.path.starts_with("有 空格"))
            .unwrap_or_else(|| panic!("带空格的中文路径没解析对：{:?}", st.entries));
        assert_eq!(d.path, "有 空格/");
        assert!(d.is_dir && d.untracked);

        // 目录里的单个文件被跟踪之后，路径就是完整的（不再折叠）
        stage(&dir, &["有 空格/中 文.md".into()]).unwrap();
        let st = status_full(&dir).unwrap();
        let f = st.entries.iter().find(|e| e.path.contains("中 文")).unwrap();
        assert_eq!(f.path, "有 空格/中 文.md");
        assert!(!f.is_dir);
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
        assert!(e.untracked && !e.is_dir, "应该是一条未跟踪的文件条目：{e:?}");

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

        // 被折叠的未跟踪目录：没有单文件差异可言，返回空串而不是报错
        std::fs::create_dir_all(dir.join("brand-new")).unwrap();
        std::fs::write(dir.join("brand-new/a.txt"), "x\n").unwrap();
        let st = status_full(&dir).unwrap();
        let d2 = st.entries.iter().find(|e| e.is_dir).unwrap();
        assert_eq!(d2.path, "brand-new/");
        assert_eq!(diff(&dir, &d2.path, false, true).unwrap(), Diff::default());

        std::fs::remove_dir_all(&dir).unwrap();
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
}
