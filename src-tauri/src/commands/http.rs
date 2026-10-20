//! 发 `.http` 里的请求（docs/IDEA.md 第 19 节）。业务在 `crates/httpsvc`（交给系统的 curl）；解析 `.http`、替换变量在前端。
//! 全部 async + 阻塞池：等的是网络（停在断点上的服务能等几分钟）。

use super::*;
use tauri::Manager;

/// curl 用绝对路径，不靠 PATH：从终端启动时 PATH 是用户的，不该让它决定我们调到哪个 curl（同 `open`）
const CURL: &str = "/usr/bin/curl";
/// 响应体留最近这么多个（应用数据目录 `http/` 里）
const KEEP: usize = 50;

/// 这个 `.http` 文件能用的环境：从它所在的目录往上到项目根找 `http-client.env.json` / `http-client.private.env.json`
#[tauri::command]
pub async fn http_envs(root: String, path: String) -> Result<HttpEnvsDto, String> {
    blocking(move || {
        let dir = Path::new(&path).parent().unwrap_or(Path::new(&root));
        let e = httpsvc::env::load(Path::new(&root), dir);
        Ok(HttpEnvsDto {
            names: e.envs.keys().cloned().collect(),
            json: serde_json::Value::Object(e.envs).to_string(),
            files: e.files.iter().map(|p| p.display().to_string()).collect(),
            problems: e.problems,
        })
    })
    .await
}

/// 发一个（变量已经替换好的）请求。`op` 是前端给的操作号：发之前就定下来，取消按钮才点得动（rust.md「操作 id 由谁发」）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn http_send(
    window: tauri::Window,
    app: tauri::AppHandle,
    op: u32,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    body_file: Option<String>,
    follow: bool,
) -> Result<HttpResponseDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        let dir = app.path().app_data_dir().map_err(|e| format!("找不到应用数据目录：{e}"))?.join("http");
        let h = st.begin_http(&owner, op).ok_or("这个请求已经在发了")?;
        let req = httpsvc::Request { method, url, headers, body: body.map(String::into_bytes), body_file: body_file.map(Into::into), follow };
        let r = httpsvc::send(Path::new(CURL), &req, &dir, httpsvc::next_seq(), &h);
        st.end_http(&owner, op);
        httpsvc::prune(&dir, KEEP);
        r.map(Into::into).map_err(|e| e.to_string())
    })
    .await
}

/// 取消正在发的那个（找不到 = 已经回来了，不算错）
#[tauri::command]
pub fn http_cancel(window: tauri::Window, state: State<'_, AppState>, op: u32) {
    state.cancel_http(window.label(), op);
}
