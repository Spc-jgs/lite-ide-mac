//! 测试通道：只在 `--features test-bridge` 的测试构建里存在（`scripts/build-test-app.sh`），**正式包里没有这个文件**。
//!
//! # 为什么要有它
//!
//! 验收和 smoke 原来站在应用外面模仿人：System Events 敲键、AX 树找元素点、截图量字。三样都靠不住 ——
//!
//! - 敲键发给「当前最前面的应用」，而让测试应用到最前这一步会被系统悄悄拦掉：2026-10-08 的 ⌘= 全落进了用户的 Claude 应用；
//! - AX 点击偶尔落空（#22），后台窗口里网页的 AX 树读不全；
//! - 截图要屏幕录制权限，权限会过期。
//!
//! 这里让测试**在应用内部**下指令、读状态：不抢焦点（应用用 `open -g` 在后台起），用户可以照常用电脑。
//!
//! # 协议
//!
//! 环境变量 `LITE_IDE_TEST_SOCK=<路径>` 设了才开（光编进去不够），在那个路径上开一个 Unix socket，权限 0600。
//! 一行一条 JSON 请求，一行一条 JSON 回应（`{"ok":true,"value":…}` / `{"ok":false,"error":"…"}`）：
//!
//! | 请求 | 做什么 |
//! |---|---|
//! | `{"cmd":"windows"}` | 开着的窗口和各自的项目根，前台的在前 |
//! | `{"cmd":"menu","id":"save"}` | 触发菜单项 —— 和点原生菜单走同一个函数（`winctl::menu_event`）；带 `"window"` 就只发给那个窗口 |
//! | `{"cmd":"close","window":"w-1"}` | 关一个窗口（和点红叉一样走 `Destroyed`） |
//! | `{"cmd":"eval","window":"main","js":"return …"}` | 在那个窗口里跑一段 JS（函数体，可以 `await`），把 `return` 的值拿回来 |
//!
//! `eval` 的回话走 Tauri 现成的事件通道：页面跑完 `emit("test-reply")`，这边只在测试构建里听它 —— 正式版里不需要任何测试专用的命令。
//! 前端的状态对象由 `src/lib/dev/test-hooks.ts` 挂在 `window.__lite` 上（同样只在测试构建里，见 main.ts）。
//!
//! # 安全
//!
//! 它等于「本机任何能连上这个 socket 的进程都能操作编辑器」。所以三道：只在测试构建里编译、要设环境变量才开、socket 只有本人能读写。

use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Listener, Manager};

/// `eval` 等页面回话最多等多久。页面卡死时测试要报错，不能挂住
const EVAL_WAIT: Duration = Duration::from_secs(15);

fn pending() -> &'static Mutex<HashMap<u64, mpsc::Sender<Value>>> {
    static P: OnceLock<Mutex<HashMap<u64, mpsc::Sender<Value>>>> = OnceLock::new();
    P.get_or_init(Default::default)
}

/// `setup` 里调。没设 `LITE_IDE_TEST_SOCK` 就什么都不做
pub fn start(app: &AppHandle) {
    let Ok(path) = std::env::var("LITE_IDE_TEST_SOCK") else { return };
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[testbridge] 开不了 {path}：{e}");
            return;
        }
    };
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    // 页面跑完 eval 的回话
    app.listen_any("test-reply", |ev| {
        let Ok(v) = serde_json::from_str::<Value>(ev.payload()) else { return };
        let Some(id) = v.get("id").and_then(Value::as_u64) else { return };
        if let Some(tx) = pending().lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
            let _ = tx.send(v);
        }
    });
    eprintln!("[testbridge] 在听 {path}");
    let app = app.clone();
    let _ = std::thread::Builder::new().name("testbridge".into()).spawn(move || {
        for s in listener.incoming().flatten() {
            let app = app.clone();
            let _ = std::thread::spawn(move || serve(&app, s));
        }
    });
}

fn serve(app: &AppHandle, s: UnixStream) {
    let Ok(mut out) = s.try_clone() else { return };
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { return };
        let resp = match serde_json::from_str::<Value>(&line) {
            Ok(req) => run(app, &req),
            Err(e) => Err(format!("不是 JSON：{e}")),
        };
        let v = match resp {
            Ok(value) => json!({ "ok": true, "value": value }),
            Err(error) => json!({ "ok": false, "error": error }),
        };
        if writeln!(out, "{v}").is_err() {
            return;
        }
    }
}

fn run(app: &AppHandle, req: &Value) -> Result<Value, String> {
    let s = |k: &str| req.get(k).and_then(Value::as_str);
    match s("cmd").ok_or("缺 cmd")? {
        "windows" => {
            let list = app.state::<AppState>().windows.list();
            Ok(Value::Array(list.into_iter().map(|(label, root)| json!({ "label": label, "root": root })).collect()))
        }
        "menu" => {
            let id = s("id").ok_or("缺 id")?;
            match s("window") {
                Some(w) => app.emit_to(w, "menu", id).map_err(|e| e.to_string())?,
                None => crate::winctl::menu_event(app, id),
            }
            Ok(Value::Null)
        }
        "close" => {
            let w = s("window").ok_or("缺 window")?;
            app.get_webview_window(w).ok_or(format!("没有窗口 {w}"))?.close().map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "eval" => {
            let w = s("window").ok_or("缺 window")?;
            let js = s("js").ok_or("缺 js")?;
            eval(app, w, js)
        }
        other => Err(format!("不认识的指令：{other}")),
    }
}

/// 在窗口里跑一段 JS（函数体），等它 `emit("test-reply")` 回话
fn eval(app: &AppHandle, window: &str, body: &str) -> Result<Value, String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let wv = app.get_webview_window(window).ok_or(format!("没有窗口 {window}"))?;
    let (tx, rx) = mpsc::channel();
    pending().lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
    // 值先在页面里 JSON 化成字符串再带回来：DOM 节点、循环引用之类序列化不了的，在那边就变成错误信息，不让事件通道去猜
    let wrapped = format!(
        r#"(async () => {{
  let ok = true, value;
  try {{ value = await (async () => {{ {body}
  }})(); }} catch (e) {{ ok = false; value = String((e && e.stack) || e); }}
  let text;
  try {{ text = JSON.stringify(value === undefined ? null : value); }} catch (e) {{ ok = false; text = JSON.stringify("回话序列化不了：" + e); }}
  window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {{ event: "test-reply", payload: {{ id: {id}, ok, text }} }});
}})();"#
    );
    if let Err(e) = wv.eval(&wrapped) {
        pending().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        return Err(e.to_string());
    }
    let reply = rx.recv_timeout(EVAL_WAIT).map_err(|_| {
        pending().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        // 没回话多半是页面里那段根本没跑起来（语法错、被别的东西截断）：把送进去的原文记下来，看页面收到的是什么
        crate::diag!("eval#{id} 在 {window} 里 {} 秒没回话，送进去的是：{wrapped}", EVAL_WAIT.as_secs());
        format!("{window} 在 {} 秒内没回话", EVAL_WAIT.as_secs())
    })?;
    let text = reply.get("text").and_then(Value::as_str).unwrap_or("null");
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    if reply.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(value)
    } else {
        Err(value.as_str().unwrap_or("页面里出错了").to_string())
    }
}
