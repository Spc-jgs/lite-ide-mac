mod budget;
mod commands;
pub mod diag;
pub mod menu;
mod open;
mod state;
mod trust_store;
mod windows;
mod winctl;

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
        // 启动分段：WebView 把页面加载完的时刻（入口脚本还没开始跑）
        .on_page_load(|_webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                budget::mark("page");
            }
        })
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
            use tauri::{Emitter, Manager};
            let id = event.id().0.as_str();
            // 「退出」是第二个在 Rust 侧处理的：要先让每个窗口存好现场再退（winctl::quit）
            if id == "quit" {
                winctl::quit(app);
                return;
            }
            // 只发给前台窗口（多窗口第 2 步）。原来是广播：两个窗口时在 A 里按 ⌘S，B 也保存。
            // 一个窗口都没有时（#41：关掉最后一个窗口应用还在）由 Rust 自己接能接的那几项
            match app.state::<state::AppState>().windows.front() {
                Some(label) => {
                    // 发给了谁：多窗口验收要靠这一行看「在 A 里按 ⌘S，只有 A 收到」
                    crate::diag!("menu {id} → {label}");
                    let _ = app.emit_to(label.as_str(), "menu", id);
                }
                None => winctl::menu_without_window(app, id),
            }
        })
        .on_window_event(|window, event| {
            // 窗口关了，它名下的终端、日志句柄、监听、远程操作跟着走 —— 否则留下孤儿 zsh 常驻。
            // 只收**这个窗口**的：别的窗口的终端里可能正跑着 gradle（docs/MULTIWINDOW.md 3.2）
            use tauri::Manager;
            let st = window.state::<state::AppState>();
            match event {
                tauri::WindowEvent::Destroyed => {
                    st.release_window(window.label());
                    st.windows.remove(window.label());
                    winctl::save_soon();
                    // 最后一个窗口没了：前端都没了，没人再同步菜单状态 —— 要靠窗口的项全灰掉
                    // （#41 那条）。「打开文件夹」「新建草稿」不在这些组里，照样能点
                    if st.windows.len() == 0 {
                        menu::apply(window.app_handle(), windows::MenuState::default());
                    }
                }
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                    winctl::track(window.app_handle(), window);
                }
                // 到前台了：菜单事件改发给它，原生菜单的可用状态换成它存着的那份
                // （前端自己不知道什么时候「到前台」了，等它推会慢一拍或者根本不来）
                tauri::WindowEvent::Focused(true) => {
                    if let Some(m) = st.windows.focus(window.label()) {
                        menu::apply(window.app_handle(), m);
                    }
                    // 前台换了：落盘的窗口顺序跟着变（下次启动最后建的那个在最前面）
                    winctl::save_soon();
                }
                _ => {}
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

            /*
             * 登记第一个窗口，再把命令行参数送进它的收件箱。
             *
             * `argv` 原来是 `initial_paths` 每次被调时现读的 —— 多窗口下每个新窗口都会调它，
             * 于是每个窗口都会把启动参数再开一遍（第 0 步的原型里 w-1 就多开了 App.svelte）。
             * 启动参数只属于启动时那一个窗口，在这儿送一次。
             */
            let st = app.state::<state::AppState>();
            st.windows.register("main");
            winctl::start_saver(app.handle());
            // 上次退出时开着的窗口开回来（多窗口第 3 步）。要在送 argv 之前：
            // 恢复出来的窗口先报上项目根，argv 里的目录才路由得对（已经开着就不再开一个）
            winctl::restore_at_launch(app.handle());
            // 「最近打开」菜单：名单在 windows.json 里，现在就建（原来要等前端起来推过来）
            let _ = menu::refresh_recent(app.handle(), &st.windows.recent());
            let mut args: Vec<String> = std::env::args()
                .skip(1)
                .filter(|a| !a.starts_with('-') && std::path::Path::new(a).exists())
                .collect();
            // 冷启动时比 setup 早到的系统事件（Finder 双击启动）攒成了孤儿：
            // 现在窗口登记完、恢复完了，各自的项目根都知道，和 argv 一起重新路由
            args.extend(st.windows.take_orphans());
            open::deliver(app.handle(), args);

            if let Some(w) = app.get_webview_window("main") {
                budget::mark("window");
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
            commands::read_clipboard,
            commands::create_entry,
            commands::scratch_dir,
            commands::create_scratch,
            commands::list_scratches,
            commands::install_cli,
            commands::discard_empty_scratch,
            commands::move_entry,
            commands::rename_entry,
            commands::trash_entry,
            commands::open_log,
            commands::log_stat,
            commands::log_lines,
            commands::log_filter,
            commands::log_filter_stat,
            commands::log_seek_time,
            commands::log_lines_filtered,
            commands::log_filter_map,
            commands::log_refresh,
            commands::close_log,
            commands::initial_paths,
            commands::set_window_root,
            commands::quit_ready,
            commands::request_quit,
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
            commands::git_head_text,
            commands::git_blame,
            commands::git_apply_cached,
            commands::git_apply_worktree,
            commands::git_commit_vs_worktree,
            commands::git_cherry_pick,
            commands::git_stash_list,
            commands::git_stash_push,
            commands::git_stash_pop,
            commands::git_commit_diff,
            commands::git_branches,
            commands::git_switch,
            commands::git_worktrees,
            commands::git_worktree_add,
            commands::git_worktree_remove,
            commands::grep_scratches,
            commands::git_branch_delete,
            commands::git_branch_rename,
            commands::git_trust_scan,
            commands::git_trust_grant,
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
            commands::boot_mark,
            commands::devtools_build,
            commands::watch_root,
            commands::pick_folder,
            commands::pick_save_path,
            commands::recent_projects,
            commands::forget_recent,
            commands::clear_recent,
            commands::adopt_recent,
            commands::claim_empty_session,
            commands::open_window,
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
        .build(tauri::generate_context!())
        .expect("Tauri 启动失败")
        /*
         * `run` 而不是 `.run(ctx)`：要接 `RunEvent::Opened`（issue #40）。
         * macOS 把 Finder 双击 / 拖 Dock / `open -a` 都送成这个事件，
         * 冷启动和已在运行都走它 —— 读 `argv` 一条都接不到。按窗口路由，
         * 那个窗口的前端没就绪时先攒在它的收件箱里；细节见 `open.rs` / `windows.rs`。
         */
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => open::deliver(app, open::paths_from_urls(&urls)),
            /*
             * 关掉最后一个窗口不退出（#41）。第 0 步实测：`code == None` 只来自「最后一个窗口
             * 被销毁」；⌘Q 走我们自己的「退出」→ `app.exit(0)`，带着 `Some(0)`，放行。
             */
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                if windows::should_prevent_exit(code) {
                    api.prevent_exit();
                }
            }
            // 点 Dock 图标，而且一个看得见的窗口都没有：开回最近关掉的那个
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { has_visible_windows: false, .. } => winctl::reopen(app),
            // 不管从哪条路退出（Dock 右键退出、注销也走这里），最后补存一次窗口位置
            tauri::RunEvent::Exit => winctl::save_now(app),
            _ => {}
        });
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
