//! 发 `.http` 文件里的请求（docs/IDEA.md 第 19 节）。**交给系统的 curl**，不自己带一个 HTTP 库：
//! - macOS 自带（`/usr/bin/curl`），不加依赖；
//! - 走系统的证书库和代理环境变量 —— 公司内网的 CA、`HTTPS_PROXY`，和你在终端里 curl 一样（自己带的库要么只认 Mozilla 的根证书、要么要另配）；
//! - 计时、重定向、gzip 解压它都做好了，`-w '%{json}'` 一次把统计全给出来。
//!
//! 解析 `.http`、替换变量在前端（行号旁的 ▶ 本来就要知道每个请求在哪一行）；交到这里的已经是替换好的请求。这里管的是**交给 curl 的东西安全**：
//! - `-q` 放第一个：不读 `~/.curlrc`（那是用户给终端里的 curl 写的，不该悄悄改变我们发出去的请求）
//! - 只许 http / https：curl 默认还认 `file://`、`dict://`、`gopher://` 等二十几种，`.http` 文件是项目里带来的东西。第一跳先由 [`validate`] 卡；
//!   `--proto =http,https` 管每一跳（它禁掉的 `--proto-redir` 也放不开，curl 文档原话），`--proto-redir` 再单独管一次重定向 ——
//!   两道是重复的，留着是多一道防线：验红时只去掉其中一个测试照样绿，两个都去掉才红（重定向去 `ftp://` 那条）
//! - 方法、头名按 token 的字符卡，头的值不许带换行（不然一行能拆成两个头），全部走 `Command::arg`、不拼 shell
//! - 请求体先写进我们自己的临时文件再 `--data-binary @文件` —— 不让文件内容被 curl 当成参数解释
//! - `< 文件` 只许读项目里的（[`body_file_inside`]）：`.http` 是项目带来的，一句 `< ../../.ssh/id_rsa` 加一个外面的地址，点一下 ▶ 私钥就发出去了
//!
//! 响应体走 procutil 的上限（[`BODY_MAX`]），超了照实说截断；头和统计写进两个小临时文件（`-D`、`%output{}`），用完删。
//! 取消：另一条线程按 pid 发 SIGKILL（[`Handle::cancel`]）。pid 在收尸之前摘掉 —— 没收尸之前它还是我们的僵尸，不会被系统分给别人。

pub mod env;
pub mod json;

use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// 响应体最多收多少：一个几十 MB 的接口返回值已经不是「看一眼」的东西了。超了截断、照实说
pub const BODY_MAX: usize = 32 << 20;
/// 这么大以内的 JSON 才重新缩进（再大的原样显示：重排要整份进内存再写一份）
pub const PRETTY_MAX: usize = 8 << 20;
/// 连不上最多等多久。**不设总时长**：调试的时候请求停在断点上几分钟是常事（第 5 步），要停用取消
const CONNECT_TIMEOUT: &str = "10";

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// 写在 `.http` 里的请求体
    pub body: Option<Vec<u8>>,
    /// `< ./file.json`：从文件读请求体（绝对路径，前端按 `.http` 所在目录解析好了）。必须在 `root` 底下（[`body_file_inside`]）
    pub body_file: Option<PathBuf>,
    /// 项目根：`body_file` 只许在它底下
    pub root: PathBuf,
    /// 跟着重定向走（IDEA 默认跟；`# @no-redirect` 不跟）
    pub follow: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Response {
    /// `HTTP/1.1`、`HTTP/2`
    pub version: String,
    pub status: u16,
    /// `OK`、`Not Found`（HTTP/2 没有这一段，空的）
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub ms: u64,
    /// 响应体多少字节（收到的，截断之前的真数字是 `bytes`、存下来的最多 [`BODY_MAX`]）
    pub bytes: u64,
    /// 响应体存在哪（JSON 的话是排好的那份）
    pub body: PathBuf,
    pub truncated: bool,
    /// 重新缩进过（是 JSON、不太大、排得开）
    pub pretty: bool,
    /// 跟了几次重定向；最后落在哪
    pub redirects: u32,
    pub final_url: String,
}

#[derive(Debug, PartialEq)]
pub enum Error {
    /// 请求本身不对（方法、URL、头）：没发出去
    Invalid(String),
    /// curl 说的（连不上、超时、证书）。已经翻成人话，带 curl 的原话
    Curl(String),
    Cancelled,
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Invalid(m) | Error::Curl(m) | Error::Io(m) => write!(f, "{m}"),
            Error::Cancelled => write!(f, "已取消"),
        }
    }
}

