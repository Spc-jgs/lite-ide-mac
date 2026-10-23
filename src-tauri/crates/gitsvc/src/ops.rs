//! 做到一半的操作：变基、合并、cherry-pick、回退停在冲突上（docs/TAKEOVER.md G25）。
//!
//! 原来一样都不认：拉取选「变基」撞上冲突，仓库就停在变基中途，界面上只有一堆冲突文件，「继续 / 放弃」要去终端敲。
//! 交互式变基更离不开它 —— 它本来就会停在冲突上。所以先认出「停在哪」、给出继续 / 跳过 / 放弃，再做交互式变基。
//!
//! **认状态读 git 自己的标记文件，不起进程**：状态每次存盘都要刷一遍，多起一个 `git rev-parse` 就是每次存盘多一个进程。
//! 认的是这几样标记：`rebase-merge/`（交互式和默认的 merge 后端）、`rebase-apply/`（am 后端）、`MERGE_HEAD`、`CHERRY_PICK_HEAD`、`REVERT_HEAD`。
//! 没去读 git 的源码对表；靠单测在真仓库里让变基、合并、cherry-pick、回退各停一次冲突，认出来的是对的那个（`tests.rs`）。

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Rebase,
    Merge,
    CherryPick,
    Revert,
}

impl OpKind {
    /// 交给 git 的子命令名
    fn verb(self) -> &'static str {
        match self {
            OpKind::Rebase => "rebase",
            OpKind::Merge => "merge",
            OpKind::CherryPick => "cherry-pick",
            OpKind::Revert => "revert",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpState {
    pub kind: OpKind,
    /// 变基做到第几步、一共几步（`rebase-merge/msgnum` / `end`）；别的没有
    pub step: Option<(u32, u32)>,
}

/// 仓库的 git 目录：`.git` 是目录就是它；是文件（工作树、子模块）就照里面那句 `gitdir: …` 走
pub fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    let m = std::fs::symlink_metadata(&dot).ok()?;
    if m.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let p = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let p = Path::new(p);
    Some(if p.is_absolute() { p.to_path_buf() } else { root.join(p) })
}

fn num(p: &Path) -> Option<u32> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

/// 这个仓库停在哪个操作中间；没有就是 None
pub fn op_state(root: impl AsRef<Path>) -> Option<OpState> {
    let g = git_dir(root.as_ref())?;
    for (dir, now, end) in [("rebase-merge", "msgnum", "end"), ("rebase-apply", "next", "last")] {
        let d = g.join(dir);
        if d.is_dir() {
            let step = num(&d.join(now)).zip(num(&d.join(end)));
            return Some(OpState { kind: OpKind::Rebase, step });
        }
    }
    for (f, kind) in [("MERGE_HEAD", OpKind::Merge), ("CHERRY_PICK_HEAD", OpKind::CherryPick), ("REVERT_HEAD", OpKind::Revert)] {
        if g.join(f).is_file() {
            return Some(OpState { kind, step: None });
        }
    }
    None
}

/// 继续 / 跳过这一步 / 放弃
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpAction {
    Continue,
    Skip,
    Abort,
}

/// 继续、跳过、放弃。**不弹编辑器**：`core.editor=true`（继续时 git 要改提交说明就用它原来的那句）—— 我们没有终端给它开 vim，
/// 不设的话 git 起一个编辑器、等它退出，这条命令就挂在那儿。合并没有「跳过」
pub fn op_run(root: impl AsRef<Path>, kind: OpKind, action: OpAction) -> R<()> {
    let flag = match action {
        OpAction::Continue => "--continue",
        OpAction::Skip => "--skip",
        OpAction::Abort => "--abort",
    };
    if kind == OpKind::Merge && action == OpAction::Skip {
        return Err(Error::Git("合并没有「跳过」：解决冲突后继续，或者放弃合并".into()));
    }
    // 两路都读：「还有冲突没解决」那句 git 写在 stdout 里、stderr 是空的（单测撞见：只读 stderr 时报出来的是一句「git -c 失败」）；
    // 继续时还可能跑提交钩子、话多 —— 照提交那样读完不杀（`drain_both`）
    let (out, _, err, ok) = drain_both(root.as_ref(), &["-c", "core.editor=true", kind.verb(), flag], MAX_STDOUT_BYTES)?;
    if ok {
        return Ok(());
    }
    let raw = [err.as_str(), String::from_utf8_lossy(&out).trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n");
    Err(Error::Git(if unresolved(&raw) {
        "还有冲突的文件没解决：解决完、暂存（＋）之后再继续".into()
    } else if raw.is_empty() {
        format!("git {} {flag} 失败", kind.verb())
    } else {
        raw
    }))
}

/// git 说「还有没解决的冲突」的那几种原话（不同子命令措辞不一样）
fn unresolved(raw: &str) -> bool {
    let low = raw.to_ascii_lowercase();
    low.contains("unmerged") || low.contains("needs merge") || low.contains("mark them as resolved") || low.contains("you must edit all merge conflicts")
}
