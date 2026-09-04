mod commands;
pub mod diag;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(state::AppState::default())
        .on_window_event(|window, event| {
            // 窗口关了，终端必须跟着走 —— 否则留下孤儿 zsh 常驻
            if matches!(event, tauri::WindowEvent::Destroyed) {
                use tauri::Manager;
                window.state::<state::AppState>().kill_all_ptys();
            }
        })
        .setup(|app| {
            use tauri::Manager;
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
            commands::read_text,
            commands::detect_encoding,
            commands::list_encodings,
            commands::write_text,
            commands::file_stamp,
            commands::reveal_in_finder,
            commands::create_entry,
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
            commands::pty_resize,
            commands::pty_kill,
            commands::diag,
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
