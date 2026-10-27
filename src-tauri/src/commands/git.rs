//! Git：状态、差异、暂存、提交、历史、blame、stash、分支、工作树、仓库信任、控制台。

use super::*;

/// 跑过的 git，最新的在前。
///
/// 只在内存里（`gitsvc::console`），关掉应用就没 —— 它的用途是
/// 「刚才那条为什么失败」，不是考古。判据、上限和打码全在那边。
#[tauri::command]
pub fn git_console(root: String) -> Vec<GitCmdDto> {
    // 只给这个窗口的仓库的（多窗口第 4 步）：环是整个进程一份
    gitsvc::console::entries_under(std::path::Path::new(&root))
        .into_iter()
        .map(|e| GitCmdDto {
            ms: e.ms,
            cwd: e.cwd,
            argv: e.argv,
            code: e.code,
            dur_ms: e.dur_ms,
            err: e.err,
            err_truncated: e.err_truncated,
        })
        .collect()
}

/// 清空 Git 控制台**里这个仓库的那些**。只碰内存里那个环，盘上本来就没有东西。
/// 别的窗口（别的仓库）的记录留着 —— 在 A 里点「清空」，B 的控制台不该跟着空掉
#[tauri::command]
pub fn clear_git_console(root: String) {
    gitsvc::console::clear_under(std::path::Path::new(&root));
}

/// 找 `path` 所属的仓库根。不是仓库返回 null —— 这是正常情况，
/// 界面据此让整块 Git 功能隐身，而不是弹错误。
#[tauri::command]
pub async fn git_root(path: String) -> Option<String> {
    // 这条起 git 子进程（rev-parse），和别的 git 命令一样不许在主线程上等它。
    // 后台任务本身挂了（几乎不可能）也当「不是仓库」—— 界面上的意思是一样的
    blocking(move || Ok(gitsvc::discover(&path).map(|p| p.to_string_lossy().into_owned())))
        .await
        .unwrap_or(None)
}

fn op_dto(s: gitsvc::OpState) -> GitOpDto {
    let kind = match s.kind {
        gitsvc::OpKind::Rebase => "rebase",
        gitsvc::OpKind::Merge => "merge",
        gitsvc::OpKind::CherryPick => "cherry-pick",
        gitsvc::OpKind::Revert => "revert",
    };
    GitOpDto { kind: kind.into(), step: s.step.map(|(a, _)| a), total: s.step.map(|(_, b)| b) }
}

/// 做到一半的操作：继续 / 跳过这一步 / 放弃（`action`：`continue` / `skip` / `abort`，docs/TAKEOVER.md G25）。
/// 停在哪个操作上由这边现读（不信前端传来的）：界面上那块是刷新前的样子，这期间你可能在终端里已经放弃了
#[tauri::command]
pub async fn git_op(root: String, action: String) -> Result<(), String> {
    blocking(move || {
        let act = match action.as_str() {
            "continue" => gitsvc::OpAction::Continue,
            "skip" => gitsvc::OpAction::Skip,
            "abort" => gitsvc::OpAction::Abort,
            other => return Err(format!("未知的操作：{other}")),
        };
        let st = gitsvc::op_state(&root).ok_or("仓库当前没有进行中的操作（可能已在其他位置完成或中止）")?;
        gitsvc::op_run(&root, st.kind, act).map_err(|e| format!("{e}"))
    })
    .await
}

/// 交互式变基（docs/TAKEOVER.md G25）：从 `from`（含）到 HEAD 的提交，从旧到新。不在这条线上、中间有合并提交的报错
#[tauri::command]
pub async fn git_rebase_plan(root: String, from: String) -> Result<Vec<RebaseCommitDto>, String> {
    blocking(move || {
        let plan = gitsvc::rebase_plan(&root, &from).map_err(|e| format!("{e}"))?;
        Ok(plan.into_iter().map(|c| RebaseCommitDto { sha: c.sha, short: c.short, subject: c.subject, message: c.message }).collect())
    })
    .await
}