/// 一次请求的取消开关。`cancel` 可以在任何线程、任何时候调（还没起、正在等、已经完了都行）
#[derive(Default)]
pub struct Handle {
    pid: Mutex<Option<u32>>,
    cancelled: AtomicBool,
}

impl Handle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(p) = *self.pid.lock().unwrap_or_else(|e| e.into_inner()) {
            // SAFETY: 只是发信号；pid 在收尸之前一直是我们的子进程（见文件头）
            unsafe {
                libc::kill(p as i32, libc::SIGKILL);
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// RFC 7230 的 token：方法和头名只能是这些字符
fn token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

pub fn validate(r: &Request) -> Result<(), Error> {
    if !token(&r.method) {
        return Err(Error::Invalid(format!("方法「{}」不对", r.method)));
    }
    let lower = r.url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(Error::Invalid(format!("只发 http:// 和 https:// 的请求：{}", r.url)));
    }
    if r.url.chars().any(|c| c.is_control() || c == ' ') {
        return Err(Error::Invalid(format!("URL 里有空格或控制字符（变量的值带了换行？）：{}", r.url.escape_debug())));
    }
    for (k, v) in &r.headers {
        if !token(k) {
            return Err(Error::Invalid(format!("头的名字「{k}」不对")));
        }
        if v.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
            return Err(Error::Invalid(format!("头「{k}」的值里有换行（变量的值带了换行？）")));
        }
    }
    if r.body.is_some() && r.body_file.is_some() {
        return Err(Error::Invalid("请求体写了一份、又从文件读一份".into()));
    }
    Ok(())
}

/// `< 文件` 只许是项目里的文件（解开软链之后）。IDEA 不拦这个；我们拦，理由见文件头：`.http` 和它旁边的软链都是项目带来的，
/// 别人仓库里的一个请求不该能把你机器上项目外面的文件发出去。文件不存在不在这儿拦 —— 交给 curl 报「读不了请求体的文件」（退出码 26）
/// 项目根解析不出来就不发（加固出错往「不做」那边倒，rust.md）
pub fn body_file_inside(root: &Path, file: &Path) -> Result<(), Error> {
    let Ok(f) = file.canonicalize() else { return Ok(()) };
    let r = root.canonicalize().map_err(|e| Error::Invalid(format!("找不到项目根（{}：{e}），没发带文件的请求", root.display())))?;
    if f.starts_with(&r) {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "请求体的文件在项目外面（{}），没发：.http 是项目带来的文件，从项目外面读文件发出去可能把你的私钥、配置发给别人。要发就把文件放进项目里",
        f.display()
    )))
}

/// 这次请求用到的几个临时文件（都在调用方给的目录里，名字带序号，用完删）
pub struct Files {
    pub headers: PathBuf,
    pub stats: PathBuf,
    pub body_in: PathBuf,
}

impl Files {
    pub fn new(dir: &Path, seq: u64) -> Files {
        Files { headers: dir.join(format!("hdr-{seq}.txt")), stats: dir.join(format!("stats-{seq}.json")), body_in: dir.join(format!("req-{seq}.bin")) }
    }

    fn remove(&self) {
        for p in [&self.headers, &self.stats, &self.body_in] {
            #[allow(clippy::disallowed_methods)] // 我们自己这次请求的临时文件（名字带序号），不是用户的
            let _ = std::fs::remove_file(p);
        }
    }
}

/// 交给 curl 的参数（不含程序名）。`body_in`：请求体已经写到哪了（写在 `.http` 里的那种）
pub fn curl_args(r: &Request, f: &Files) -> Vec<String> {
    let mut a: Vec<String> = vec!["-q".into(), "-sS".into()];
    a.extend(["--proto", "=http,https", "--proto-redir", "=http,https", "--connect-timeout", CONNECT_TIMEOUT, "--compressed"].map(String::from));
    if r.follow {
        a.extend(["-L", "--max-redirs", "10"].map(String::from));
    }
    let body = r.body.as_ref().map(|_| f.body_in.clone()).or_else(|| r.body_file.clone());
    match r.method.as_str() {
        // HEAD 要用 -I：`-X HEAD` 会让 curl 等一个永远不来的响应体
        "HEAD" => a.push("-I".into()),
        // GET 不带 -X；带请求体的 POST 也不带（--data-binary 本来就是 POST），这样 301/302/303 时 curl 像浏览器一样改成 GET
        "GET" => {}
        "POST" if body.is_some() => {}
        m => a.extend(["-X".to_string(), m.to_string()]),
    }
    a.extend(["-D".to_string(), f.headers.display().to_string()]);
    a.extend(["-w".to_string(), format!("%output{{{}}}%{{json}}", f.stats.display())]);
    let mut has_ct = false;
    for (k, v) in &r.headers {
        has_ct |= k.eq_ignore_ascii_case("content-type");
        // 空值要写成 `名字;`：`名字:` 在 curl 里是「删掉这个头」
        a.push("-H".into());
        a.push(if v.is_empty() { format!("{k};") } else { format!("{k}: {v}") });
    }
    if let Some(p) = body {
        a.push("--data-binary".into());
        a.push(format!("@{}", p.display()));
        // --data-binary 会自作主张加 `Content-Type: application/x-www-form-urlencoded`。没写就不带（发出去的就是文件里写的）
        if !has_ct {
            a.extend(["-H".to_string(), "Content-Type:".to_string()]);
        }
    }
    a.extend(["--url".to_string(), r.url.clone()]);
    a
}

