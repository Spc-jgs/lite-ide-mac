//! Git 拉取与推送：进度经 Channel 流式回传，可取消。

use super::*;

/// 这个 op_id 已经在跑了（issue #18 第二条）。
///
/// 走 `other` 这一档：界面对 `cancelled` / `rejected` / `auth-*` 各有专门的
/// 处理，而这条不是远程那边的事，是我们自己这边撞了号。
fn busy_err_dto(id: u32) -> RemoteErrDto {
    let m = format!("操作编号 {id} 已经有一个在跑了。这多半是界面上的并发防线漏了 —— 等它跑完再来");
    RemoteErrDto { kind: "other".to_string(), message: m.clone(), raw: m }
}

fn to_err_dto(e: gitsvc::remote::RemoteError) -> RemoteErrDto {
    use gitsvc::remote::RemoteError as E;
    let kind = match &e {
        E::Auth { https: true, .. } => "auth-https",
        E::Auth { https: false, .. } => "auth-ssh",
        E::Cancelled => "cancelled",
        E::Rejected { .. } => "rejected",
        E::Conflict { .. } => "conflict",
        E::Other { .. } | E::NoGit(_) => "other",
    };
    RemoteErrDto { kind: kind.to_string(), message: e.to_string(), raw: e.raw().to_string() }
}

/// 把 gitsvc 的进度回调接到 Tauri 的 Channel 上。
///
/// 节流已经在 gitsvc 里做过了（阶段变了或百分比变了且距上次 >100ms），
/// 这里只负责转形状 —— **业务不写在命令层**，那是这个文件的规矩。
fn pump(ch: &tauri::ipc::Channel<ProgressDto>) -> impl FnMut(gitsvc::progress::Progress) + '_ {
    move |p| {
        let _ = ch.send(ProgressDto {
            phase: p.phase,
            percent: p.percent,
            done: p.done.map(|(a, _)| a),
            total: p.done.map(|(_, b)| b),
            finished: p.finished,
        });
    }
}

/// `spawn_blocking` 自己失败时的错误（线程池满、任务 panic）。
///
/// 只有 `kind: "other"` 一档 —— 这不是 git 的错，界面照样得说点什么。
fn join_err_dto(e: impl std::fmt::Display) -> RemoteErrDto {
    RemoteErrDto {
        kind: "other".into(),
        message: format!("后台任务没跑完：{e}"),
        raw: String::new(),
    }
}

/// 抓远程。**只读，不动工作区。**
///
/// 活跑在**阻塞池**上，不在 async worker 上。`async fn` 里直接调一个
/// 阻塞几十秒的函数，占住的是 runtime 的 worker —— 而 worker 只有 2 个
/// （见 `lib.rs::install_runtime`），一次 fetch 加一次 push 就能把它占满，
/// 之后所有异步命令一起卡住。这是本轮审查里最先要修的一条。
#[tauri::command]
pub async fn git_fetch(
    root: String,
    remote: String,
    op_id: u32,
    on_progress: tauri::ipc::Channel<ProgressDto>,
    window: tauri::Window,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), RemoteErrDto> {
    let id = op_id;
    // op_id 是前端发的、每个窗口各数各的，所以按 (窗口, id) 登记
    let owner = window.label().to_string();
    // 撞号就当场退，**不能 end_remote**：表里那条是别人的，划掉它
    // 等于把那个还在跑的操作的取消能力一起划掉
    let Some(cancel) = state.begin_remote(&owner, id) else {
        return Err(busy_err_dto(id));
    };
    crate::diag!("git_fetch id={id} remote={remote}");
    let r = tauri::async_runtime::spawn_blocking(move || {
        gitsvc::remote::fetch(&root, &remote, &cancel, &mut pump(&on_progress))
    })
    .await;
    // end_remote 必须无论如何都跑到 —— 漏一次，那个 id 就永远留在表里，
    // 而 `git_cancel` 会对着一个早就结束的操作返回 true
    state.end_remote(&owner, id);
    r.map_err(join_err_dto)?.map_err(to_err_dto)
}

/// 推送当前分支。**这是第一个会改到别人东西的操作。**
///
/// `set_upstream` 只在「这个分支还没有上游」时由前端传真，
/// 而且界面上要把「它要建立什么」写出来，不做成沉默的开关。
#[tauri::command]
pub async fn git_push(
    root: String,
    remote: String,
    branch: String,
    set_upstream: bool,
    op_id: u32,
    on_progress: tauri::ipc::Channel<ProgressDto>,
    window: tauri::Window,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), RemoteErrDto> {
    let id = op_id;
    let owner = window.label().to_string();
    // 同 git_fetch：撞号当场退，不碰表里那条
    let Some(cancel) = state.begin_remote(&owner, id) else {
        return Err(busy_err_dto(id));
    };
    crate::diag!("git_push id={id} remote={remote} branch={branch} set_upstream={set_upstream}");
    let opts = gitsvc::remote::PushOpts { set_upstream };
    let r = tauri::async_runtime::spawn_blocking(move || {
        gitsvc::remote::push(&root, &remote, &branch, opts, &cancel, &mut pump(&on_progress))
    })
    .await;
    state.end_remote(&owner, id);
    r.map_err(join_err_dto)?.map_err(to_err_dto)
}

/// 把已经抓下来的上游合进当前分支。**不走网络，瞬间完成。**
///
/// 拉取 = `git_fetch` + 这个，不是 `git pull`：复合命令失败时分不清
/// 是网络断了还是合并冲突了（退出码都非零）。
/// 注释说它「瞬间完成」，那是指**不走网络**；merge/rebase 本身要检出文件，
/// 几千个文件的分支上是秒级的，所以照样走阻塞池。
#[tauri::command]
pub async fn git_merge_upstream(
    root: String,
    upstream: String,
    mode: String,
) -> Result<(), RemoteErrDto> {
    use gitsvc::remote::MergeMode;
    let mode = match mode.as_str() {
        "merge" => MergeMode::Merge,
        "rebase" => MergeMode::Rebase,
        // 默认只允许快进 —— 永远不会「拉一下，凭空多出一个合并提交」
        _ => MergeMode::FfOnly,
    };
    crate::diag!("git_merge_upstream {upstream} mode={mode:?}");
    tauri::async_runtime::spawn_blocking(move || {
        gitsvc::remote::merge_upstream(&root, &upstream, mode)
    })
    .await
    .map_err(join_err_dto)?
    .map_err(to_err_dto)
}

/// 取消一个正在跑的远程操作。
///
/// **只对 fetch 开放。** push 进行中不给取消：kill 的是本地这一端，
/// 而远程可能已经收完了 —— 一个点了之后状态不确定的取消按钮，
/// 比没有按钮更糟。前端负责不显示那个按钮，这里不拦（拦了也只是重复一遍）。
#[tauri::command]
pub fn git_cancel(id: u32, window: tauri::Window, state: tauri::State<'_, crate::state::AppState>) -> bool {
    crate::diag!("git_cancel id={id}");
    state.cancel_remote(window.label(), id)
}


/// 推上去会送出哪些提交。照 IDEA 的推送对话框：**列出提交，不是只给计数**。
#[tauri::command]
pub async fn git_outgoing(
    root: String,
    upstream: String,
    branch: String,
) -> Result<Vec<String>, String> {
    blocking(move || {
        // 20 条是对话框的显示上限 —— 再多也没人读，而且要走一趟 IPC
        gitsvc::remote::outgoing(&root, &upstream, &branch, 20).map_err(|e| format!("{e}"))
    })
    .await
}
