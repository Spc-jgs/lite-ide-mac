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
                        crate::taskdefs::Source::Spring { .. } => "spring",
                        crate::taskdefs::Source::Goal { .. } => "goal",
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

/// 跑一个任务（按名字现找定义：tasks.json 可能刚改过）。同一个窗口里同名的还在跑 → 先停它、等它真没了再起
/// （`AppState::begin_run`）。返回这次运行的 id 和日志文件
#[tauri::command]
pub async fn task_run(window: tauri::Window, app: tauri::AppHandle, root: String, name: String) -> Result<TaskRunDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        let def = crate::taskdefs::resolve(Path::new(&root), &name).ok_or_else(|| format!("没有叫「{name}」的任务了（tasks.json 改过？）"))?;
        let base = app.path().app_data_dir().map_err(|e| format!("找不到应用数据目录：{e}"))?.join("runs");
        // 起完、登记进运行表之前一直攥着：同一个任务的第二次 ⌃R 在这儿被挡住
        let _claim = st.begin_run(&owner, &root, &name)?;
        let spec = tasksvc::Spec {
            shell: crate::settingsctl::terminal_shell(&st),
            command: def.command.clone(),
            cwd: Path::new(&root).join(&def.cwd),
            env: def.env,
            log: tasksvc::log_file(&base, &root, &name),
            cap: tasksvc::LOG_CAP,
        };
        let task = std::sync::Arc::new(tasksvc::Task::start(&spec).map_err(|e| format!("「{name}」起不来：{e}"))?);
        crate::diag!("task_run {name} pgid={} log={}", task.pgid(), spec.log.display());
        st.live_add(tasksvc::live::Live {
            pgid: task.pgid(),
            started_us: task.started_us().unwrap_or(0),
            name: name.clone(),
            command: def.command.clone(),
            root: root.clone(),
        });
        let id = st.insert_run(&owner, crate::state::Run { root, name: name.clone(), task: task.clone() });
        let log = spec.log.clone();
        // 等它退出，退了告诉起它的那个窗口。一次运行一条线程，跟着任务的寿命走
        let app2 = app.clone();
        let _ = std::thread::Builder::new().name(format!("task-watch-{id}")).spawn(move || {
            let end = task.wait();
            let tasksvc::State::Exited { code, signal, stopped } = end else { return };
            let failed = end.failed();
            let st = app2.state::<AppState>();
            st.live_remove(task.pgid());
            // 自己失败退出的：看看是不是端口被占（只读输出的最后 64KB —— 报错就在结尾）
            let port = if failed {
                tasksvc::port::from_log(&log)
                    .map(|(port, h)| PortHolderDto { port, pid: h.pid, ours: st.task_named_by_group(h.pgid), command: h.command })
            } else {
                None
            };
            let _ = app2.emit_to(owner.as_str(), "task-exit", TaskExitDto { id, code, signal, stopped, failed, port });
        });
        let service = matches!(def.source, crate::taskdefs::Source::Spring { .. });
        Ok(TaskRunDto { id, name, command: def.command, log: spec.log.to_string_lossy().into_owned(), service })
    })
    .await
}

/// 停。第一次软停（SIGINT 整组，宽限期后 SIGKILL）；软停还没完再叫一次 → 立刻强杀（同 IDEA 按钮变「强制结束」）。
/// 组长退了、`&` 出去的后台进程还在的，状态照样是 `Running`（`tasksvc::State`），走同一条路
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
        _ => "gone".into(),
    }
}

/// 一次运行此刻在听的 TCP 端口（Services 里服务名后面那个 `:8080`）。Spring Boot 起来要几秒，前端在「跑着、还没端口」时隔一会儿问一次 ——
/// 不走 `services_list`：那条要重新翻一遍所有模块的 Java 源码认入口，这条只是一次 `lsof`
#[tauri::command]
pub async fn task_ports(id: u32, app: tauri::AppHandle) -> Result<Vec<u16>, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        Ok(match st.run_task(id) {
            Some(task) if task.alive() => tasksvc::port::listening(task.pgid()),
            _ => Vec::new(),
        })
    })
    .await
}

/// 关掉一个任务的标签：从表里摘掉；还在跑就软停（宽限期在后台线程里等）
#[tauri::command]
pub fn task_close(id: u32, state: State<'_, AppState>) {
    if let Some(r) = state.remove_run(id) {
        if r.task.alive() {
            state.retire(r.task);
        }
    }
}

/// 端口卡片上点「结束它」：结束现在占着 `port` 的那个（Rust 这边重新查一次是谁，不信前端带回来的 pid —— 中间可能已经换了人）
#[tauri::command]
pub async fn task_free_port(port: u16, app: tauri::AppHandle) -> Result<(), String> {
    blocking(move || {
        let st = app.state::<AppState>();
        tasksvc::port::free_port(port, tasksvc::GRACE, |pgid| st.task_by_group(pgid))
    })
    .await
}

/// 这个项目上次没停干净的任务（启动后、项目根定了再问）
#[tauri::command]
pub fn task_stale(root: String, state: State<'_, AppState>) -> Vec<TaskStaleDto> {
    state.stale_for(&root).into_iter().map(|l| TaskStaleDto { pgid: l.pgid, name: l.name, command: l.command }).collect()
}

/// 卡片上的两个按钮：`kill` = 结束它们（整组先软后硬，等它们退干净），否则从账上划掉、不管它们
#[tauri::command]
pub async fn task_stale_resolve(root: String, kill: bool, app: tauri::AppHandle) -> Result<(), String> {
    blocking(move || {
        let mine = app.state::<AppState>().stale_take(&root);
        if kill {
            let pgids: Vec<i32> = mine.iter().filter(|l| tasksvc::live::still_running(l)).map(|l| l.pgid).collect();
            tasksvc::stop_groups(&pgids, tasksvc::GRACE);
        }
        Ok(())
    })
    .await
}


