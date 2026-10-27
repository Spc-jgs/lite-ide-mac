use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

const CURL: &str = "/usr/bin/curl";

fn tmp(name: &str) -> PathBuf {
    // 名字里不能有 `{}`（`Instant` 的 Debug 格式就有）：curl 的 `%output{路径}` 读到 `}` 就截断，统计悄悄写丢 —— 第一版就是这么「全绿」的
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("httpsvc-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 收到的一个请求：请求行 + 头（原样，小写化的头名方便找）+ 体
#[derive(Debug)]
struct Got {
    line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Got {
    fn header(&self, k: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

fn read_req(s: &mut BufReader<TcpStream>) -> Option<Got> {
    let mut line = String::new();
    s.read_line(&mut line).ok()?;
    let mut headers = Vec::new();
    loop {
        let mut l = String::new();
        s.read_line(&mut l).ok()?;
        let l = l.trim_end().to_string();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':')?;
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    let len: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).ok()?;
    Some(Got { line: line.trim_end().to_string(), headers, body })
}

/// 一个假服务：每来一个连接，按 `reply(第几个, 请求)` 回一段原样的字节。收到的请求从通道里拿
fn serve(reply: impl Fn(usize, &Got) -> Vec<u8> + Send + 'static) -> (u16, mpsc::Receiver<Got>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for (i, c) in l.incoming().enumerate() {
            let Ok(c) = c else { break };
            let mut w = c.try_clone().unwrap();
            let mut r = BufReader::new(c);
            let Some(got) = read_req(&mut r) else { continue };
            let out = reply(i, &got);
            let _ = tx.send(got);
            let _ = w.write_all(&out);
            let _ = w.flush();
        }
    });
    (port, rx)
}

fn resp(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut s = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (k, v) in headers {
        s += &format!("{k}: {v}\r\n");
    }
    s += "\r\n";
    let mut b = s.into_bytes();
    b.extend_from_slice(body);
    b
}

fn req(method: &str, url: String) -> Request {
    Request { method: method.into(), url, follow: true, ..Default::default() }
}

#[test]
fn get_json_头和状态_json_按原顺序排好_不读_curlrc() {
    // ~/.curlrc 是给终端里的 curl 写的：-q 之后它不该改变我们发出去的请求（CURL_HOME 让 curl 去这个目录找 .curlrc）
    let home = tmp("rc");
    #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
    std::fs::write(home.join(".curlrc"), "header = \"X-From-Rc: 1\"\n").unwrap();
    std::env::set_var("CURL_HOME", &home);
    let (port, rx) = serve(|_, _| resp("200 OK", &[("Content-Type", "application/json;charset=UTF-8"), ("X-Trace", "abc")], br#"{"zeta":1,"alpha":[true,null]}"#));
    let dir = tmp("get");
    let mut r = req("GET", format!("http://127.0.0.1:{port}/api/orders?x=1"));
    r.headers.push(("Accept".into(), "application/json".into()));
    let got = send(Path::new(CURL), &r, &dir, 1, &Handle::default()).unwrap();
    let sent = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(sent.line, "GET /api/orders?x=1 HTTP/1.1");
    assert_eq!(sent.header("accept"), Some("application/json"));
    assert_eq!(sent.header("x-from-rc"), None, "读了 ~/.curlrc（少了 -q）");
    assert_eq!((got.version.as_str(), got.status, got.reason.as_str()), ("HTTP/1.1", 200, "OK"));
    assert!(got.headers.iter().any(|(k, v)| k == "X-Trace" && v == "abc"));
    assert!(got.pretty && got.body.extension().unwrap() == "json");
    assert_eq!(std::fs::read_to_string(&got.body).unwrap(), "{\n  \"zeta\": 1,\n  \"alpha\": [\n    true,\n    null\n  ]\n}");
    assert_eq!(got.bytes, 30);
    assert!(!dir.join("hdr-1.txt").exists() && !dir.join("stats-1.json").exists(), "临时文件用完要删");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn post_请求体原样_不替你加表单的_content_type_写了的照带() {
    let (port, rx) = serve(|_, _| resp("201 Created", &[], b""));
    let dir = tmp("post");
    let mut r = req("POST", format!("http://127.0.0.1:{port}/api/orders"));
    r.body = Some("{\"sku\": \"A-1\", \"qty\": 2}\n第二行 @不是文件".as_bytes().to_vec());
    let got = send(Path::new(CURL), &r, &dir, 2, &Handle::default()).unwrap();
    let sent = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(got.status, 201);
    assert_eq!(sent.line, "POST /api/orders HTTP/1.1");
    assert_eq!(String::from_utf8(sent.body.clone()).unwrap(), "{\"sku\": \"A-1\", \"qty\": 2}\n第二行 @不是文件");
    assert_eq!(sent.header("content-type"), None, "curl 自作主张加了表单的 Content-Type");

    r.headers.push(("Content-Type".into(), "application/json".into()));
    r.headers.push(("X-Empty".into(), String::new()));
    r.method = "PUT".into();
    send(Path::new(CURL), &r, &dir, 3, &Handle::default()).unwrap();
    let sent = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(sent.line, "PUT /api/orders HTTP/1.1");
    assert_eq!(sent.header("content-type"), Some("application/json"));
    assert_eq!(sent.header("x-empty"), Some(""), "空值的头要照发（`名字:` 在 curl 里是删掉它）");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 跟着重定向走_取最后一段头_不跟的时候停在_302() {
    let (port, _rx) = serve(move |i, g| {
        if g.line.starts_with("GET /old") {
            resp("302 Found", &[("Location", "/new")], b"")
        } else {
            resp("200 OK", &[("Content-Type", "text/plain")], format!("第 {i} 个连接").as_bytes())
        }
    });
    let dir = tmp("redir");
    let got = send(Path::new(CURL), &req("GET", format!("http://127.0.0.1:{port}/old")), &dir, 4, &Handle::default()).unwrap();
    // 重定向次数只有 curl 的统计里有：统计没读到（`%output{}` 写丢了）这里就是 0
    assert_eq!((got.status, got.redirects), (200, 1));
    assert!(got.final_url.ends_with("/new"), "{}", got.final_url);
    assert_eq!(got.body.extension().unwrap(), "txt");
    let mut r = req("GET", format!("http://127.0.0.1:{port}/old"));
    r.follow = false;
    let got = send(Path::new(CURL), &r, &dir, 5, &Handle::default()).unwrap();
    assert_eq!((got.status, got.redirects), (302, 0), "@no-redirect：停在 302");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 不对的请求不发_只许_http_https_值里不许换行() {
    let dir = tmp("bad");
    let h = Handle::default();
    let e = |r: Request| send(Path::new(CURL), &r, &dir, 9, &h).unwrap_err();
    assert!(matches!(e(req("GET", "file:///etc/passwd".into())), Error::Invalid(_)));
    assert!(matches!(e(req("GET", "gopher://x".into())), Error::Invalid(_)));
    assert!(matches!(e(req("GE T", "http://x".into())), Error::Invalid(_)));
    let mut r = req("GET", "http://127.0.0.1:1/".into());
    r.headers.push(("X-A".into(), "1\r\nX-Injected: 2".into()));
    assert!(matches!(e(r), Error::Invalid(_)), "一个头的值不能拆成两个头");
    let mut r = req("GET", "http://127.0.0.1:1/".into());
    r.headers.push(("X A".into(), "1".into()));
    assert!(matches!(e(r), Error::Invalid(_)));
    assert!(matches!(e(req("GET", "http://a b/".into())), Error::Invalid(_)));
    // 重定向到 http / https 以外的也不跟（--proto-redir）。用 ftp:// 测：curl 7.65 起默认就不许重定向到 file://，拿它测的话去掉 --proto-redir 照样过
    //（第一版就是这么写的，验红时发现它是一条空断言）；ftp 是 curl 默认允许跟过去的
    let (port, _rx) = serve(|_, _| resp("302 Found", &[("Location", "ftp://127.0.0.1:1/x")], b""));
    let got = send(Path::new(CURL), &req("GET", format!("http://127.0.0.1:{port}/")), &dir, 10, &h);
    assert!(matches!(&got, Err(Error::Curl(m)) if m.starts_with("协议不受支持")), "跟着重定向去了 ftp://：{got:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 连不上说人话() {
    // 先占一个端口再放掉：多半没人在听
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let dir = tmp("refused");
    let e = send(Path::new(CURL), &req("GET", format!("http://127.0.0.1:{port}/")), &dir, 11, &Handle::default()).unwrap_err();
    assert!(matches!(&e, Error::Curl(m) if m.starts_with("无法连接")), "{e:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 超过上限截断照实说_收到的真字节数() {
    let big = vec![b'x'; 5000];
    let (port, _rx) = serve(move |_, _| resp("200 OK", &[("Content-Type", "text/plain")], &big));
    let dir = tmp("cap");
    let got = send_capped(Path::new(CURL), &req("GET", format!("http://127.0.0.1:{port}/")), &dir, 12, &Handle::default(), 1000).unwrap();
    assert!(got.truncated);
    assert!(got.bytes > 1000, "要报真数字：{}", got.bytes);
    assert_eq!(std::fs::metadata(&got.body).unwrap().len(), 1000);
    assert_eq!(got.status, 200, "截断而杀掉的退出码不算失败，头照样收到了");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 取消_正在等的请求当场停() {
    // 只收不回：请求一直挂着（像停在断点上的服务）
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let keep: Vec<_> = l.incoming().take(1).flatten().collect();
        std::thread::sleep(Duration::from_secs(30));
        drop(keep);
    });
    let dir = tmp("cancel");
    let h = Arc::new(Handle::default());
    let h2 = Arc::clone(&h);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        h2.cancel();
    });
    let t = Instant::now();
    let e = send(Path::new(CURL), &req("GET", format!("http://127.0.0.1:{port}/")), &dir, 13, &h).unwrap_err();
    assert_eq!(e, Error::Cancelled);
    assert!(t.elapsed() < Duration::from_secs(3), "取消要当场生效：{:?}", t.elapsed());
    // 还没起就取消了：不起
    let h = Handle::default();
    h.cancel();
    assert_eq!(send(Path::new(CURL), &req("GET", format!("http://127.0.0.1:{port}/")), &dir, 14, &h).unwrap_err(), Error::Cancelled);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 响应头_取最后一段_http2_没有原因短语() {
    let raw = "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 302 Found\r\nLocation: /b\r\n\r\nHTTP/2 200\r\ncontent-type: application/json\r\nx-a: 1:2\r\n\r\n";
    let (v, s, reason, h) = parse_headers(raw).unwrap();
    assert_eq!((v.as_str(), s, reason.as_str()), ("HTTP/2", 200, ""));
    assert_eq!(h, vec![("content-type".to_string(), "application/json".to_string()), ("x-a".to_string(), "1:2".to_string())]);
}

#[test]
fn 只清自己的响应体_留最近几个() {
    let dir = tmp("prune");
    for (i, n) in ["resp-1.json", "resp-2.txt", "resp-10.json", "notes.txt", "resp-x.json"].iter().enumerate() {
        #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
        std::fs::write(dir.join(n), i.to_string()).unwrap();
    }
    prune(&dir, 2);
    let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, vec!["notes.txt", "resp-10.json", "resp-2.txt", "resp-x.json"], "按序号留最近两个（10 比 2 新，不是按字母），别的名字一个不碰");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 请求体的文件只许在项目里_软链出去的也不行() {
    let base = tmp("bodyfile");
    let root = base.join("proj");
    std::fs::create_dir_all(root.join("api")).unwrap();
    #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
    {
        std::fs::write(root.join("api/body.json"), "{}").unwrap();
        std::fs::write(base.join("secret.txt"), "私钥").unwrap();
    }
    std::os::unix::fs::symlink(base.join("secret.txt"), root.join("api/link.txt")).unwrap();
    assert_eq!(body_file_inside(&root, &root.join("api/body.json")), Ok(()), "项目里的");
    assert!(matches!(body_file_inside(&root, &root.join("api/../../secret.txt")), Err(Error::Invalid(m)) if m.contains("项目之外")), "../ 出去的");
    assert!(matches!(body_file_inside(&root, &base.join("secret.txt")), Err(Error::Invalid(_))), "绝对路径指到外面的");
    assert!(matches!(body_file_inside(&root, &root.join("api/link.txt")), Err(Error::Invalid(_))), "项目里一个软链指到外面：按解开之后的路径算");
    assert_eq!(body_file_inside(&root, &root.join("api/nope.json")), Ok(()), "不存在的交给 curl 报「读不了」");
    assert!(matches!(body_file_inside(&base.join("gone"), &root.join("api/body.json")), Err(Error::Invalid(_))), "项目根找不到：不发（往「不做」那边倒）");
    // 接在发送路径上：项目外面的文件，curl 都不起
    let mut r = req("POST", "http://127.0.0.1:9/x".into());
    r.root = root.clone();
    r.body_file = Some(root.join("api/link.txt"));
    let out = send(Path::new(CURL), &r, &base.join("out"), next_seq(), &Handle::default());
    assert!(matches!(out, Err(Error::Invalid(ref m)) if m.contains("项目之外")), "{out:?}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn cookie_罐_登录拿到的下一个请求带上_不记的不带也不写_文件只有自己能读() {
    let (port, rx) = serve(|i, _| {
        if i == 0 {
            resp("200 OK", &[("Set-Cookie", "SESSION=abc123; Path=/; HttpOnly"), ("Set-Cookie", "theme=dark; Path=/; Max-Age=3600")], b"{}")
        } else {
            resp("200 OK", &[], b"{}")
        }
    });
    let dir = tmp("jar");
    let jar = jar_path(&dir, "/proj", Some("dev"));
    let url = |p: &str| format!("http://127.0.0.1:{port}{p}");
    let with = |p: &str, j: Option<PathBuf>| Request { cookie_jar: j, ..req("GET", url(p)) };
    send(Path::new(CURL), &with("/login", Some(jar.clone())), &dir, next_seq(), &Handle::default()).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().header("cookie").is_none(), "第一次罐子是空的");
    send(Path::new(CURL), &with("/orders", Some(jar.clone())), &dir, next_seq(), &Handle::default()).unwrap();
    let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let sent = got.header("cookie").unwrap_or_default().to_string();
    assert!(sent.contains("SESSION=abc123") && sent.contains("theme=dark"), "下一个请求带上了登录拿到的：{sent}");
    send(Path::new(CURL), &with("/public", None), &dir, next_seq(), &Handle::default()).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().header("cookie").is_none(), "@no-cookie-jar 的不带");

    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode(&jar), mode(jar.parent().unwrap())), (0o600, 0o700), "里面是登录态：只有自己能读");
    let mut list = cookies(&jar);
    list.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(list.iter().map(|c| (c.name.as_str(), c.http_only, c.expires > 0)).collect::<Vec<_>>(), [("SESSION", true, false), ("theme", false, true)], "{list:?}");
    assert!(list.iter().all(|c| c.domain.contains("127.0.0.1") && c.path == "/"));
    assert_ne!(jar, jar_path(&dir, "/proj", Some("prod")), "换个环境是另一个罐子");
    assert_ne!(jar, jar_path(&dir, "/other", Some("dev")), "换个项目也是");
    clear_cookies(&jar).unwrap();
    assert!(cookies(&jar).is_empty() && clear_cookies(&jar).is_ok(), "清掉了；再清一次不算错");
}