/// 计划里的一步：`action` 是 pick / reword / squash / fixup / drop，`message` 只有 reword 用
#[derive(serde::Deserialize)]
pub struct RebaseStepArg {
    pub sha: String,
    pub action: String,
    #[serde(default)]
    pub message: String,
}

/// 照计划做交互式变基。回 `done`（做完了）或 `stopped`（停在冲突上，Git 面板里继续 / 放弃）。
/// 临时文件（todo、说明）放在应用缓存目录里：停在冲突上之后继续时还要用，下一次变基开始时清
#[tauri::command]
pub async fn git_rebase_run(app: tauri::AppHandle, root: String, from: String, steps: Vec<RebaseStepArg>) -> Result<String, String> {
    use tauri::Manager;
    let scratch = app.path().app_cache_dir().map_err(|e| format!("无法获取缓存目录：{e}"))?.join("rebase");
    blocking(move || {
        let steps = steps
            .into_iter()
            .map(|s| match s.action.as_str() {
                "pick" => Ok(gitsvc::Step::Pick(s.sha)),
                "reword" => Ok(gitsvc::Step::Reword(s.sha, s.message)),
                "squash" => Ok(gitsvc::Step::Squash(s.sha)),
                "fixup" => Ok(gitsvc::Step::Fixup(s.sha)),
                "drop" => Ok(gitsvc::Step::Drop(s.sha)),
                other => Err(format!("未知的操作：{other}")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        match gitsvc::rebase_interactive(&root, &from, &steps, &scratch).map_err(|e| format!("{e}"))? {
            gitsvc::RebaseOutcome::Done => Ok("done".into()),
            gitsvc::RebaseOutcome::Stopped => Ok("stopped".into()),
        }
    })
    .await
}

/// 读一次仓库状态。分支、领先落后、变更文件一次拿全。
#[tauri::command]
pub async fn git_status(root: String) -> Result<GitStatusDto, String> {
    blocking(move || {
        let st = gitsvc::status_full(&root).map_err(|e| format!("{e}"))?;
        let op = gitsvc::op_state(&root).map(op_dto);
        Ok(GitStatusDto {
            op,
            root,
            branch: st.branch,
            upstream: st.upstream,
            ahead: st.ahead,
            behind: st.behind,
            detached: st.detached,
            head: st.head,
            unborn: st.unborn,
            untracked_dirs: st.untracked_dirs,
            truncated: st.truncated,
            entries: st
                .entries
                .into_iter()
                .map(|e| GitEntryDto {
                    staged: e.staged(),
                    unstaged: e.unstaged(),
                    index: e.index.to_string(),
                    work: e.work.to_string(),
                    path: e.path,
                    untracked: e.untracked,
                    conflicted: e.conflicted,
                    orig: e.orig,
                })
                .collect(),
        })
    })
    .await
}

#[tauri::command]
pub async fn git_diff(
    root: String,
    path: String,
    staged: bool,
    untracked: bool,
) -> Result<DiffDto, String> {
    blocking(move || {
        gitsvc::diff(&root, &path, staged, untracked)
            .map(DiffDto::from)
            .map_err(|e| format!("{e}"))
    })
    .await
}

#[tauri::command]
pub async fn git_stage(root: String, paths: Vec<String>) -> Result<(), String> {
    blocking(move || {
        gitsvc::stage(&root, &paths).map_err(|e| format!("暂存失败：{e}"))
    })
    .await
}

#[tauri::command]
pub async fn git_unstage(root: String, paths: Vec<String>) -> Result<(), String> {
    blocking(move || {
        gitsvc::unstage(&root, &paths).map_err(|e| format!("取消暂存失败：{e}"))
    })
    .await
}

/// 丢弃工作区改动。跟踪的**不可撤销** —— 前端必须先让用户确认过才准调；未跟踪的进废纸篓
#[tauri::command]
pub async fn git_discard(
    root: String,
    paths: Vec<String>,
    untracked: Vec<String>,
) -> Result<(), String> {
    crate::diag!("git_discard {} 个跟踪 + {} 个未跟踪", paths.len(), untracked.len());
    blocking(move || gitsvc::discard(&root, &paths, &untracked, fsservice::move_all_to_trash).map_err(|e| format!("放弃更改失败：{e}"))).await
}

/// 提交。**这条是最该挪出主线程的一条** —— `pre-commit` 钩子跑什么
/// 完全是仓库说了算，跑一遍 eslint 三十秒也不奇怪。
#[tauri::command]
pub async fn git_commit(root: String, message: String, amend: bool) -> Result<String, String> {
    blocking(move || gitsvc::commit(&root, &message, amend).map_err(|e| format!("{e}"))).await
}


#[tauri::command]
pub async fn git_log_entries(
    root: String,
    limit: usize,
    all: bool,
    path: String,
    lines: Option<(u32, u32)>,
) -> Result<Vec<LogEntryDto>, String> {
    blocking(move || {
        let es = gitsvc::log_entries(&root, limit, all, &path, lines).map_err(|e| format!("读取历史失败：{e}"))?;
        Ok(es
            .into_iter()
            .map(|c| LogEntryDto {
                sha: c.sha,
                short: c.short,
                author: c.author,
                email: c.email,
                when: c.when,
                date: c.date,
                subject: c.subject,
                parents: c.parents,
                refs: c.refs,
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn git_commit_files(root: String, sha: String) -> Result<Vec<GitEntryDto>, String> {
    blocking(move || {
        let es = gitsvc::commit_files(&root, &sha).map_err(|e| format!("读取提交内容失败：{e}"))?;
        Ok(es
            .into_iter()
            .map(|e| GitEntryDto {
                staged: true,
                unstaged: false,
                index: e.index.to_string(),
                work: e.work.to_string(),
                path: e.path,
                untracked: false,
                conflicted: false,
                orig: e.orig,
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn git_blame(root: String, path: String) -> Result<BlameDto, String> {
    // 大文件的 blame 要几百毫秒，别占主线程
    blocking(move || {
        let (hunks, truncated) = gitsvc::blame(&root, &path).map_err(|e| format!("{e}"))?;
        Ok(BlameDto {
            hunks: hunks
                .into_iter()
                .map(|h| BlameHunkDto {
                    sha: h.sha,
                    short: h.short,
                    author: h.author,
                    time: h.time,
                    summary: h.summary,
                    start: h.start,
                    count: h.count,
                })
                .collect(),
            truncated,
        })
    })
    .await
}

/// 按块暂存（issue #33 ⑫）：一段 patch 应用到暂存区；`reverse` = 撤掉
#[tauri::command]
pub async fn git_apply_cached(root: String, patch: String, reverse: bool) -> Result<(), String> {
    blocking(move || {
        gitsvc::apply_cached(&root, &patch, reverse).map_err(|e| format!("{e}"))
    })
    .await
}

/// 撤销一块（issue #38）：一段 patch 反向应用到工作区。盘上的文件会变
#[tauri::command]
pub async fn git_apply_worktree(root: String, patch: String, reverse: bool) -> Result<(), String> {
    blocking(move || {
        gitsvc::apply_worktree(&root, &patch, reverse).map_err(|e| format!("{e}"))
    })
    .await
}

#[tauri::command]
pub async fn git_stash_list(root: String) -> Result<Vec<StashDto>, String> {
    blocking(move || {
        gitsvc::stash_list(&root)
            .map(|v| v.into_iter().map(|s| StashDto { index: s.index, message: s.message }).collect())
            .map_err(|e| format!("{e}"))
    })
    .await
}

#[tauri::command]
pub async fn git_stash_push(root: String) -> Result<(), String> {
    blocking(move || {
        gitsvc::stash_push(&root).map_err(|e| format!("{e}"))
    })
    .await
}

#[tauri::command]
pub async fn git_stash_pop(root: String) -> Result<(), String> {
    blocking(move || {
        gitsvc::stash_pop(&root).map_err(|e| format!("{e}"))
    })
    .await
}

/// 文件在 HEAD 里的内容，编辑器拿它当基线在前端实时算改动行（issue #33 ④）。
/// `None` = 不在 HEAD 里（新文件 / 还没有提交），界面上不标。
/// 复用 `DiffDto`：要传的就是「一段文本 + 有没有被上限截断」，形状一样。
#[tauri::command]
pub async fn git_head_text(root: String, path: String) -> Result<Option<DiffDto>, String> {
    blocking(move || {
        gitsvc::head_text(&root, &path)
            .map(|d| d.map(DiffDto::from))
            .map_err(|e| format!("{e}"))
    })
    .await
}

/// 某个版本里一个文件的全文（`rev` 空 = 暂存区 / `HEAD` / 提交号），差异视图「展开 N 行未改动」用（`gitsvc::file_at`）。
/// `None` = 那个版本里没有这个文件
#[tauri::command]
pub async fn git_file_at(root: String, rev: String, path: String) -> Result<Option<DiffDto>, String> {
    blocking(move || {
        gitsvc::file_at(&root, &rev, &path)
            .map(|d| d.map(DiffDto::from))
            .map_err(|e| format!("{e}"))
    })
    .await
}

#[tauri::command]
pub async fn git_commit_diff(root: String, sha: String, path: String) -> Result<DiffDto, String> {
    blocking(move || {
        gitsvc::commit_diff(&root, &sha, &path)
            .map(DiffDto::from)
            .map_err(|e| format!("{e}"))
    })
    .await
}

/// 「和本地比较」（issue #39）：那次提交到现在的工作区
#[tauri::command]
pub async fn git_commit_vs_worktree(root: String, sha: String, path: String) -> Result<DiffDto, String> {
    blocking(move || {
        gitsvc::commit_vs_worktree(&root, &sha, &path)
            .map(DiffDto::from)
            .map_err(|e| format!("{e}"))
    })
    .await
}

/// cherry-pick（issue #39）。撞冲突时报错但盘上已经变了 —— 前端失败也要刷新
#[tauri::command]
pub async fn git_cherry_pick(root: String, sha: String) -> Result<String, String> {
    crate::diag!("git_cherry_pick {sha}");
    blocking(move || gitsvc::cherry_pick(&root, &sha).map_err(|e| format!("{e}"))).await
}

#[tauri::command]
pub async fn git_branches(root: String) -> Result<Vec<BranchDto>, String> {
    blocking(move || {
        let bs = gitsvc::branches(&root).map_err(|e| format!("读取分支失败：{e}"))?;
        Ok(bs
            .into_iter()
            .map(|b| BranchDto {
                name: b.name,
                sha: b.sha,
                upstream: b.upstream,
                is_head: b.is_head,
                is_remote: b.is_remote,
                when: b.when,
                subject: b.subject,
            })
            .collect())
    })
    .await
}

/// 切分支。**「本地改动挡着」单独分一档**（见 [`SwitchErrDto`]），其余原样上抛。
#[tauri::command]
pub async fn git_switch(
    root: String,
    name: String,
    create: bool,
    from: Option<String>,
) -> Result<String, SwitchErrDto> {
    let from = from.unwrap_or_default();
    crate::diag!("git_switch {name} create={create} from={from}");
    let r = tauri::async_runtime::spawn_blocking(move || {
        gitsvc::switch_branch_from(&root, &name, create, &from)
    })
    .await;
    match r {
        Err(e) => Err(SwitchErrDto {
            kind: "other".into(),
            message: format!("后台任务未完成：{e}"),
            files: Vec::new(),
            raw: String::new(),
        }),
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => {
            let files = match &e {
                gitsvc::Error::LocalChanges { files, .. } => files.clone(),
                _ => Vec::new(),
            };
            Err(SwitchErrDto {
                kind: if files.is_empty() { "other" } else { "local-changes" }.into(),
                message: e.to_string(),
                raw: e.raw().to_string(),
                files,
            })
        }
    }
}

#[tauri::command]
pub async fn git_worktrees(root: String) -> Result<Vec<WorktreeDto>, String> {
    blocking(move || {
        let ws = gitsvc::worktrees(&root).map_err(|e| format!("读取工作树失败：{e}"))?;
        Ok(ws
            .into_iter()
            .map(|w| WorktreeDto {
                path: w.path,
                sha: w.sha,
                branch: w.branch,
                detached: w.detached,
                bare: w.bare,
                locked: w.locked,
                current: w.current,
            })
            .collect())
    })
    .await
}

/// 新建工作树，返回新目录的绝对路径 —— 前端可以直接把它当项目根打开。
#[tauri::command]
pub async fn git_worktree_add(root: String, path: String, branch: String) -> Result<String, String> {
    crate::diag!("git_worktree_add path={path} branch={branch}");
    blocking(move || gitsvc::worktree_add(&root, &path, &branch).map_err(|e| format!("{e}"))).await
}

/// 移除工作树。**会删掉那个目录**，前端必须先确认。
#[tauri::command]
pub async fn git_worktree_remove(root: String, path: String, force: bool) -> Result<(), String> {
    crate::diag!("git_worktree_remove path={path} force={force}");
    blocking(move || gitsvc::worktree_remove(&root, &path, force).map_err(|e| format!("{e}"))).await
}

/// 删本地分支。`force` = `-D`，只在用户看过「还有没合并的提交」之后才传 true。
#[tauri::command]
pub async fn git_branch_delete(root: String, name: String, force: bool) -> Result<(), BranchErrDto> {
    crate::diag!("git_branch_delete {name} force={force}");
    let r = tauri::async_runtime::spawn_blocking(move || gitsvc::branch_delete(&root, &name, force)).await;
    match r {
        Err(e) => Err(BranchErrDto { kind: "other".into(), message: format!("后台任务未完成：{e}"), raw: String::new() }),
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(BranchErrDto {
            kind: if matches!(e, gitsvc::Error::NotMerged { .. }) { "not-merged" } else { "other" }.into(),
            message: e.to_string(),
            raw: e.raw().to_string(),
        }),
    }
}

#[tauri::command]
pub async fn git_branch_rename(root: String, old: String, new: String) -> Result<(), String> {
    crate::diag!("git_branch_rename {old} -> {new}");
    blocking(move || gitsvc::branch_rename(&root, &old, &new).map_err(|e| format!("{e}"))).await
}


fn trust_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|d| d.join("trust.json"))
        .map_err(|e| format!("无法获取应用数据目录：{e}"))
}

/// 扫一遍仓库的 config。只跑 `git config --list` 和 `rev-parse`，两条都不执行配置
/// （`gitsvc::trust` 头上有实测）。走阻塞池：网络卷上读 config 也可能慢。
#[tauri::command]
pub async fn git_trust_scan(app: tauri::AppHandle, root: String) -> Result<TrustScanDto, String> {
    let file = trust_file(&app)?;
    blocking(move || {
        let s = gitsvc::trust::scan(&root).map_err(|e| format!("{e}"))?;
        let trusted = s.suspects.is_empty() || crate::trust_store::is_trusted(&file, &root, &s.fingerprint);
        if !trusted {
            applog::write(
                applog::Level::Warn,
                "trust",
                &format!("{root} 的 .git/config 包含 {} 条可执行命令的配置，Git 未启用", s.suspects.len()),
            );
        }
        Ok(TrustScanDto {
            root,
            trusted,
            suspects: s
                .suspects
                .into_iter()
                .map(|x| TrustSuspectDto { key: x.key, value: x.value, origin: x.origin })
                .collect(),
            hooks: s.hooks,
            fingerprint: s.fingerprint,
        })
    })
    .await
}

/// 用户点了「信任这个仓库」。记的是扫描时那份指纹：config 再变就要重问。
#[tauri::command]
pub async fn git_trust_grant(app: tauri::AppHandle, root: String, fingerprint: String) -> Result<(), String> {
    let file = trust_file(&app)?;
    crate::diag!("git_trust_grant {root}");
    blocking(move || crate::trust_store::grant(&file, &root, &fingerprint).map_err(|e| format!("无法保存信任设置：{e}"))).await
}
