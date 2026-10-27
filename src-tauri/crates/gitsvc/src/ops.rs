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

/// 现在在哪个分支（欢迎页的最近项目那一行，docs/TAKEOVER.md G27）：**读 `HEAD` 文件，不起 git** ——
/// 欢迎页一次列十来个项目，每个起一条 `git` 要几十毫秒，还要先过信任检查（`trust`，别人的目录里 `.git/config` 能让 git 执行命令）。
/// 读一个文件不执行任何东西。分离头指针给 7 位 sha；不是仓库、读不到都是 None
pub fn head_branch(root: impl AsRef<Path>) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(root.as_ref())?.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(r) => Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_string()),
        None if head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit()) => Some(head[..7].to_string()),
        None => None,
    }
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
        return Err(Error::Git("合并不支持「跳过」：请解决冲突后继续，或中止合并".into()));
    }
    // 两路都读：「还有冲突没解决」那句 git 写在 stdout 里、stderr 是空的（单测撞见：只读 stderr 时报出来的是一句「git -c 失败」）；
    // 继续时还可能跑提交钩子、话多 —— 照提交那样读完不杀（`drain_both`）
    let (out, _, err, ok) = drain_both(root.as_ref(), &["-c", "core.editor=true", kind.verb(), flag], MAX_STDOUT_BYTES)?;
    if ok {
        return Ok(());
    }
    let raw = [err.as_str(), String::from_utf8_lossy(&out).trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n");
    Err(Error::Git(if unresolved(&raw) {
        "仍有未解决的冲突：请解决并暂存（＋）后再继续".into()
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

// ─────────────────── 交互式变基（IDEA 的 Interactively Rebase from Here） ───────────────────

/// 变基计划里的一个提交（从旧到新）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanCommit {
    pub sha: String,
    pub short: String,
    pub subject: String,
    /// 整段提交说明（改说明、合并说明时要用）
    pub message: String,
}

/// 从 `from` 这个提交（含）到 HEAD 的提交，从旧到新。不在当前这条线上、中间有合并提交的拒绝：
/// 合并提交的变基（`--rebase-merges`）规矩复杂得多，这一版只做直线的（IDEA 的对话框遇到合并提交也是另一套）
pub fn rebase_plan(root: impl AsRef<Path>, from: &str) -> R<Vec<PlanCommit>> {
    let root = root.as_ref();
    if run(root, &["merge-base", "--is-ancestor", from, "HEAD"]).is_err() {
        return Err(Error::Git("此提交不在当前分支的历史上，无法从此处变基".into()));
    }
    // 范围：from 本身也要（「从这里」含这一条）。from 是根提交时没有父，用 `HEAD` 往回全列、再截到 from
    let has_parent = run(root, &["rev-parse", "--verify", "--quiet", &format!("{from}^")]).is_ok();
    let range = if has_parent { format!("{from}^..HEAD") } else { "HEAD".to_string() };
    let merges = run(root, &["rev-list", "--merges", &range])?;
    if !merges.trim().is_empty() {
        return Err(Error::Git("此范围包含合并提交，当前版本仅支持整理线性历史".into()));
    }
    let out = run(root, &["log", "--reverse", "--format=%H%x1f%h%x1f%s%x1f%B%x1e", &range])?;
    let mut list: Vec<PlanCommit> = out
        .split('\u{1e}')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').splitn(4, '\u{1f}').collect();
            (f.len() == 4 && !f[0].is_empty()).then(|| PlanCommit { sha: f[0].into(), short: f[1].into(), subject: f[2].into(), message: f[3].trim_end().to_string() })
        })
        .collect();
    if !has_parent {
        let full = run(root, &["rev-parse", from])?;
        if let Some(i) = list.iter().position(|c| c.sha == full.trim()) {
            list.drain(..i);
        }
    }
    Ok(list)
}

/// 对一个提交做什么（`git rebase -i` 的那几个词）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Pick(String),
    /// 改说明（新的整段说明）
    Reword(String, String),
    /// 并进上一个、两段说明接在一起
    Squash(String),
    /// 并进上一个、只留上一个的说明
    Fixup(String),
    Drop(String),
}

impl Step {
    fn sha(&self) -> &str {
        match self {
            Step::Pick(s) | Step::Reword(s, _) | Step::Squash(s) | Step::Fixup(s) | Step::Drop(s) => s,
        }
    }
}

/// 变基做完了，还是停在冲突上（那时 [`op_state`] 认得出、界面上出继续 / 放弃）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseOutcome {
    Done,
    Stopped,
}

