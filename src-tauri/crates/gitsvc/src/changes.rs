//! 工作区的改动：取差异、暂存 / 取消暂存、丢弃、按块应用补丁。

use super::*;

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

/// 把一段 patch 应用到暂存区（issue #33 ⑫ 按块暂存）。`reverse` = 从暂存区撤掉。
///
/// patch 写进临时文件再交给 git，而不是走 stdin：`git_cmd` 把所有子进程的 stdin
/// 都接到了 /dev/null（后台调用挂着等输入是一整类事故），为这一条开口子不值。
/// 临时文件用完就删；删不掉不算错。
///
/// `--recount`：前端拆出来的单块 patch 行数是原样的，本该对；但 `@@` 头里的计数
/// 一旦对不上（比如末尾没有换行的文件）git 就整份拒收，让它自己重数一遍更稳。
pub fn apply_cached(root: impl AsRef<Path>, patch: &str, reverse: bool) -> R<()> {
    apply_patch(root.as_ref(), patch, true, reverse)
}

/// 把一段 patch 应用到**工作区**（issue #38 撤销一块：`reverse = true`）。
///
/// 差异视图工作区那一侧比的是「工作区 vs 暂存区」，所以 `-R` 之后那几行回到的是
/// 暂存区里的样子（没暂存过就是 HEAD）—— 和界面上左边那列一致，不多不少。
/// 动的是盘上的文件，调用方做完要 `worktree.changed()` 让开着的编辑器重读。
/// 不带 `--index`：只改盘不碰暂存区，暂存过的那部分留着。
pub fn apply_worktree(root: impl AsRef<Path>, patch: &str, reverse: bool) -> R<()> {
    apply_patch(root.as_ref(), patch, false, reverse)
}

/// `apply_cached` / `apply_worktree` 共用的那一段：写临时文件、拼参数、用完删。
/// 两个入口只差一个 `--cached` —— 抄第二份的话「临时文件删不掉不算错」「`--recount`」
/// 这两条就要各记一遍。
fn apply_patch(root: &Path, patch: &str, cached: bool, reverse: bool) -> R<()> {
    let tmp = std::env::temp_dir().join(format!(
        "lite-ide-hunk-{}-{}.patch",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, patch).map_err(|e| Error::Git(format!("写不了临时 patch：{e}")))?;
    let tmp_s = tmp.to_string_lossy().into_owned();
    let mut args = vec!["apply", "--recount"];
    if cached {
        args.push("--cached");
    }
    if reverse {
        args.push("-R");
    }
    args.push("--");
    args.push(&tmp_s);
    let r = run(root, &args).map(|_| ());
    let _ = std::fs::remove_file(&tmp);
    r
}
