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
//!
//! Cookie 罐（#75，同 IDEA）：按「项目 + 环境」一个文件（[`jar_path`]），交给 curl 自己的 `-b` / `-c` 读写（Netscape 格式，过期的 curl 自己丢）。
//! 里面是登录态：目录 0700、文件 0600，不进日志；界面上只列域名、名字、过期时间（[`cookies`]），不给值。

pub mod env;
pub mod history;
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
    /// multipart（#77）：请求体由文字段和文件按次序拼成（文字段里的换行前端已经换成 CRLF）。每个文件同 `body_file`，只许在项目里
    pub parts: Option<Vec<Part>>,
    /// 项目根：`body_file` 只许在它底下
    pub root: PathBuf,
    /// 跟着重定向走（IDEA 默认跟；`# @no-redirect` 不跟）
    pub follow: bool,
    /// Cookie 罐（[`jar_path`]）；`# @no-cookie-jar` 的请求是 None：不带、也不记
    pub cookie_jar: Option<PathBuf>,
}

/// multipart 请求体的一段
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(Vec<u8>),
    /// 整个文件的字节原样拼进去（图片、压缩包都行）
    File(PathBuf),
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
        return Err(Error::Invalid(format!("方法「{}」无效", r.method)));
    }
    let lower = r.url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(Error::Invalid(format!("仅支持 http:// 和 https:// 请求：{}", r.url)));
    }
    if r.url.chars().any(|c| c.is_control() || c == ' ') {
        return Err(Error::Invalid(format!("URL 包含空格或控制字符（变量值可能含换行）：{}", r.url.escape_debug())));
    }
    for (k, v) in &r.headers {
        if !token(k) {
            return Err(Error::Invalid(format!("请求头名称「{k}」无效")));
        }
        if v.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
            return Err(Error::Invalid(format!("请求头「{k}」的值包含换行（变量值可能含换行）")));
        }
    }
    if [r.body.is_some(), r.body_file.is_some(), r.parts.is_some()].iter().filter(|x| **x).count() > 1 {
        return Err(Error::Invalid("请求体不能同时内联书写并从文件读取".into()));
    }
    Ok(())
}

/// `< 文件` 只许是项目里的文件（解开软链之后）。IDEA 不拦这个；我们拦，理由见文件头：`.http` 和它旁边的软链都是项目带来的，
/// 别人仓库里的一个请求不该能把你机器上项目外面的文件发出去。文件不存在不在这儿拦 —— 交给 curl 报「读不了请求体的文件」（退出码 26）
/// 项目根解析不出来就不发（加固出错往「不做」那边倒，rust.md）
pub fn body_file_inside(root: &Path, file: &Path) -> Result<(), Error> {
    let Ok(f) = file.canonicalize() else { return Ok(()) };
    let r = root.canonicalize().map_err(|e| Error::Invalid(format!("无法确定项目根目录（{}：{e}），未发送引用文件的请求", root.display())))?;
    if f.starts_with(&r) {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "请求体文件位于项目之外（{}），未发送：.http 文件随项目分发，读取项目之外的文件可能泄露私钥或配置。如需发送，请将文件移入项目",
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
    let body = (r.body.is_some() || r.parts.is_some()).then(|| f.body_in.clone()).or_else(|| r.body_file.clone());
    match r.method.as_str() {
        // HEAD 要用 -I：`-X HEAD` 会让 curl 等一个永远不来的响应体
        "HEAD" => a.push("-I".into()),
        // GET 不带 -X；带请求体的 POST 也不带（--data-binary 本来就是 POST），这样 301/302/303 时 curl 像浏览器一样改成 GET
        "GET" => {}
        "POST" if body.is_some() => {}
        m => a.extend(["-X".to_string(), m.to_string()]),
    }
    if let Some(j) = &r.cookie_jar {
        a.extend(["-b".to_string(), j.display().to_string(), "-c".to_string(), j.display().to_string()]);
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
        6 => "无法解析主机名",
        7 => "无法连接（服务未启动或端口错误）",
        28 => "请求超时",
        35 | 60 | 77 | 58 | 59 | 80 | 83 | 90 | 91 => "TLS / 证书错误",
        47 => "重定向次数过多（超过 10 次）",
        52 => "服务器未返回任何数据即断开连接",
        56 => "接收数据时连接中断",
        1 => "协议不受支持（仅支持 http / https）",
        3 => "URL 格式错误",
        26 => "无法读取请求体文件",
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
    for p in r.parts.iter().flatten() {
        if let Part::File(f) = p {
            body_file_inside(&r.root, f)?;
        }
    }
    // 统计写进 `%output{路径}`：curl 读到第一个 `}` 就当路径结束了，带 `}` 的目录会让统计悄悄写丢（测试的临时目录名撞上过）
    if dir.to_string_lossy().contains('}') {
        return Err(Error::Io(format!("响应目录名包含「}}」，curl 无法写入统计信息：{}", dir.display())));
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("无法创建 {}：{e}", dir.display())))?;
    if let Some(j) = &r.cookie_jar {
        prepare_jar(j)?;
    }
    let f = Files::new(dir, seq);
    let res = send_inner(curl, r, dir, seq, h, &f, cap);
    f.remove();
    if let Some(j) = &r.cookie_jar {
        // 兜底。2026-10-28 拿系统的 curl 8.7.1 实测：罐子已在、是 0600 时，curl 写新文件再改名（inode 变了）、沿用原来的 0600；
        // 罐子不在、由 curl 新建时是 0644（跟 umask）—— 所以真正管用的是 prepare_jar 先建一个 0600 的空文件，这一句防的是别的版本的行为
        let _ = set_mode(j, 0o600);
    }
    res
}

/// 这个项目、这个环境的 Cookie 罐。文件名是两段 FNV 指纹：项目路径和环境名（可能是中文）都不直接进文件名
pub fn jar_path(dir: &Path, root: &str, env: Option<&str>) -> PathBuf {
    dir.join("cookies").join(format!("{:016x}-{:016x}.txt", fnv(root), fnv(env.unwrap_or(""))))
}

/// 同 [`fnv`]，给别的模块用（请求历史的文件名）
pub(crate) fn fnv_pub(s: &str) -> u64 {
    fnv(s)
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

fn set_mode(p: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
}

/// 罐子的目录 0700、文件 0600。没有就先建一个 0600 的空文件：由 curl 的 `-c` 新建的话是 0644（跟 umask，实测），别的用户读得到登录态
fn prepare_jar(j: &Path) -> Result<(), Error> {
    use std::os::unix::fs::OpenOptionsExt;
    let io = |e: std::io::Error| Error::Io(format!("无法准备 Cookie 文件 {}：{e}", j.display()));
    if let Some(d) = j.parent() {
        std::fs::create_dir_all(d).map_err(io)?;
        set_mode(d, 0o700).map_err(io)?;
    }
    if !j.exists() {
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(j).map_err(io)?;
    }
    set_mode(j, 0o600).map_err(io)
}

/// 罐子里的一条（不带值：值是登录态）
#[derive(Clone, Debug, PartialEq)]
pub struct Cookie {
    pub domain: String,
    pub path: String,
    pub name: String,
    /// Unix 秒；0 = 会话 Cookie（浏览器关掉就没，在这里是「清除前一直在」）
    pub expires: u64,
    pub http_only: bool,
}

/// 读罐子（Netscape 格式：七列、Tab 分开；`#HttpOnly_` 开头的是 HttpOnly 的那种，别的 `#` 开头是注释）
pub fn cookies(jar: &Path) -> Vec<Cookie> {
    read_small(jar)
        .lines()
        .filter_map(|l| {
            let (http_only, l) = match l.strip_prefix("#HttpOnly_") {
                Some(rest) => (true, rest),
                None if l.starts_with('#') => return None,
                None => (false, l),
            };
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 7).then(|| Cookie { domain: c[0].to_string(), path: c[2].to_string(), name: c[5].to_string(), expires: c[4].parse().unwrap_or(0), http_only })
        })
        .collect()
}

/// 清空罐子。是我们自己的文件（名字是 [`jar_path`] 算出来的），直接删
pub fn clear_cookies(jar: &Path) -> Result<(), Error> {
    #[allow(clippy::disallowed_methods)] // 应用数据目录里我们自己的 Cookie 罐，范围卡死在 jar_path 算出来的名字上
    match std::fs::remove_file(jar) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Io(format!("无法清除 Cookie：{e}"))),
        _ => Ok(()),
    }
}

