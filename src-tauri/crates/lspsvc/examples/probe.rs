//! 拿真 jdtls 验这个 crate：自己找 JDK 21 和 Lombok、起、等 ServiceReady、问一次跳转、收一轮报错、按协议收尾。
//!
//! cargo run -p lspsvc --example probe -- <jdtls 目录> <项目根> <缓存的上级目录> <要打开的 .java> <跳转的那个词>
//!
//! 不是测试：要真 jdtls、真 JDK 21，CI 上没有。第 2 步接进应用之前先用它确认线、进程、参数三层对得上真东西。

use lspsvc::jdtls;
use lspsvc::Server;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [home, root, base, file, word] = &a[..] else {
        eprintln!("用法：probe <jdtls 目录> <项目根> <缓存的上级目录> <要打开的 .java> <跳转的那个词>");
        std::process::exit(2);
    };
    let user = PathBuf::from(std::env::var("HOME").unwrap());
    let (v, java) = jdtls::find_jdk(&jdtls::default_jdk_dirs(&user), jdtls::MIN_JAVA).expect("没有 JDK 21+");
    let settings = std::fs::read_to_string(user.join(".m2/settings.xml")).ok();
    let repo = jdtls::local_repo(&user, settings.as_deref());
    let lombok = jdtls::lombok_jar(&repo);
    let layout = jdtls::layout(Path::new(home)).unwrap();
    let root = Path::new(root);
    let ws = jdtls::workspace_dir(Path::new(base), root);
    jdtls::prepare_config(&layout, &ws).unwrap();
    println!("JDK {v} {}", java.display());
    println!("Lombok {}", lombok.as_ref().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).unwrap_or("没找到".into()));
    println!("缓存 {}（{}）", ws.display(), if ws.join("ws/.metadata").is_dir() { "有" } else { "冷" });
    let spec = jdtls::spec(&jdtls::Launch { java_home: &java, layout: &layout, workspace: &ws, lombok: lombok.as_deref(), xmx: "1G", root });

    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let (diag_tx, diag_rx) = mpsc::channel::<(String, Value)>();
    let (exit_tx, exit_rx) = mpsc::channel();
    let t0 = Instant::now();
    let s = Server::start(
        &spec,
        Box::new(|m, p| {
            Ok(match m {
                "workspace/configuration" => Value::Array(vec![Value::Null; p["items"].as_array().map_or(0, Vec::len)]),
                _ => Value::Null,
            })
        }),
        Box::new(move |m, p| match m {
            "language/status" if p["type"] == "ServiceReady" => {
                let _ = ready_tx.send(());
            }
            "textDocument/publishDiagnostics" => {
                let _ = diag_tx.send((p["uri"].as_str().unwrap_or("").to_owned(), p["diagnostics"].clone()));
            }
            _ => {}
        }),
        Box::new(move |e| {
            let _ = exit_tx.send(e);
        }),
    )
    .unwrap();
    let uri = |p: &Path| format!("file://{}", p.display());
    let mut opts = jdtls::init_options();
    // GUESS=off|insertParameterNames|insertBestGuessedArguments：试 jdtls 的方法补全插什么
    if let Ok(g) = std::env::var("GUESS") {
        opts["settings"]["java"]["completion"] = json!({ "guessMethodArguments": g });
    }
    if let Err(e) = s.initialize(&uri(root), "root", jdtls::capabilities(), opts, Duration::from_secs(120)) {
        let x = exit_rx.recv_timeout(Duration::from_secs(5));
        panic!("initialize 失败：{e:?}；退出：{:?}", x.map(|x| (x.status, x.stderr_tail)));
    }
    println!("initialize {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);
    ready_rx.recv_timeout(Duration::from_secs(300)).expect("300 秒没等到 ServiceReady");
    println!("ServiceReady {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);

    let path = Path::new(file);
    let text = std::fs::read_to_string(path).unwrap();
    s.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri(path), "languageId": "java", "version": 1, "text": text } })).unwrap();
    let (line, col) = text
        .lines()
        .enumerate()
        .find_map(|(i, l)| l.find(word.as_str()).map(|c| (i, c + 1)))
        .expect("文件里没有那个词");
    let t = Instant::now();
    let r = s.request(
        "textDocument/definition",
        json!({ "textDocument": { "uri": uri(path) }, "position": { "line": line, "character": col } }),
        Duration::from_secs(30),
    );
    println!("跳到定义 {word}（第 {} 行）{:.0} ms → {}", line + 1, t.elapsed().as_secs_f64() * 1000.0, match &r {
        Ok(v) => v.get(0).map_or("空".into(), |l| format!("{} 第 {} 行", l["uri"].as_str().unwrap_or("?").rsplit('/').next().unwrap_or(""), l["range"]["start"]["line"].as_u64().unwrap_or(0) + 1)),
        Err(e) => format!("错：{e}"),
    });
    // COMPL=<一段文字>：在第一处出现它的末尾问一次补全，打出前几项原样、再 resolve 第一项（第 3 步看 jdtls 的补全长什么样）
    if let Ok(after) = std::env::var("COMPL") {
        let (l, c) = text.lines().enumerate().find_map(|(i, l)| l.find(after.as_str()).map(|c| (i, c + after.len()))).expect("没有那段文字");
        let t = Instant::now();
        let r = s.request("textDocument/completion", json!({ "textDocument": { "uri": uri(path) }, "position": { "line": l, "character": c } }), Duration::from_secs(30)).unwrap();
        let items = r.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        println!("补全 {:.0} ms，{} 项，isIncomplete={}", t.elapsed().as_secs_f64() * 1000.0, items.len(), r["isIncomplete"]);
        let pick = std::env::var("PICK").ok();
        for it in items.iter().filter(|i| pick.as_deref().is_none_or(|p| i["label"].as_str().unwrap_or("").starts_with(p))).take(3) {
            println!("  项：{}", it);
        }
        if let Some(first) = items.iter().find(|i| pick.as_deref().is_none_or(|p| i["label"].as_str().unwrap_or("").starts_with(p))) {
            let t = Instant::now();
            let res = s.request("completionItem/resolve", first.clone(), Duration::from_secs(30)).unwrap();
            println!("  resolve {:.0} ms：{}", t.elapsed().as_secs_f64() * 1000.0, res);
        }
    }
    // RENAME=<词>=<新名字>：在文件里第一处出现那个词的地方问 prepareRename + rename，打出原样的 WorkspaceEdit（第 3 步看改名长什么样）
    if let Ok(spec) = std::env::var("RENAME") {
        let (from, to) = spec.split_once('=').expect("RENAME=旧=新");
        let (l, c) = text.lines().enumerate().find_map(|(i, l)| l.find(from).map(|c| (i, c + 1))).expect("没有那个词");
        let pos = json!({ "textDocument": { "uri": uri(path) }, "position": { "line": l, "character": c } });
        let t = Instant::now();
        let p = s.request("textDocument/prepareRename", pos.clone(), Duration::from_secs(30));
        println!("prepareRename {:.0} ms：{:?}", t.elapsed().as_secs_f64() * 1000.0, p.map(|v| v.to_string()));
        let mut req = pos;
        req["newName"] = json!(to);
        let t = Instant::now();
        let r = s.request("textDocument/rename", req, Duration::from_secs(60));
        println!("rename {:.0} ms：{}", t.elapsed().as_secs_f64() * 1000.0, r.map(|v| v.to_string()).unwrap_or_else(|e| format!("错：{e}")));
    }
    if std::env::var("RAW").is_ok() {
        println!("原样：{}", r.as_ref().map(|v| v.to_string()).unwrap_or_default());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Ok((u, d)) = diag_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if u == uri(path) {
            let errs: Vec<_> = d.as_array().unwrap().iter().filter(|x| x["severity"] == 1).map(|x| x["message"].as_str().unwrap_or("").to_owned()).collect();
            println!("这个文件的报错 {} 条 {:?}", errs.len(), errs);
            break;
        }
    }
    println!("RSS {} MB", rss(s.pid()));
    if let Some(w) = std::env::var("SETTLE").ok().and_then(|g| g.parse().ok()) {
        std::thread::sleep(Duration::from_secs(w));
        println!("等了 {w} 秒再收尾，RSS {} MB", rss(s.pid()));
    }
    let t = Instant::now();
    let grace = std::env::var("GRACE").ok().and_then(|g| g.parse().ok()).unwrap_or(3);
    s.shutdown(Duration::from_secs(grace));
    let e = exit_rx.recv_timeout(Duration::from_secs(5)).expect("收尾之后没报退出");
    println!("收尾 {:.0} ms：{}（我们停的：{}）", t.elapsed().as_secs_f64() * 1000.0, e.status, e.stopped);
}

fn rss(pid: i32) -> u64 {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTASKINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    if n == size { info.pti_resident_size >> 20 } else { 0 }
}
