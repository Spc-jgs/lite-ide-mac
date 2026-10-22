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
                        crate::taskdefs::Source::User { .. } => "user",
                    }
                    .into(),
                    kind: if let crate::taskdefs::Source::User { kind, .. } = &d.source { kind.clone() } else { String::new() },
                    items: if let crate::taskdefs::Source::User { items, .. } = d.source { items } else { Vec::new() },
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
pub async fn task_run(window: tauri::Window, app: tauri::AppHandle, root: String, name: String, debug: Option<bool>) -> Result<TaskRunDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        let mut def = crate::taskdefs::resolve(Path::new(&root), &name).ok_or_else(|| format!("没有叫「{name}」的任务了（tasks.json 改过？）"))?;
        // 组合配置没有自己的命令：前端先问 `run_compound_items`、一份份起。走到这儿是前端漏了那一步
        if matches!(&def.source, crate::taskdefs::Source::User { kind, .. } if kind == "compound") {
            return Err(format!("「{name}」是组合配置，要一份份起（这是个 bug）"));
        }
        // 调试（docs/IDEA.md 第 18 节）：只有命令是我们拼的那两种 —— 认出来的 Spring Boot 应用（知道 `java` 在哪、主类是谁）、
        // 单测（`mvn -Dtest=… test`，JDWP 交给 surefire fork 的 JVM）
        // 热替换（docs/IDEA.md 第 26 节）：JVM 多带一个代理，和它说话的控制目录现在建好。开发构建没带 agent 的 jar 就不带（调试照常）
        let mut hot_dir = None;
        // 远程 JVM 调试（docs/TAKEOVER.md G23）：只能调试 —— ▶ 和虫子一样。命令是占位的（`taskdefs::remote_command`），不改；
        // 热替换的代理塞不进别人起的 JVM，没有
        let remote = crate::taskdefs::remote_target(Path::new(&root), &name);
        let debug = match (remote, debug.unwrap_or(false)) {
            (Some((host, port)), _) => Some(crate::debug::Who::Remote(host, port)),
            (None, false) => None,
            (None, true) => {
                let agent = crate::java::hotswap_jar(&app).and_then(|jar| {
                    let dir = hotswap::control_dir("dbg").ok()?;
                    let arg = hotswap::agent_arg(&jar, &dir);
                    hot_dir = Some(dir);
                    Some(arg)
                });
                Some(if let Some(main) = crate::taskdefs::java_main(Path::new(&root), &name) {
                    def.command = crate::debug::with_jdwp(&def.command, agent.as_deref()).ok_or_else(|| format!("「{name}」的命令不是 java 起的，调试不了"))?;
                    crate::debug::Who::Main(main)
                } else if let Some(project) = crate::taskdefs::test_project(Path::new(&root), &name).filter(|_| matches!(def.source, crate::taskdefs::Source::Goal { .. })) {
                    def.command = crate::debug::with_surefire_debug(&def.command, agent.as_deref()).ok_or_else(|| format!("「{name}」的命令不是 mvn 起的，调试不了"))?;
                    crate::debug::Who::Project(project)
                } else {
                    return Err(format!("「{name}」不是 Java 应用（认出来的 Spring Boot、你建的应用配置）也不是单测，调试不了"));
                })
            }
        };
        // surefire 那条路径带空格时没把代理塞进去：控制目录也就用不上，这次没有热替换
        if hot_dir.is_some() && !def.command.contains("-javaagent:") {
            if let Some(d) = hot_dir.take() {
                hotswap::remove_control_dir(&d);
            }
        }
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
        let id = st.insert_run(&owner, crate::state::Run { root: root.clone(), name: name.clone(), task: task.clone() });
        let log = spec.log.clone();
        let is_debug = debug.is_some();
        if let Some(who) = debug {
            crate::debug::start(&app, &owner, id, &root, log.clone(), who, task.clone(), hot_dir);
        }
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
        Ok(TaskRunDto { id, name, command: def.command, log: spec.log.to_string_lossy().into_owned(), service, debug: is_debug })
    })
    .await
}

