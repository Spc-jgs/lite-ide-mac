//! AI 助手（docs/TAKEOVER.md MVP-6）。业务在 `crate::agent`，ACP 的线和进程在 `acpsvc`。
//! 全部 async + 阻塞池：起 agent 第一次要下适配器（几十秒），`agent_prompt` 一直等到这一轮说完（几分钟）

use super::*;
use tauri::Manager;

/// 测试构建里可以换成按剧本回话的假 agent（`LITE_AGENT_CMD`，交给 `/bin/sh -c`；scripts/lib/bridge.sh 默认给 scripts/fake-acp-agent.py）：
/// smoke 不能真去调模型、花钱、等网。设成空串就用真的。正式包里没有这条路
fn program() -> Option<(std::path::PathBuf, Vec<String>)> {
    #[cfg(feature = "test-bridge")]
    if let Some(cmd) = std::env::var("LITE_AGENT_CMD").ok().filter(|c| !c.is_empty()) {
        return Some(("/bin/sh".into(), vec!["-c".into(), cmd]));
    }
    None
}

/// 起一个 agent（`kind`：claude / codex）、在 `root` 里建一个会话
#[tauri::command]
pub async fn agent_start(app: tauri::AppHandle, window: tauri::Window, root: String, kind: String) -> Result<AgentStartedDto, String> {
    let kind = crate::agent::Kind::parse(&kind).ok_or_else(|| format!("不认识的 agent：{kind}"))?;
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        let shell = crate::settingsctl::terminal_shell(&st);
        let s = st.agents.start(&app, &owner, &root, kind, &shell, program())?;
        Ok(AgentStartedDto { id: s.id, session_id: s.session_id, name: s.name, version: s.version, modes: s.modes })
    })
    .await
}

/// 发一轮话，等到这一轮说完，回 `stopReason`（end_turn / cancelled / max_tokens / refusal…）。过程中的字、工具调用走 `agent-event`
#[tauri::command]
pub async fn agent_prompt(app: tauri::AppHandle, id: u32, prompt: serde_json::Value) -> Result<String, String> {
    blocking(move || app.state::<AppState>().agents.prompt(id, prompt)).await
}

#[tauri::command]
pub async fn agent_cancel(app: tauri::AppHandle, id: u32) -> Result<(), String> {
    blocking(move || app.state::<AppState>().agents.cancel(id)).await
}

#[tauri::command]
pub async fn agent_set_mode(app: tauri::AppHandle, id: u32, mode: String) -> Result<(), String> {
    blocking(move || app.state::<AppState>().agents.set_mode(id, &mode)).await
}

/// 回 agent 问的那个请求（权限、读文件、写文件）。`error` 非空就回错误
#[tauri::command]
pub async fn agent_reply(app: tauri::AppHandle, id: u32, req: serde_json::Value, result: serde_json::Value, error: Option<String>) -> Result<(), String> {
    blocking(move || app.state::<AppState>().agents.reply(id, req, result, error)).await
}

#[tauri::command]
pub async fn agent_stop(app: tauri::AppHandle, id: u32) -> Result<(), String> {
    blocking(move || {
        app.state::<AppState>().agents.stop(id);
        Ok(())
    })
    .await
}
