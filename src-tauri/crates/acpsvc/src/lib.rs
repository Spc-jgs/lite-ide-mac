//! Agent Client Protocol（ACP）的宿主一侧（docs/TAKEOVER.md MVP-6，设计在 docs/DIRECTION.md「agent 会话子系统」）。
//!
//! lite-ide 不自己做 agent（2026-09-24 定的）：接 Claude Code、Codex 的 ACP 适配器（`@agentclientprotocol/claude-agent-acp` /
//! `codex-acp`，经 npx 起），当它们的「编辑器」—— 给它们读文件、写文件（先进编辑器、审阅了才存）、问人要不要允许。
//!
//! - [`conn`]：一行一条 JSON-RPC；agent 反过来问宿主的请求延后回答
//! - [`process`]：起、看着、整组停；找 npx
//!
//! 和 Tauri 无关：会话和前端之间的那些事件在 `src-tauri/src/agent.rs`。

pub mod conn;
pub mod process;

/// 钉死的适配器版本（2026-10-26 挑的：发布满两周、实测过初始化 / 建会话 / 一轮对话 / 读到 stdin 结束就退）。
/// 第一次用时 npx 从 npm 下（Claude 的约 2 MB，下载约 20 秒），之后用缓存
pub const CLAUDE_ADAPTER: &str = "@agentclientprotocol/claude-agent-acp@0.89.1";
pub const CODEX_ADAPTER: &str = "@agentclientprotocol/codex-acp@2.2.2";

/// 宿主声明的能力：读写文件走我们（读得到编辑器里没保存的、写进编辑器先不存盘）；不替 agent 开终端（它用自己的）
pub fn client_capabilities() -> serde_json::Value {
    serde_json::json!({ "fs": { "readTextFile": true, "writeTextFile": true }, "terminal": false })
}