/// 把单引号包起来给 sh：`'` 换成 `'\''`
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 计划 → git 的 todo 文件内容，外加要写的说明文件（文件名, 内容）。
///
/// **不让 git 弹编辑器**：改说明、合并说明本来要开编辑器让人写。这里改成 `pick` / `fixup` 之后紧跟一行
/// `exec git commit --amend -F <说明文件>` —— 说明是对话框里写好的、合并说明在这里按次序拼好（上一段 + 空行 + 这一段）。
/// `--no-verify`：变基自己挑提交时不跑提交钩子，改说明这一下也不该跑（不然一个慢钩子每改一次说明跑一遍）。
/// exec 里的 git 吃到的是外面那条 git 的 `-c`（git 把它们放进 `GIT_CONFIG_PARAMETERS` 传给子进程），加固照样在
pub fn rebase_todo(plan: &[PlanCommit], steps: &[Step], msg_dir: &Path) -> R<(String, Vec<(PathBuf, String)>)> {
    let want: std::collections::HashSet<&str> = plan.iter().map(|c| c.sha.as_str()).collect();
    let got: std::collections::HashSet<&str> = steps.iter().map(|s| s.sha()).collect();
    if want != got || steps.len() != plan.len() {
        return Err(Error::Git("变基计划与提交范围不一致（多出或缺少提交），未执行。如需删除提交，请将其标记为「删除」".into()));
    }
    let msg_of = |sha: &str| plan.iter().find(|c| c.sha == sha).map(|c| c.message.clone()).unwrap_or_default();
    let mut todo = String::new();
    let mut files = Vec::new();
    let mut acc: Option<String> = None;
    let amend = |todo: &mut String, files: &mut Vec<(PathBuf, String)>, text: String| {
        let p = msg_dir.join(format!("msg-{}.txt", files.len()));
        todo.push_str(&format!("exec git commit --amend --allow-empty --no-verify -F {}\n", sh_quote(&p.to_string_lossy())));
        files.push((p, text));
    };
    for (i, s) in steps.iter().enumerate() {
        match s {
            Step::Pick(sha) => {
                todo.push_str(&format!("pick {sha}\n"));
                acc = Some(msg_of(sha));
            }
            Step::Reword(sha, m) => {
                todo.push_str(&format!("pick {sha}\n"));
                amend(&mut todo, &mut files, m.clone());
                acc = Some(m.clone());
            }
            Step::Squash(sha) | Step::Fixup(sha) => {
                let Some(prev) = acc.clone() else {
                    return Err(Error::Git(format!("第 {} 个提交需要合并到上一个提交，但其前面没有保留的提交", i + 1)));
                };
                todo.push_str(&format!("fixup {sha}\n"));
                if matches!(s, Step::Squash(_)) {
                    let m = format!("{prev}\n\n{}", msg_of(sha));
                    amend(&mut todo, &mut files, m.clone());
                    acc = Some(m);
                }
            }
            Step::Drop(sha) => todo.push_str(&format!("drop {sha}\n")),
        }
    }
    Ok((todo, files))
}

/// 照计划做交互式变基。工作区有没提交的改动（未跟踪的不算）拒绝：变基中途要切提交，改动会挡路或者被卷进去。
/// `scratch`：放 todo 和说明文件的目录（应用自己的目录）—— 变基停在冲突上、过一会再继续时 exec 那几行还要读说明文件，所以不能用完就删，
/// 下一次变基开始时清
pub fn rebase_interactive(root: impl AsRef<Path>, from: &str, steps: &[Step], scratch: &Path) -> R<RebaseOutcome> {
    let root = root.as_ref();
    if op_state(root).is_some() {
        return Err(Error::Git("仓库有进行中的操作（变基或合并等），请先在 Git 面板中继续或中止".into()));
    }
    let st = status(root)?;
    if st.entries.iter().any(|e| !e.untracked) {
        return Err(Error::Git("存在未提交的更改，请先提交或储藏后再变基".into()));
    }
    let plan = rebase_plan(root, from)?;
    #[allow(clippy::disallowed_methods)] // 清的是我们自己上一次变基留下的 todo 和说明文件（目录是调用方给的应用目录）
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch).map_err(|e| Error::Git(format!("无法创建临时目录 {}：{e}", scratch.display())))?;
    let (todo, files) = rebase_todo(&plan, steps, scratch)?;
    let todo_path = scratch.join("todo.txt");
    for (p, text) in files.iter().chain(std::iter::once(&(todo_path.clone(), todo))) {
        #[allow(clippy::disallowed_methods)] // 新建的临时目录里写我们自己的文件
        std::fs::write(p, text).map_err(|e| Error::Git(format!("无法写入 {}：{e}", p.display())))?;
    }
    let seq = format!("cp {}", sh_quote(&todo_path.to_string_lossy()));
    let has_parent = run(root, &["rev-parse", "--verify", "--quiet", &format!("{from}^")]).is_ok();
    let onto = format!("{from}^");
    let mut args = vec!["-c", "core.editor=true", "-c"];
    let seq_cfg = format!("sequence.editor={seq}");
    args.push(&seq_cfg);
    args.extend(["rebase", "-i"]);
    if has_parent {
        args.push(&onto);
    } else {
        args.push("--root");
    }
    let (out, _, err, ok) = drain_both(root, &args, MAX_STDOUT_BYTES)?;
    if op_state(root).is_some() {
        return Ok(RebaseOutcome::Stopped);
    }
    if !ok {
        let raw = [err.as_str(), String::from_utf8_lossy(&out).trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n");
        return Err(Error::Git(if raw.is_empty() { "变基失败".into() } else { raw }));
    }
    Ok(RebaseOutcome::Done)
}
