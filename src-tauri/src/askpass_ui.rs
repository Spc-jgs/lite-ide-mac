//! 把 git 的凭据提问（`gitsvc::askpass`，#62）问到发起那次拉取 / 推送 / 克隆的窗口上。
//!
//! 一问一个号（`AppState::askpass_begin`），发 `git-askpass` 给那个窗口、等它用 `git_askpass_answer` 答。
//! **不设超时**（问人的请求，docs/research/codex.md 主题 2）；那次操作结束（成功、失败、取消）时 gitsvc 置位结束标志，
//! 这里不再等、发 `git-askpass-done` 让那个窗口收起对话框。答案只在内存里过一趟，不落盘、不进日志。

use crate::commands::GitAskpassDto;
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// 这次远程操作的问答通道。建不起来（拿不到自己的可执行文件、临时目录写不了）就是 None：照旧不问、拿不到凭据就报认证失败
pub fn for_window(app: &AppHandle, owner: &str) -> Option<gitsvc::askpass::Askpass> {
    let exe = std::env::current_exe().ok()?;
    let (app, owner) = (app.clone(), owner.to_string());
    match gitsvc::askpass::Askpass::start(exe, Box::new(move |prompt, stop| ask(&app, &owner, prompt, stop))) {
        Ok(a) => Some(a),
        Err(e) => {
            applog::write(applog::Level::Warn, "git", &format!("无法准备凭据提问通道，本次不会弹框询问：{e}"));
            None
        }
    }
}

fn ask(app: &AppHandle, owner: &str, prompt: &str, stop: &AtomicBool) -> Option<String> {
    let st = app.state::<AppState>();
    let (id, rx) = st.askpass_begin(owner);
    let prompt = prompt.trim().to_string();
    let secret = is_secret(&prompt);
    let _ = app.emit_to(owner, "git-askpass", GitAskpassDto { id, prompt, secret });
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(v) => return v,
            Err(RecvTimeoutError::Timeout) if !stop.load(Ordering::SeqCst) => continue,
            // 操作结束了、或者窗口关了（发送端被 drop）：不等了，对话框收起来
            Err(_) => break,
        }
    }
    st.askpass_end(id);
    let _ = app.emit_to(owner, "git-askpass-done", id);
    None
}

/// 问的是密码一类（输入框不回显）：git 的提示串是 `Password for '…':`、ssh 的是 `Enter passphrase for key …`
fn is_secret(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    ["password", "passphrase", "token", "密码", "口令"].iter().any(|k| p.contains(k))
}

#[cfg(test)]
mod tests {
    #[test]
    fn 密码一类的提问不回显() {
        assert!(super::is_secret("Password for 'https://alice@git.example.com':"));
        assert!(super::is_secret("Enter passphrase for key '/Users/a/.ssh/id_ed25519':"));
        assert!(!super::is_secret("Username for 'https://git.example.com':"));
    }
}