/// 组合配置（compound）的那几份；不是组合配置回 null（docs/TAKEOVER.md R1）。⌃R / ▶ 之前问一句：只读应用自己的那份文件，不扫项目
#[tauri::command]
pub async fn run_compound_items(root: String, name: String) -> Result<Option<Vec<String>>, String> {
    blocking(move || Ok(crate::taskdefs::compound_items(Path::new(&root), &name))).await
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

/// 停不掉的那些（#68）：宽限期过了还在「正在停」时界面来问 —— 组里活着、但不归我们管的进程（免密 sudo 的 root）。
/// 我们发不了信号（EPERM），只能说清是谁、谁的，`sudo kill` 由人自己抄去跑（ui.md 第七条：别人的东西，人点了 / 人抄了才动）。
/// 枚举组员、查每个进程碰的是内核，量不大，但照规矩不在主线程上
#[tauri::command]
pub async fn task_stuck(id: u32, state: State<'_, AppState>) -> Result<Vec<StuckProcDto>, String> {
    let Some(task) = state.run_task(id) else { return Ok(Vec::new()) };
    blocking(move || Ok(task.foreign().into_iter().map(|f| StuckProcDto { pid: f.pid, user: f.user, command: f.command }).collect())).await
}

/// 构建那次（⌘F9）的编译错误：从这次运行的输出里认（`javac.rs`）。`log` 是 `task_run` 回的那个输出文件
#[tauri::command]
pub async fn build_problems(log: String) -> Result<BuildProblemsDto, String> {
    blocking(move || {
        let (items, total) = crate::javac::parse_file(Path::new(&log)).map_err(|e| format!("读不了这次构建的输出：{e}"))?;
        Ok(BuildProblemsDto {
            items: items.into_iter().map(|p| BuildProblemDto { path: p.path, line: p.line, col: p.col, error: p.error, message: p.message }).collect(),
            total: total as u32,
        })
    })
    .await
}

/// 跑单测那次（「模块 [test 选择器]」）的结果：读那个模块 `target/surefire-reports` 里 `since_ms`（这次运行开始的时刻，毫秒）之后写的报告。
/// 不是测试任务、模块找不到给空的
#[tauri::command]
pub async fn test_results(root: String, name: String, since_ms: u64) -> Result<TestResultsDto, String> {
    blocking(move || {
        let Some(dir) = crate::taskdefs::test_module_dir(Path::new(&root), &name) else { return Ok(TestResultsDto { cases: Vec::new(), total: 0 }) };
        let since = std::time::UNIX_EPOCH + std::time::Duration::from_millis(since_ms);
        let (cases, total) = crate::junit::results(&dir, since);
        let cases = cases
            .into_iter()
            .map(|c| TestCaseDto {
                class: c.class,
                name: c.name,
                ms: c.ms,
                outcome: match c.outcome {
                    crate::junit::Outcome::Passed => "passed",
                    crate::junit::Outcome::Failed => "failed",
                    crate::junit::Outcome::Error => "error",
                    crate::junit::Outcome::Skipped => "skipped",
                }
                .into(),
                message: c.message,
                detail: c.detail,
            })
            .collect();
        Ok(TestResultsDto { cases, total: total as u32 })
    })
    .await
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

/// 这几次运行的 JVM 堆用了多少（docs/IDEA.md 第 27 节，Services 工具栏上选中服务的数和小曲线）。读 JVM 自己写的性能计数，不起子进程。
/// 进程组长就是那个 JVM（Spring 的命令是 `… && exec java …`）；还在 `mvn compile` 的、不是 JVM 的、读不到的不回
#[tauri::command]
pub async fn task_heap(ids: Vec<u32>, app: tauri::AppHandle) -> Result<Vec<TaskHeapDto>, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let tmp = std::env::temp_dir();
        Ok(ids
            .into_iter()
            .filter_map(|id| {
                let task = st.run_task(id).filter(|t| t.alive())?;
                let h = jvmstat::read(&tmp, task.pgid() as u32).ok()?;
                Some(TaskHeapDto { id, used: h.used, committed: h.committed, max: h.max, gc_count: h.gc_count, gc_ms: h.gc_ms })
            })
            .collect())
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
        let repo = crate::maven::local_repo(&user);
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
