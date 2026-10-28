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
    root: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    body_file: Option<String>,
    follow: bool,
    env: Option<String>,
    cookies: bool,
    parts: Option<Vec<HttpPartDto>>,
) -> Result<HttpResponseDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        let dir = http_dir(&app)?;
        let h = st.begin_http(&owner, op).ok_or("该请求正在发送")?;
        // Cookie 罐按「项目 + 环境」一个（同 IDEA）；`# @no-cookie-jar` 的请求不带也不记
        let cookie_jar = cookies.then(|| httpsvc::jar_path(&dir, &root, env.as_deref()));
        let parts = parts.map(|ps| {
            ps.into_iter()
                .map(|p| match p.file {
                    Some(f) => httpsvc::Part::File(f.into()),
                    None => httpsvc::Part::Text(p.text.unwrap_or_default().into_bytes()),
                })
                .collect()
        });
        let req = httpsvc::Request { method, url, headers, body: body.map(String::into_bytes), body_file: body_file.map(Into::into), parts, root: root.into(), follow, cookie_jar };
        let r = httpsvc::send(Path::new(CURL), &req, &dir, httpsvc::next_seq(), &h);
        st.end_http(&owner, op);
        httpsvc::prune(&dir, KEEP);
        r.map(Into::into).map_err(|e| e.to_string())
    })
    .await
}

fn http_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| format!("找不到应用数据目录：{e}"))?.join("http"))
}

/// 这个项目、这个环境记着的 Cookie（只给域名、名字、过期时间，不给值：值是登录态）
#[tauri::command]
pub async fn http_cookies(app: tauri::AppHandle, root: String, env: Option<String>) -> Result<Vec<HttpCookieDto>, String> {
    blocking(move || {
        let jar = httpsvc::jar_path(&http_dir(&app)?, &root, env.as_deref());
        Ok(httpsvc::cookies(&jar).into_iter().map(|c| HttpCookieDto { domain: c.domain, path: c.path, name: c.name, expires: c.expires, http_only: c.http_only }).collect())
    })
    .await
}

/// 清掉这个项目、这个环境的 Cookie
#[tauri::command]
pub async fn http_cookies_clear(app: tauri::AppHandle, root: String, env: Option<String>) -> Result<(), String> {
    blocking(move || httpsvc::clear_cookies(&httpsvc::jar_path(&http_dir(&app)?, &root, env.as_deref())).map_err(|e| e.to_string())).await
}

/// 取消正在发的那个（找不到 = 已经回来了，不算错）
#[tauri::command]
pub fn http_cancel(window: tauri::Window, state: State<'_, AppState>, op: u32) {
    state.cancel_http(window.label(), op);
}

/// 记一条请求历史（#77）。URL 里像密钥的查询参数由 httpsvc 打码，头和体不记（理由在 `httpsvc::history` 头上）
#[tauri::command]
pub async fn http_history_add(app: tauri::AppHandle, root: String, entry: HttpHistDto) -> Result<(), String> {
    blocking(move || httpsvc::history::append(&httpsvc::history::path(&http_dir(&app)?, &root), entry.into()).map_err(|e| format!("无法记录请求历史：{e}"))).await
}

/// 这个项目的请求历史，最近的在前
#[tauri::command]
pub async fn http_history(app: tauri::AppHandle, root: String) -> Result<Vec<HttpHistDto>, String> {
    blocking(move || Ok(httpsvc::history::list(&httpsvc::history::path(&http_dir(&app)?, &root)).into_iter().map(Into::into).collect())).await
}

#[tauri::command]
pub async fn http_history_clear(app: tauri::AppHandle, root: String) -> Result<(), String> {
    blocking(move || httpsvc::history::clear(&httpsvc::history::path(&http_dir(&app)?, &root)).map_err(|e| format!("无法清除请求历史：{e}"))).await
}