fn send_inner(curl: &Path, r: &Request, dir: &Path, seq: u64, h: &Handle, f: &Files, cap: usize) -> Result<Response, Error> {
    if let Some(b) = &r.body {
        write_new(&f.body_in, b)?;
    }
    if let Some(parts) = &r.parts {
        write_parts(&f.body_in, parts)?;
    }
    if h.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut cmd = Command::new(curl);
    cmd.args(curl_args(r, f));
    let t0 = Instant::now();
    let mut run = procutil::spawn(&mut cmd, 64 << 10).map_err(|e| Error::Io(format!("无法启动 curl（{}）：{e}", curl.display())))?;
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
    let fin = run.finish().map_err(|e| Error::Io(format!("等待 curl 退出时出错：{e}")))?;
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
    let (version, status, reason, headers) = parse_headers(&read_small(&f.headers)).ok_or_else(|| Error::Curl("未收到响应头".into()))?;
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

/// 把 multipart 的几段按次序写进请求体文件：文件是流式拷进去的（几百 MB 的上传包不进内存）。
/// 文件不在 / 读不了在这儿就报，说是哪个文件 —— 交给 curl 的话只剩一句「读不了请求体的文件」，看不出是第几个
fn write_parts(p: &Path, parts: &[Part]) -> Result<(), Error> {
    use std::io::Write;
    #[allow(clippy::disallowed_methods)] // 应用数据目录里我们自己的临时请求体（名字带序号），同 write_new
    let mut out = std::io::BufWriter::new(std::fs::File::create(p).map_err(|e| Error::Io(format!("无法写入 {}：{e}", p.display())))?);
    let io = |e: std::io::Error| Error::Io(format!("无法写入 {}：{e}", p.display()));
    for part in parts {
        match part {
            Part::Text(t) => out.write_all(t).map_err(io)?,
            Part::File(f) => {
                let mut src = std::fs::File::open(f).map_err(|e| Error::Invalid(format!("无法读取请求体中引用的文件 {}：{e}", f.display())))?;
                std::io::copy(&mut src, &mut out).map_err(io)?;
            }
        }
    }
    out.flush().map_err(io)
}

/// 写我们自己的文件：名字带序号，正常不会已经存在；万一在（序号从头数了），也是我们上次写的那份，覆盖掉
fn write_new(p: &Path, data: &[u8]) -> Result<(), Error> {
    #[allow(clippy::disallowed_methods)] // 应用数据目录里我们自己的临时文件 / 响应体（名字带序号），不是用户的
    std::fs::write(p, data).map_err(|e| Error::Io(format!("无法写入 {}：{e}", p.display())))
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
