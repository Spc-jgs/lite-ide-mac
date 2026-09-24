//! 提交与 stash，以及提交失败时分档（钩子拒了 / 暂存区是空的）。

use super::*;

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

/// 一条 stash。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{N}` 里的 N
    pub index: u32,
    /// git 给的那句：`WIP on main: a1b2c3d 上一条提交的标题`
    pub message: String,
}

/// stash 列表（issue #33 ⑪）。空仓库、没有 stash 都是空表，不是错误。
pub fn stash_list(root: impl AsRef<Path>) -> R<Vec<Stash>> {
    // `%gd` 是 reflog 选择子（`stash@{0}`），`%s` 是标题；中间用 TAB 隔开 ——
    // 标题里不会有 TAB（git 把控制字符当空白折掉了），也不会有换行
    let out = run(root.as_ref(), &["stash", "list", "--format=%gd%x09%s"])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let (sel, msg) = l.split_once('\t')?;
            let n = sel.strip_prefix("stash@{")?.strip_suffix('}')?.parse().ok()?;
            Some(Stash { index: n, message: msg.to_string() })
        })
        .collect())
}

/// `git stash push`：把已跟踪文件的改动（暂存区 + 工作区）收进 stash，工作区回到 HEAD。
///
/// **不带 `-u`**：和 VS Code 的「Stash」、IDEA 的默认一样，未跟踪的文件留在原地 ——
/// 它们不挡切分支，而收进去再放出来反而可能撞上同名文件。
///
/// git 在没什么可收时打一句 "No local changes to save" **退出码是 0**，
/// 界面上会变成一句假的「已收进 stash」。这里把它翻成错误。
pub fn stash_push(root: impl AsRef<Path>) -> R<()> {
    let out = run(root.as_ref(), &["stash", "push"])?;
    if out.contains("No local changes to save") {
        return Err(Error::Git("没有可以收进 stash 的改动（未跟踪的文件不算）".into()));
    }
    Ok(())
}

/// `git stash pop`：把最新的 stash 放回工作区并删掉它。
///
/// 撞上冲突时 git 退出码非 0、stash **留着不删**，工作区带着冲突标记 ——
/// 那正是用户该看到的（改动列表里出现「冲突中」），错误文本照 git 的原话给。
pub fn stash_pop(root: impl AsRef<Path>) -> R<()> {
    run(root.as_ref(), &["stash", "pop"]).map(|_| ())
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
    // 走 drain_both 而不是 run：pre-commit 钩子想打印多少打印多少，
    // 而**掐掉子进程会让退出码失去意义** —— 那时「提交成功但钩子话多」和
    // 「提交失败」就分不出来了，而把一次成功的提交报成失败，
    // 会让用户照着那句话再提交一次。
    // 成败也在这里自己判而不交给一层通用壳：分档要**两份输出都看**，
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
