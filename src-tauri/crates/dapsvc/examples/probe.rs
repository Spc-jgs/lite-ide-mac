//! 拿真东西把调试这条路从头走一遍：起一个等调试器的 JVM（`suspend=y`、端口 0）→ 起 jdtls 带 java-debug → `startDebugSession` 拿端口 →
//! DAP initialize / attach / setBreakpoints / configurationDone → 等 stopped → stackTrace / scopes / variables → next → continue → 等它退。
//!
//! cargo run -p dapsvc --example probe -- <jdtls 目录> <java-debug 的 jar> <项目根> <缓存的上级目录> <断点那个 .java> <断点行> <起 JVM 的那一行命令>
//!
//! 不是测试：要真 jdtls、真 JDK，CI 上没有。第 5 步接进应用之前先确认每一步对得上真东西、各要多久。

use dapsvc::Session;
use lspsvc::jdtls;
use lspsvc::Server;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [home, jar, root, base, file, line, cmd] = &a[..] else {
        eprintln!("用法：probe <jdtls 目录> <java-debug jar> <项目根> <缓存的上级目录> <断点 .java> <断点行> <起 JVM 的命令>");
        std::process::exit(2);
    };
    let t0 = Instant::now();
    let ms = || format!("{:>6.0} ms", t0.elapsed().as_secs_f64() * 1000.0);
    // 1. 被调试的 JVM：从输出里认端口
    #[allow(clippy::disallowed_methods)] // 探针，不进产品
    let mut jvm = Command::new("/bin/sh").arg("-c").arg(cmd).current_dir(root).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let (port_tx, port_rx) = mpsc::channel::<u16>();
    for pipe in [Box::new(jvm.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>, Box::new(jvm.stderr.take().unwrap())] {
        let tx = port_tx.clone();
        std::thread::spawn(move || {
            for l in BufReader::new(pipe).lines().map_while(Result::ok) {
                println!("  [jvm] {l}");
                if let Some(p) = l.strip_prefix("Listening for transport dt_socket at address: ") {
                    let _ = tx.send(p.trim().parse().unwrap());
                }
            }
        });
    }
    let jvm_port = port_rx.recv_timeout(Duration::from_secs(60)).expect("60 秒没等到 JVM 的调试端口");
    println!("{} JVM 调试端口 {jvm_port}", ms());

    // 2. jdtls + java-debug
    let user = PathBuf::from(std::env::var("HOME").unwrap());
    let (_, java) = jdtls::find_jdk(&jdtls::default_jdk_dirs(&user), jdtls::MIN_JAVA).expect("没有 JDK 21+");
    let layout = jdtls::layout(Path::new(home)).unwrap();
    let ws = jdtls::workspace_dir(Path::new(base), Path::new(root));
    jdtls::prepare_config(&layout, &ws).unwrap();
    let spec = jdtls::spec(&jdtls::Launch { java_home: &java, layout: &layout, workspace: &ws, lombok: None, xmx: "1G", root: Path::new(root) });
    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let s = Server::start(
        &spec,
        Box::new(|m, p| Ok(if m == "workspace/configuration" { Value::Array(vec![Value::Null; p["items"].as_array().map_or(0, Vec::len)]) } else { Value::Null })),
        Box::new(move |m, p| {
            if m == "language/status" && p["type"] == "ServiceReady" {
                let _ = ready_tx.send(());
            }
        }),
        Box::new(|_| {}),
    )
    .unwrap();
    let mut opts = jdtls::init_options();
    opts["bundles"] = json!([jar]);
    s.initialize(&format!("file://{root}"), "root", jdtls::capabilities(), opts, Duration::from_secs(120)).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(300)).unwrap();
    println!("{} jdtls ServiceReady", ms());
    let r = s.request("workspace/executeCommand", json!({ "command": "vscode.java.startDebugSession", "arguments": [] }), Duration::from_secs(30)).unwrap();
    let dap_port = r.as_u64().expect("端口不是数") as u16;
    let mains = s.request("workspace/executeCommand", json!({ "command": "vscode.java.resolveMainClass", "arguments": [format!("file://{root}")] }), Duration::from_secs(30));
    println!("{} resolveMainClass → {:?}", ms(), mains.as_ref().map(|v| v.to_string()));
    let project = std::env::var("PROJECT").ok();
    println!("{} startDebugSession → {dap_port}", ms());

    // 3. DAP
    let (ev_tx, ev_rx) = mpsc::channel::<Value>();
    let d = Session::connect(dap_port, Box::new(move |e| { let _ = ev_tx.send(e); }), Box::new(|| println!("  [dap] 线断了"))).unwrap();
    let to = Duration::from_secs(30);
    let caps = d.request("initialize", json!({ "adapterID": "java", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsVariableType": true }), to).unwrap();
    println!("{} initialize：supportsConfigurationDoneRequest={}", ms(), caps["supportsConfigurationDoneRequest"]);
    let r = d.request("attach", json!({ "hostName": "127.0.0.1", "port": jvm_port, "timeout": 10000, "projectName": project }), to);
    println!("{} attach → {:?}", ms(), r);
    let wait = |name: &str, secs: u64| -> Value {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let e = ev_rx.recv_timeout(left).unwrap_or_else(|_| panic!("{secs} 秒没等到 {name}"));
            let ev = e["event"].as_str().unwrap_or("");
            if ev == "output" {
                print!("  [out] {}", e["body"]["output"].as_str().unwrap_or(""));
                continue;
            }
            println!("  [ev] {ev} {}", e["body"]);
            if ev == name {
                return e;
            }
        }
    };
    wait("initialized", 10);
    let line: u64 = line.parse().unwrap();
    let r = d.request("setBreakpoints", json!({ "source": { "path": file }, "breakpoints": [{ "line": line }], "lines": [line] }), to).unwrap();
    println!("{} setBreakpoints → {}", ms(), r);
    let r = d.request("configurationDone", json!({}), to);
    println!("{} configurationDone → {:?}", ms(), r);
    let st = wait("stopped", 30);
    let tid = st["body"]["threadId"].as_i64().unwrap();
    println!("{} 停下了 thread={tid} reason={}", ms(), st["body"]["reason"]);
    let t = Instant::now();
    let frames = d.request("stackTrace", json!({ "threadId": tid, "startFrame": 0, "levels": 20 }), to).unwrap();
    println!("  共 {} 帧（totalFrames={}）", frames["stackFrames"].as_array().map_or(0, Vec::len), frames["totalFrames"]);
    println!("stackTrace {:.0} ms：{}", t.elapsed().as_secs_f64() * 1000.0, frames["stackFrames"].as_array().map(|f| f.iter().map(|x| format!("{} {}:{}", x["name"], x["source"]["path"].as_str().or(x["source"]["name"].as_str()).unwrap_or("?"), x["line"])).collect::<Vec<_>>().join(" | ")).unwrap_or_default());
    let fid = frames["stackFrames"][0]["id"].as_i64().unwrap();
    let scopes = d.request("scopes", json!({ "frameId": fid }), to).unwrap();
    println!("scopes：{}", scopes);
    let vref = scopes["scopes"][0]["variablesReference"].as_i64().unwrap();
    let vars = d.request("variables", json!({ "variablesReference": vref }), to).unwrap();
    println!("variables：{}", vars);
    for v in vars["variables"].as_array().into_iter().flatten().filter(|v| v["variablesReference"].as_i64().unwrap_or(0) > 0) {
        let sub = d.request("variables", json!({ "variablesReference": v["variablesReference"] }), to).unwrap();
        println!("  展开 {}：{}", v["name"], sub);
    }
    // 往上一帧（调用者）和最底下那帧（JDK 里的）各看一眼 source 长什么样
    for f in frames["stackFrames"].as_array().into_iter().flatten().take(3) {
        println!("  帧 {} source={}", f["name"], f["source"]);
    }
    let expr = std::env::var("EXPR").unwrap_or("n * 10".into());
    let t = Instant::now();
    let ev = d.request("evaluate", json!({ "expression": expr, "frameId": fid, "context": "watch" }), to);
    println!("evaluate {expr} {:.0} ms → {:?}", t.elapsed().as_secs_f64() * 1000.0, ev);
    d.request("next", json!({ "threadId": tid }), to).unwrap();
    let st = wait("stopped", 10);
    let frames = d.request("stackTrace", json!({ "threadId": tid, "startFrame": 0, "levels": 1 }), to).unwrap();
    println!("{} next 之后停在第 {} 行（reason={}）", ms(), frames["stackFrames"][0]["line"], st["body"]["reason"]);
    // 去掉断点再继续，等它跑完
    d.request("setBreakpoints", json!({ "source": { "path": file }, "breakpoints": [], "lines": [] }), to).unwrap();
    d.request("continue", json!({ "threadId": tid }), to).unwrap();
    wait("terminated", 30);
    println!("{} 程序跑完、会话结束；JVM 退出码 {:?}", ms(), jvm.wait().map(|s| s.code()));
    let _ = s.request("shutdown", Value::Null, Duration::from_secs(10));
    let _ = s.notify("exit", Value::Null);
}