/// Maven 工具窗：模块树、每个模块里认出来的 Spring Boot 应用（带任务名 —— 点运行和 ⌃⌥R 里选它是同一个任务）。读盘，走阻塞池
#[tauri::command]
pub async fn maven_project(root: String) -> Result<MavenProjectDto, String> {
    blocking(move || {
        let rootp = Path::new(&root);
        let prefs = crate::maven::prefs(rootp);
        let mut profiles: Vec<MavenProfileDto> = Vec::new();
        let reactors = crate::maven::reactors(rootp)
            .into_iter()
            .map(|rel| {
                let r = rootp.join(&rel);
                let mods = crate::maven::modules(&r);
                for m in &mods {
                    let text = std::fs::read_to_string(r.join(&m.rel).join("pom.xml")).unwrap_or_default();
                    for (id, on_default) in crate::maven::pom_profiles(&text) {
                        match profiles.iter_mut().find(|p| p.id == id) {
                            Some(p) => p.active_by_default |= on_default,
                            None => profiles.push(MavenProfileDto { on: prefs.profiles.contains(&id), id, active_by_default: on_default }),
                        }
                    }
                }
                let modules = mods.into_iter().map(|m| MavenModuleDto { rel: m.rel, artifact_id: m.artifact_id, parent: m.parent }).collect();
                MavenReactorDto { rel, modules }
            })
            .collect();
        Ok(MavenProjectDto {
            reactors,
            phases: crate::maven::PHASES.iter().map(|g| g.to_string()).collect(),
            profiles,
            offline: prefs.offline,
            skip_tests: prefs.skip_tests,
        })
    })
    .await
}

/// Services 工具窗（照 IDEA）：认出来的 Spring Boot 服务，在跑的带上它在听的端口（`lsof -g 进程组`）。
/// 跑没跑、退出码这些前端的运行记录里有；端口只有这里查得到 —— Spring 起来要几秒，前端在「在跑但还没端口」时隔几秒再问一次
#[tauri::command]
pub async fn services_list(window: tauri::Window, app: tauri::AppHandle, root: String) -> Result<Vec<ServiceDto>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        let (tasks, _) = crate::taskdefs::spring_tasks(Path::new(&root));
        Ok(tasks
            .into_iter()
            .map(|t| {
                let ports = match st.find_run(&owner, &root, &t.def.name) {
                    Some((_, task)) if task.alive() => tasksvc::port::listening(task.pgid()),
                    _ => Vec::new(),
                };
                let module = if let crate::taskdefs::Source::Spring { module } = &t.def.source { module.clone() } else { String::new() };
                ServiceDto { task: t.def.name, main_class: t.app.main_class, module, ports }
            })
            .collect())
    })
    .await
}

/// 插件和依赖：每个 reactor 跑一次 mvn（`maven_run.rs`）。**会等到跑完**（几秒到几十秒，要上网解析依赖时更久），前端先画别的、这两个节点转圈
#[tauri::command]
pub async fn maven_resolve(app: tauri::AppHandle, root: String) -> Result<MavenResolvedDto, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let rootp = Path::new(&root);
        let prefs = crate::maven::prefs(rootp);
        let shell = crate::settingsctl::terminal_shell(&st);
        let user = std::env::var_os("HOME").map(std::path::PathBuf::from).ok_or("找不到用户目录（没有 HOME）")?;
        let settings = std::fs::read_to_string(user.join(".m2/settings.xml")).ok();
        let repo = lspsvc::jdtls::local_repo(&user, settings.as_deref());
        let data = app.path().app_data_dir().map_err(|e| format!("找不到应用数据目录：{e}"))?.join("maven");
        let reactors = crate::maven::reactors(rootp)
            .into_iter()
            .map(|rel| {
                let r = rootp.join(&rel);
                let scratch = lspsvc::jdtls::workspace_dir(&data, &r).with_extension("effective-pom.xml");
                match crate::maven_run::resolve(&r, &prefs, &shell, &repo, &scratch) {
                    Ok(mods) => MavenResolvedReactorDto {
                        rel,
                        error: String::new(),
                        modules: mods
                            .into_iter()
                            .map(|m| MavenResolvedModuleDto {
                                artifact_id: m.artifact_id,
                                plugins: m
                                    .plugins
                                    .into_iter()
                                    .map(|p| MavenPluginDto { group_id: p.group_id, artifact_id: p.artifact_id, version: p.version, prefix: p.prefix, goals: p.goals })
                                    .collect(),
                                deps: m
                                    .deps
                                    .into_iter()
                                    .map(|d| MavenDepDto { depth: d.depth, group_id: d.group_id, artifact_id: d.artifact_id, version: d.version, scope: d.scope, optional: d.optional })
                                    .collect(),
                            })
                            .collect(),
                    },
                    Err(error) => MavenResolvedReactorDto { rel, modules: Vec::new(), error },
                }
            })
            .collect();
        Ok(MavenResolvedDto { reactors })
    })
    .await
}

/// 工具栏的开关、勾的 profile（按项目记，跑目标和起服务都带上）
#[tauri::command]
pub async fn maven_prefs_set(root: String, offline: bool, skip_tests: bool, profiles: Vec<String>) -> Result<(), String> {
    blocking(move || {
        let p = crate::maven::Prefs { offline, skip_tests, profiles };
        crate::maven::set_prefs(Path::new(&root), &p).map_err(|e| format!("记不下 Maven 的开关：{e}"))
    })
    .await
}