/// `-D` 写下的头：跟了重定向的话有好几段（每一跳一段），还可能有 `100 Continue` 那种中间的。取最后一段
pub fn parse_headers(raw: &str) -> Option<(String, u16, String, Vec<(String, String)>)> {
    let mut last = None;
    let mut cur: Option<(String, u16, String, Vec<(String, String)>)> = None;
    for line in raw.split('\n').map(|l| l.trim_end_matches('\r')) {
        if line.starts_with("HTTP/") {
            if let Some(c) = cur.take() {
                last = Some(c);
            }
            let mut parts = line.splitn(3, ' ');
            let version = parts.next()?.to_string();
            let status = parts.next()?.parse().ok()?;
            cur = Some((version, status, parts.next().unwrap_or("").trim().to_string(), Vec::new()));
        } else if let (Some(c), Some((k, v))) = (cur.as_mut(), line.split_once(':')) {
            c.3.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    cur.or(last)
}

/// curl 退出码 → 人话（带上 curl 自己的原话，查问题要它）
fn explain(code: i64, msg: &str) -> String {
    let what = match code {
        6 => "解析不了主机名",
        7 => "连不上（服务没起？端口不对？）",
        28 => "超时了",
        35 | 60 | 77 | 58 | 59 | 80 | 83 | 90 | 91 => "TLS / 证书出问题了",
        47 => "重定向太多（超过 10 次）",
        52 => "服务器没回任何东西就断了",
        56 => "收数据的时候连接断了",
        1 => "协议不支持（只发 http / https）",
        3 => "URL 写得不对",
        26 => "读不了请求体的文件",
        _ => "请求失败",
    };
    if msg.is_empty() {
        format!("{what}（curl 退出码 {code}）")
    } else {
        format!("{what}：{msg}")
    }
}

fn read_small(p: &Path) -> String {
    let mut s = String::new();
    if let Ok(f) = std::fs::File::open(p) {
        let _ = f.take(1 << 20).read_to_string(&mut s);
    }
    s
}

/// 发一个请求。`curl`：curl 在哪（`/usr/bin/curl`）；`dir`：放临时文件和响应体的目录（应用数据目录里的，不往项目里写）；
/// `seq`：这次的序号（临时文件和响应体文件名带着它，不撞）
pub fn send(curl: &Path, r: &Request, dir: &Path, seq: u64, h: &Handle) -> Result<Response, Error> {
    send_capped(curl, r, dir, seq, h, BODY_MAX)
}

/// 同 [`send`]，上限可以注入：测「截断了照实说」不用真造一个 32 MB 的响应
pub fn send_capped(curl: &Path, r: &Request, dir: &Path, seq: u64, h: &Handle, cap: usize) -> Result<Response, Error> {
    validate(r)?;
    if let Some(f) = &r.body_file {
        body_file_inside(&r.root, f)?;
    }
    // 统计写进 `%output{路径}`：curl 读到第一个 `}` 就当路径结束了，带 `}` 的目录会让统计悄悄写丢（测试的临时目录名撞上过）
    if dir.to_string_lossy().contains('}') {
        return Err(Error::Io(format!("放响应的目录名里有「}}」，curl 的统计写不进去：{}", dir.display())));
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("建不了 {}：{e}", dir.display())))?;
    let f = Files::new(dir, seq);
    let res = send_inner(curl, r, dir, seq, h, &f, cap);
    f.remove();
    res
}

fn send_inner(curl: &Path, r: &Request, dir: &Path, seq: u64, h: &Handle, f: &Files, cap: usize) -> Result<Response, Error> {
    if let Some(b) = &r.body {
        write_new(&f.body_in, b)?;
    }
    if h.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut cmd = Command::new(curl);
    cmd.args(curl_args(r, f));
    let t0 = Instant::now();
    let mut run = procutil::spawn(&mut cmd, 64 << 10).map_err(|e| Error::Io(format!("起不了 curl（{}）：{e}", curl.display())))?;
    *h.pid.lock().unwrap_or_else(|e| e.into_inner()) = Some(run.id());
    // 登记 pid 和取消之间有空档：取消先到的话这里补一刀
    if h.is_cancelled() {
        h.cancel();
    }
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut buf = [0u8; 64 << 10];
    loop {
        match run.stdout().read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                total += n;
                if out.len() < cap {
                    out.extend_from_slice(&buf[..n.min(cap - out.len())]);
                }
                if total > cap {
                    run.kill();
                    break;
                }
            }
        }
    }
    // 收尸之前摘掉 pid（见文件头）
    h.pid.lock().unwrap_or_else(|e| e.into_inner()).take();
    let fin = run.finish().map_err(|e| Error::Io(format!("等 curl 退出时出错：{e}")))?;
    let ms = t0.elapsed().as_millis() as u64;
    if h.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let truncated = total > cap;
    // HEAD 用的是 -I：curl 把头也写到了 stdout，那不是响应体
    if r.method == "HEAD" {
        out.clear();
        total = 0;
    }
    let stats: Value = serde_json::from_str(&read_small(&f.stats)).unwrap_or(Value::Null);
    let code = stats["exitcode"].as_i64().or(fin.status.code().map(i64::from)).unwrap_or(-1);
    // 被我们截断而杀掉的：退出码没有意义（rust.md「被掐掉的子进程退出码没有意义」），头已经收到了，照常往下走
    if code != 0 && !truncated {
        let msg = stats["errormsg"].as_str().map(str::to_owned).unwrap_or_else(|| String::from_utf8_lossy(&fin.stderr).trim().trim_start_matches("curl: ").to_string());
        return Err(Error::Curl(explain(code, &msg)));
    }
    let (version, status, reason, headers) = parse_headers(&read_small(&f.headers)).ok_or_else(|| Error::Curl("没收到响应头".into()))?;
    let ctype = headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.to_ascii_lowercase()).unwrap_or_default();
    let looks_json = ctype.contains("json") || matches!(out.iter().find(|b| !b.is_ascii_whitespace()), Some(b'{' | b'['));
    let pretty = if looks_json && !truncated && out.len() <= PRETTY_MAX { json::reindent(&out) } else { None };
    let ext = if looks_json { "json" } else if ctype.contains("html") { "html" } else if ctype.contains("xml") { "xml" } else { "txt" };
    let body = dir.join(format!("resp-{seq}.{ext}"));
    let was_pretty = pretty.is_some();
    write_new(&body, pretty.as_deref().unwrap_or(&out))?;
    Ok(Response {
        version,
        status,
        reason,
        headers,
        ms: stats["time_total"].as_f64().map_or(ms, |s| (s * 1000.0).round() as u64),
        bytes: total as u64,
        body,
        truncated,
        pretty: was_pretty,
        redirects: stats["num_redirects"].as_u64().unwrap_or(0) as u32,
        final_url: stats["url_effective"].as_str().unwrap_or(&r.url).to_string(),
    })
}

