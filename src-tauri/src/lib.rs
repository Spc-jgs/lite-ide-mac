mod budget;
mod commands;
pub mod diag;
pub mod menu;
mod state;

/// 把 Tauri 的 async runtime 换成一个小的。
///
/// 默认那个是 `tokio::runtime::Runtime::new()`，worker 数 = 逻辑核数 ——
/// 这台机器上 18 核，于是 `sample` 出来的主进程里有 **18 条常驻空转的
/// `tokio-rt-worker`**，占了 24 条线程里的四分之三。而这个应用同一时刻
/// 最多跑一两个异步命令，剩下十几条是纯开销。
///
/// **会阻塞的活不跑在 worker 上**，走 `spawn_blocking`（见 `commands::blocking`）——
/// 那个池是按需长、空闲了自己收的，所以 worker 给 2 个就够。
/// 反过来把阻塞命令直接扔给 worker 的话，两条并发的 git 就能把 2 个 worker 占满，
/// 之后所有异步命令一起卡住。
///
/// `set()` 只存 Handle，**不接管 runtime 的所有权** —— 它一 drop，
/// 那个 handle 就是废的。所以这里 leak 掉，让它活到进程结束。
fn install_runtime() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("lite-ide-rt")
        .build()
        .expect("建不起 tokio runtime");
    tauri::async_runtime::set(rt.handle().clone());
    Box::leak(Box::new(rt));
}

