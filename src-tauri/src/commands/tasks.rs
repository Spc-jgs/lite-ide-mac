//! 任务（#48）：列出来、新建 tasks.json、起、停、关。业务在 `taskdefs`（定义从哪来）和 `tasksvc`（进程）。

use super::*;
use tauri::{Emitter, Manager};

/// 列出这个项目的全部任务。读盘（tasks.json、各层 package.json），走阻塞池
#[tauri::command]
pub async fn task_list(root: String) -> Result<TaskListDto, String> {
    blocking(move || {
        let f = crate::taskdefs::discover(Path::new(&root));
        Ok(TaskListDto {
            tasks: f
                .defs
                .into_iter()
                .map(|d| TaskDefDto {
                    name: d.name,
                    command: d.command,
                    cwd: d.cwd,
                    source: match d.source {
                        crate::taskdefs::Source::File => "file",
                        crate::taskdefs::Source::Package { .. } => "package",
                    }
                    .into(),
                })
                .collect(),
            file: f.file,
            problems: f.problems,
        })
    })
    .await
}

/// 新建 `.lite-ide/tasks.json`（带注释掉的例子），已经有了就不动。返回路径，前端拿去开成标签
#[tauri::command]
pub async fn task_new_file(root: String) -> Result<String, String> {
    blocking(move || {
        crate::taskdefs::create_file(Path::new(&root))
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| format!("建不了 {}：{e}", crate::taskdefs::FILE))
    })
    .await
}

/// 日志放哪：`<应用数据>/runs/<项目根的指纹>/<任务名>.log`（TASKS.md Q7：不往项目里写）。
/// 指纹用 FNV-1a：要的是「同一个项目每次都落在同一个目录」，`DefaultHasher` 不保证跨版本稳定
fn log_path(app: &tauri::AppHandle, root: &str, name: &str) -> Result<std::path::PathBuf, String> {
    let base = app.path().app_data_dir().map_err(|e| format!("找不到应用数据目录：{e}"))?;
    let mut h: u64 = 0xcbf29ce484222325;
    for b in root.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // 名字里的 `/`（`web/dev`）和别的怪字符换掉：它是文件名的一段，不能变成子目录
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    Ok(base.join("runs").join(format!("{h:016x}")).join(format!("{safe}.log")))
}

/// 跑一个任务（按名字现找定义：tasks.json 可能刚改过）。同一个窗口里同名的还在跑 → 先停它、等它真没了再起
/// （端口要先空出来，TASKS.md「重跑」）。返回这次运行的 id 和日志文件
#[tauri::command]
pub async fn task_run(window: tauri::Window, app: tauri::AppHandle, root: String, name: String) -> Result<TaskRunDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        let found = crate::taskdefs::discover(Path::new(&root));
        let def = found.defs.into_iter().find(|d| d.name == name).ok_or_else(|| format!("没有叫「{name}」的任务了（tasks.json 改过？）"))?;
        if let Some((old, task)) = st.find_run(&owner, &root, &name) {
            st.remove_run(old);
            if task.alive() {
                tasksvc::stop_group(task.pgid(), tasksvc::GRACE);
            }
        }
        let spec = tasksvc::Spec {
            shell: crate::settingsctl::terminal_shell(&st),
            command: def.command.clone(),
            cwd: Path::new(&root).join(&def.cwd),
            env: def.env,
            log: log_path(&app, &root, &name)?,
            cap: tasksvc::LOG_CAP,
        };
        let task = std::sync::Arc::new(tasksvc::Task::start(&spec).map_err(|e| format!("「{name}」起不来：{e}"))?);
        crate::diag!("task_run {name} pgid={} log={}", task.pgid(), spec.log.display());
        let id = st.insert_run(crate::state::Run { owner: owner.clone(), root, name: name.clone(), task: task.clone() });
        // 等它退出，退了告诉起它的那个窗口。一次运行一条线程，跟着任务的寿命走
        let app2 = app.clone();
        let _ = std::thread::Builder::new().name(format!("task-watch-{id}")).spawn(move || loop {
            if let Some(tasksvc::State::Exited { code, signal, stopped }) = task.wait_exit(std::time::Duration::from_secs(3600)) {
                let failed = tasksvc::State::Exited { code, signal, stopped }.failed();
                let _ = app2.emit_to(owner.as_str(), "task-exit", TaskExitDto { id, code, signal, stopped, failed });
                return;
            }
        });
        Ok(TaskRunDto { id, name, command: def.command, log: spec.log.to_string_lossy().into_owned() })
    })
    .await
}

/// 停。第一次软停（SIGINT 整组，宽限期后 SIGKILL）；软停还没完再叫一次 → 立刻强杀（同 IDEA 按钮变「强制结束」）。
/// 返回 "stopping" / "killed" / "gone"（已经没了）
#[tauri::command]
pub fn task_stop(id: u32, state: State<'_, AppState>) -> String {
    let Some(task) = state.run_task(id) else { return "gone".into() };
    match task.state() {
        tasksvc::State::Running if task.alive() => {
            task.stop(tasksvc::GRACE);
            "stopping".into()
        }
        tasksvc::State::Stopping if task.alive() => {
            task.kill();
            "killed".into()
        }
        // 组长退了但组里还有活的（`&` 出去的后台进程）：直接强杀，没什么可等的
        _ if task.alive() => {
            task.kill();
            "killed".into()
        }
        _ => "gone".into(),
    }
}

/// 关掉一个任务的标签：从表里摘掉；还在跑就软停（宽限期在后台线程里等）
#[tauri::command]
pub fn task_close(id: u32, state: State<'_, AppState>) {
    if let Some(r) = state.remove_run(id) {
        if r.task.alive() {
            crate::state::stop_in_background(r.task);
        }
    }
}