/// 写我们自己的文件：名字带序号，正常不会已经存在；万一在（序号从头数了），也是我们上次写的那份，覆盖掉
fn write_new(p: &Path, data: &[u8]) -> Result<(), Error> {
    #[allow(clippy::disallowed_methods)] // 应用数据目录里我们自己的临时文件 / 响应体（名字带序号），不是用户的
    std::fs::write(p, data).map_err(|e| Error::Io(format!("写不了 {}：{e}", p.display())))
}

/// 下一个序号：临时文件和响应体的名字带着它。从「现在的毫秒数」起往上数 —— 重启之后接着往上，[`prune`] 按序号删旧的才对
pub fn next_seq() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let mut cur = NEXT.load(Ordering::SeqCst);
    loop {
        let next = cur.max(now) + 1;
        match NEXT.compare_exchange(cur, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return next,
            Err(x) => cur = x,
        }
    }
}

/// 响应体文件只留最近 `keep` 个（按序号）。只碰 `resp-<数字>.<扩展名>` 这种我们自己起的名字
pub fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut mine: Vec<(u64, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let seq = name.strip_prefix("resp-")?.split('.').next()?.parse::<u64>().ok()?;
            Some((seq, e.path()))
        })
        .collect();
    mine.sort();
    let n = mine.len().saturating_sub(keep);
    for (_, p) in mine.into_iter().take(n) {
        #[allow(clippy::disallowed_methods)] // 我们自己存的响应体（resp-序号.扩展名），范围卡死在这个名字上
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests;