/// Rust 侧 panic 也要落进同一份日志。
///
/// 默认 hook 只写 stderr —— 而双击启动的 `.app` **没有 stderr**，
/// 那几行字等于没写。**接在默认 hook 后面而不是替换它**：
/// 从终端跑（`cargo test`、开发模式）时那份输出仍然有用。
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        applog::write(applog::Level::Error, "rust", &format!("panic: {info}"));
        prev(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 必须在 Builder 之前 —— Tauri 的 RUNTIME 是个 OnceLock，
    // 谁先碰它谁就把默认那个 18 worker 的装进去了，之后 set() 直接 panic
    install_runtime();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::default())
        /*
         * 菜单项按下去做什么，**一律转给前端**。
         *
         * 唯一的例外是「打开文件夹…」：那个原生面板必须在 Rust 侧开
         * （见 `commands::pick_folder`）。剩下的动作全在前端 ——
         * 它们要读的状态（当前标签、当前仓库、终端列表）都在那边，
         * 搬到 Rust 来就是把一份状态存两处。
         */
        .on_menu_event(|app, event| {
            use tauri::Emitter;
            let id = event.id().0.as_str();
            let _ = app.emit("menu", id);
        })
        .on_window_event(|window, event| {
            // 窗口关了，终端必须跟着走 —— 否则留下孤儿 zsh 常驻
            if matches!(event, tauri::WindowEvent::Destroyed) {
                use tauri::Manager;
                window.state::<state::AppState>().kill_all_ptys();
            }
        })
        .setup(|app| {
            use tauri::Manager;

            /*
             * **日志第一个装。** 它服务的正是「启动时出了事」那一类 ——
             * 装在后面的话，`setup` 里任何一步失败都还是没人记得住。
             *
             * 落在 `~/Library/Logs/com.liteide.app/`（`app_log_dir`），
             * 和别的 macOS 应用放一起：用户自己看得到、删得掉、拖得走。
             * 取不到目录（不该发生）就整个跳过 —— 日志装不上不能变成启动失败。
             */
            if let Ok(dir) = app.path().app_log_dir() {
                let path = applog::install(&dir);
                crate::diag!("applog -> {}", path.display());
                applog::write(
                    applog::Level::Info,
                    "app",
                    &format!("启动 v{}", app.package_info().version),
                );
                install_panic_hook();
            }

            /*
             * 菜单必须在这里建，不能等前端 ready 之后再让它下发 ——
             * 那中间的几百毫秒里菜单栏是 Tauri 的默认英文菜单，
             * 看着像另一个应用。代价是 `menu.rs` 里存着一份 keymap.ts
             * 的拷贝，由 `tests/menu_sync.rs` 卡住。
             */
            let (m, handles) = menu::build(app.handle())?;
            app.set_menu(m)?;
            app.manage(handles);

            if let Some(w) = app.get_webview_window("main") {
                apply_window_material(&w);
                let _ = w.set_focus();
                // 开发期验证用：LITE_IDE_ONTOP=1 让窗口置顶，方便截图取证
                if std::env::var("LITE_IDE_ONTOP").is_ok() {
                    let _ = w.set_always_on_top(true);
                }
                // 同上：LITE_IDE_POS=x,y 把窗口摆到指定位置。
                // 多显示器时窗口会记住上次开在哪，而副屏上的窗口有时截不到图，
                // 有个办法把它拉回主屏能省很多事。
                if let Ok(pos) = std::env::var("LITE_IDE_POS") {
                    if let Some((x, y)) = pos.split_once(',') {
                        if let (Ok(x), Ok(y)) = (x.trim().parse::<i32>(), y.trim().parse::<i32>()) {
                            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                        }
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::probe_path,
            commands::list_dir,
            commands::ignored_dirs,
            commands::read_text,
            commands::detect_encoding,
            commands::list_encodings,
            commands::write_text,
            commands::file_stamp,
            commands::reveal_in_finder,
            commands::create_entry,
            commands::scratch_dir,
            commands::create_scratch,
            commands::discard_empty_scratch,
            commands::rename_entry,
            commands::trash_entry,
            commands::open_log,
            commands::log_stat,
            commands::log_lines,
            commands::log_filter,
            commands::log_filter_stat,
            commands::log_lines_filtered,
            commands::log_filter_map,
            commands::log_refresh,
            commands::close_log,
            commands::initial_path,
            commands::list_project_files,
            commands::grep_project,
            commands::git_root,
            commands::git_status,
            commands::git_diff,
            commands::git_stage,
            commands::git_unstage,
            commands::git_discard,
            commands::git_commit,
            commands::git_log_entries,
            commands::git_commit_files,
            commands::git_commit_diff,
            commands::git_branches,
            commands::git_switch,
            commands::git_worktrees,
            commands::git_worktree_add,
            commands::git_worktree_remove,
            commands::pty_spawn,
            commands::pty_write,
            commands::pty_ack,
            commands::pty_resize,
            commands::pty_kill,
            commands::diag,
            commands::app_log,
            commands::app_log_path,
            commands::clear_app_log,
            commands::diag_enabled,
            commands::report_budget,
            commands::devtools_build,
            commands::pick_folder,
            commands::set_recent,
            commands::sync_menu_state,
            commands::open_external,
            commands::git_fetch,
            commands::git_push,
            commands::git_merge_upstream,
            commands::git_cancel,
            commands::git_outgoing,
            commands::git_console,
            commands::clear_git_console,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}

/// 给窗口挂上 macOS 的材质层。
///
/// 这是整套「透亮」外观**唯一**的来源，webview 里做不出来：
/// `NSVisualEffectView` 用的是 `BehindWindow` 混合模式，模糊的是
/// **窗口后面的桌面**，而桌面在 webview 之外 —— CSS 的 `backdrop-filter`
/// 只能模糊页面自己的内容，换张壁纸它一动不动。
///
/// 这块 view 由 window-vibrancy 插在 webview **下面**（`NSWindowOrderingMode::Below`），
/// 所以前端要透光的地方把背景留空就行，不需要（也没办法）参与合成。
///
/// 两个参数是选过的：
///
/// - `Sidebar`：macOS 给边栏用的那档，暗色下压得住白字，又不像 `HudWindow`
///   那么厚。`UnderWindowBackground` 更淡，代码字压不住浅色壁纸。
/// - `Active` 而不是 `FollowsWindowActiveState`：这个应用的常态就是
///   「在别的窗口里敲命令，拿眼角瞟着这边的日志」。跟随焦点的话，
///   每次切走整扇窗户褪成灰色，那一下比壁纸本身还抢注意力。
///
/// **失败了要让前端知道。** `transparent: true` 的窗口后面什么都没有，
/// 材质没挂上就是一扇能看见桌面的空窗。把 `data-shell` 打回 `web`，
/// CSS 里那套不透明回落就会接上（同一个开关也服务浏览器里的 `pnpm dev`）。
#[cfg(target_os = "macos")]
fn apply_window_material(w: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

    if let Err(e) = apply_vibrancy(
        w,
        NSVisualEffectMaterial::Sidebar,
        Some(NSVisualEffectState::Active),
        None,
    ) {
        eprintln!("窗口材质没挂上，回落到不透明底：{e}");
        let _ = w.eval("document.documentElement.dataset.shell = 'web'");
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_window_material(w: &tauri::WebviewWindow) {
    let _ = w.eval("document.documentElement.dataset.shell = 'web'");
}
