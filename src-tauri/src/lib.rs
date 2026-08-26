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
                let _ = w.set_focus();
                // 开发期验证用：LITE_IDE_ONTOP=1 让窗口置顶，方便截图取证
                if std::env::var("LITE_IDE_ONTOP").is_ok() {
                    let _ = w.set_always_on_top(true);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::probe_path,
            commands::list_dir,
            commands::read_text,
            commands::write_text,
            commands::file_stamp,
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
            commands::ripgrep_available,
            commands::pty_spawn,
            commands::pty_write,
            commands::pty_resize,
            commands::pty_kill,
            commands::pty_alive,
            commands::diag,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}
