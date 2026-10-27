//! 数据库客户端的 Rust 一侧（docs/DATABASE.md）。不依赖 Tauri：
//!
//! ```text
//! 边车    内置的 Java 21 跑 lite-db.jar（scripts/java-bundle.sh 编的），stdin / stdout 一行一条 JSON-RPC —— 线和进程复用 acpsvc（AI 助手的 agent 同一个形状）
//! 驱动    第一次连某种库时从 Maven Central 下钉死的版本、核 sha256（D10 / D11），放应用数据目录；MySQL 的是 GPL，下到用户机器上自用，不进我们的包
//! 密码    钥匙串（系统的 security 命令）：不写任何文件、不过 IPC 回前端
//! ```

use acpsvc::conn::{Conn, Error as ConnError};
use acpsvc::process::Agent;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

/// 一种库的驱动：钉死的版本和 sha256（Maven Central 给的 sha1 核过之后本地算的，2026-10-27）
#[derive(Debug, PartialEq)]
pub struct DriverDef {
    pub kind: &'static str,
    /// 界面上的名字
    pub name: &'static str,
    pub class: &'static str,
    /// Maven Central 上的路径（`groupId 斜杠路径/artifactId/版本/文件名`）
    pub path: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub license: &'static str,
    pub default_port: u16,
}

pub const DRIVERS: &[DriverDef] = &[
    DriverDef {
        kind: "mysql",
        name: "MySQL",
        class: "com.mysql.cj.jdbc.Driver",
        path: "com/mysql/mysql-connector-j/9.1.0/mysql-connector-j-9.1.0.jar",
        sha256: "8776e2ebc46072c9a47ea59d98298c4273bd9f16a7b26b5dfa4744535aa26c62",
        size: 2_597_591,
        license: "GPL-2.0（带 Universal FOSS Exception）",
        default_port: 3306,
    },
    DriverDef {
        kind: "postgresql",
        name: "PostgreSQL",
        class: "org.postgresql.Driver",
        path: "org/postgresql/postgresql/42.7.4/postgresql-42.7.4.jar",
        sha256: "188976721ead8e8627eb6d8389d500dccc0c9bebd885268a3047180274a6031e",
        size: 1_086_687,
        license: "BSD-2-Clause",
        default_port: 5432,
    },
    DriverDef {
        kind: "h2",
        name: "H2",
        class: "org.h2.Driver",
        path: "com/h2database/h2/2.3.232/h2-2.3.232.jar",
        sha256: "8dae62d22db8982c3dcb3826edb9c727c5d302063a67eef7d63d82de401f07d3",
        size: 2_651_157,
        license: "MPL-2.0 / EPL-1.0",
        default_port: 0,
    },
];

pub fn driver(kind: &str) -> Option<&'static DriverDef> {
    DRIVERS.iter().find(|d| d.kind == kind)
}

/// 连接串：MySQL / PostgreSQL 按主机、端口、库拼；H2 和「自己写 URL」的直接用写的那个
pub fn jdbc_url(kind: &str, host: &str, port: u16, database: &str, url: &str) -> String {
    if !url.trim().is_empty() {
        return url.trim().to_string();
    }
    match kind {
        "mysql" => format!("jdbc:mysql://{host}:{port}/{database}"),
        "postgresql" => format!("jdbc:postgresql://{host}:{port}/{database}"),
        _ => String::new(),
    }
}

fn sha256_of(p: &Path) -> Option<String> {
    let mut c = Command::new("/usr/bin/shasum");
    c.arg("-a").arg("256").arg("--").arg(p);
    let o = procutil::run_capped(&mut c, 4096, procutil::ERR_CAP, procutil::Overflow::Kill).ok().filter(|o| o.status.success())?;
    String::from_utf8_lossy(&o.stdout).split_whitespace().next().map(str::to_string)
}

/// 驱动的 jar：`dir` 里有、sha256 对得上就用；没有就从 Maven Central 下（`base` 是仓库地址，测试里换成本机文件），核过才改名进来
pub fn ensure_driver(dir: &Path, d: &DriverDef, base: &str) -> Result<PathBuf, String> {
    let file = dir.join(d.path.rsplit('/').next().unwrap_or(d.path));
    if sha256_of(&file).as_deref() == Some(d.sha256) {
        return Ok(file);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
    let part = file.with_extension("jar.part");
    let mut c = Command::new("/usr/bin/curl");
    c.args(["-fsSL", "--proto", "=https,file", "--max-filesize", "67108864", "-o"]).arg(&part).arg("--").arg(format!("{}/{}", base.trim_end_matches('/'), d.path));
    let o = procutil::run_capped(&mut c, 16 << 10, procutil::ERR_CAP, procutil::Overflow::Kill).map_err(|e| format!("无法启动 curl：{e}"))?;
    if !o.status.success() {
        return Err(format!("下载 {} 驱动失败：{}", d.name, String::from_utf8_lossy(&o.stderr).trim()));
    }
    if sha256_of(&part).as_deref() != Some(d.sha256) {
        #[allow(clippy::disallowed_methods)] // 自己下的、校验没过的临时文件
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} 驱动的校验和不符，未使用（下载可能被篡改或已损坏）", d.name));
    }
    #[allow(clippy::disallowed_methods)] // 自己下的、核过的文件改名进来：目标是驱动目录里自己起的名字，旧的那份刚才校验没过
    std::fs::rename(&part, &file).map_err(|e| format!("无法保存 {} 驱动：{e}", d.name))?;
    Ok(file)
}

/// 钥匙串：一个服务名（正式包和测试身份各用各的）、可选的钥匙串文件（测试通道里用脚本建的临时钥匙串，不碰登录钥匙串）
pub struct Keychain {
    pub service: String,
    pub file: Option<PathBuf>,
}

impl Keychain {
    /// 存密码。走 `security -i`、命令从 stdin 进去：放在命令行参数里的话，存的那一刻 `ps` 看得见密码
    pub fn set(&self, account: &str, password: &str) -> Result<(), String> {
        let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut line = format!("add-generic-password -U -s {} -a {} -w {}", q(&self.service), q(account), q(password));
        if let Some(f) = &self.file {
            line.push(' ');
            line.push_str(&q(&f.to_string_lossy()));
        }
        line.push('\n');
        let mut c = Command::new("/usr/bin/security");
        c.arg("-i").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        #[allow(clippy::disallowed_methods)] // 要往 stdin 写一行命令（密码不进命令行参数）；输出接空 / 只读 stderr 一小段
        let mut child = c.spawn().map_err(|e| format!("无法启动 security：{e}"))?;
        if let Some(mut i) = child.stdin.take() {
            let _ = i.write_all(line.as_bytes());
        }
        let o = child.wait_with_output().map_err(|e| format!("security 执行失败：{e}"))?;
        let err = String::from_utf8_lossy(&o.stderr);
        if !o.status.success() || err.contains("error") {
            return Err(format!("无法将密码存入钥匙串：{}", err.trim()));
        }
        Ok(())
    }

    /// 取密码；没有给 None
    pub fn get(&self, account: &str) -> Option<String> {
        let mut c = Command::new("/usr/bin/security");
        c.args(["find-generic-password", "-s", &self.service, "-a", account, "-w"]);
        if let Some(f) = &self.file {
            c.arg(f);
        }
        let o = procutil::run_capped(&mut c, 64 << 10, procutil::ERR_CAP, procutil::Overflow::Kill).ok().filter(|o| o.status.success())?;
        let s = String::from_utf8_lossy(&o.stdout);
        Some(s.strip_suffix('\n').unwrap_or(&s).to_string())
    }

    pub fn delete(&self, account: &str) {
        let mut c = Command::new("/usr/bin/security");
        c.args(["delete-generic-password", "-s", &self.service, "-a", account]);
        if let Some(f) = &self.file {
            c.arg(f);
        }
        let _ = procutil::run_capped(&mut c, 64 << 10, procutil::ERR_CAP, procutil::Overflow::Kill);
    }
}

/// 边车：一个 JVM，里面按连接 id 攥着 JDBC 连接
pub struct Sidecar {
    agent: Agent,
    conn: Conn,
    exited: std::sync::Arc<Mutex<Option<String>>>,
}

/// 连接（登录、握手）最多等多久；查询不设（靠取消）
pub const CONNECT_WAIT: Duration = Duration::from_secs(15);
const META_WAIT: Duration = Duration::from_secs(60);

impl Sidecar {
    /// 起边车。参数是第 0 步实测定的（docs/DATABASE.md 6.5）：串行 GC + C1 + 256 MB 堆，冷启动 45 ms、连上之后常驻 64 MB
    pub fn start(java: &Path, jar: &Path) -> Result<Sidecar, String> {
        let exited = std::sync::Arc::new(Mutex::new(None));
        let ex = std::sync::Arc::clone(&exited);
        let args: Vec<String> = ["-XX:+UseSerialGC", "-XX:TieredStopAtLevel=1", "-Xmx256m", "-Dfile.encoding=UTF-8", "-jar"].iter().map(|s| s.to_string()).chain([jar.to_string_lossy().into_owned()]).collect();
        let cwd = jar.parent().unwrap_or(Path::new("/"));
        let (agent, pipes) = Agent::spawn(java, &args, cwd, "/usr/bin:/bin", Box::new(move |code| {
            *ex.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("数据库服务进程已退出（退出码 {}）", code.map_or("无".into(), |c| c.to_string())));
        }))
        .map_err(|e| format!("无法启动数据库服务进程：{e}"))?;
        // 边车不反过来问我们，收到的「请求」一律不理
        let conn = Conn::start(pipes.stdout, pipes.stdin, Box::new(|_| {}), Box::new(|_| {})).map_err(|e| format!("无法连接数据库服务进程：{e}"))?;
        Ok(Sidecar { agent, conn, exited })
    }

    pub fn alive(&self) -> bool {
        self.agent.alive() && !self.conn.is_closed()
    }

    fn call(&self, method: &str, params: Value, wait: impl Into<Option<Duration>>) -> Result<Value, String> {
        let wait = wait.into();
        self.conn.request(method, params, wait).map_err(|e| match e {
            ConnError::Rpc { message, .. } => message,
            ConnError::Timeout => format!("数据库服务 {} 秒内未响应", wait.map_or(0, |w| w.as_secs())),
            ConnError::Closed => {
                let why = self.exited.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let tail = self.agent.stderr_tail();
                format!("{}{}", why.unwrap_or_else(|| "数据库服务进程已断开".into()), if tail.trim().is_empty() { String::new() } else { format!("：{}", tail.trim()) })
            }
            ConnError::TooLarge(n) => format!("数据库服务返回的数据过大（{} MB）", n >> 20),
        })
    }

    pub fn ping(&self) -> Result<(), String> {
        self.call("ping", json!({}), Duration::from_secs(10)).map(|_| ())
    }

    /// 打开（或换掉）一个连接，回数据库的名字、版本、当前库 / schema
    pub fn connect(&self, conn: &str, jar: &Path, class: &str, url: &str, user: &str, password: &str) -> Result<Value, String> {
        self.call("connect", json!({ "conn": conn, "jars": [jar], "driver": class, "url": url, "user": user, "password": password }), CONNECT_WAIT)
    }

    /// 表结构树的一层：`what` 是 schemas / tables / columns
    pub fn meta(&self, conn: &str, what: &str, catalog: Option<&str>, schema: Option<&str>, table: Option<&str>) -> Result<Vec<Value>, String> {
        let v = self.call("meta", json!({ "conn": conn, "what": what, "catalog": catalog, "schema": schema, "table": table }), META_WAIT)?;
        Ok(v.as_array().cloned().unwrap_or_default())
    }

    /// 跑一条 SQL：有结果集回 `{kind: "rows", cols, rows, more, cursor, ms}`，没有回 `{kind: "count", count, ms}`。
    /// **不设超时**（docs/DATABASE.md 第 3 节：慢查询本来就要等），停它靠 [`Sidecar::cancel`]
    pub fn query(&self, conn: &str, sql: &str) -> Result<Value, String> {
        self.call("query", json!({ "conn": conn, "sql": sql }), None)
    }

    /// 结果的下一批（界面滚到底时要）。`cursor` 是 query 回的那个：同一连接上又跑了新查询的话，旧的那个已经关了、报错说清楚
    pub fn fetch(&self, conn: &str, cursor: u64) -> Result<Value, String> {
        self.call("fetch", json!({ "conn": conn, "cursor": cursor }), Some(META_WAIT))
    }

    /// 停这个连接上正在跑的语句（JDBC 的 `Statement.cancel()`，从另一条线程发）。跑着的那个 query 随即带着数据库的原话报错回来
    pub fn cancel(&self, conn: &str) {
        let _ = self.call("cancel", json!({ "conn": conn }), Some(Duration::from_secs(5)));
    }

    /// 结果标签关了：开着的游标关掉（PostgreSQL 为分批取开着的事务跟着提交）
    pub fn close_cursor(&self, conn: &str) {
        let _ = self.call("closeCursor", json!({ "conn": conn }), Some(Duration::from_secs(5)));
    }

    pub fn close(&self, conn: &str) {
        let _ = self.call("close", json!({ "conn": conn }), Duration::from_secs(5));
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.agent.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 连接串_按库拼_写了_url_就用写的() {
        assert_eq!(jdbc_url("mysql", "db.local", 3306, "shop", ""), "jdbc:mysql://db.local:3306/shop");
        assert_eq!(jdbc_url("postgresql", "localhost", 5432, "app", ""), "jdbc:postgresql://localhost:5432/app");
        assert_eq!(jdbc_url("mysql", "h", 1, "d", " jdbc:mysql://x/y?useSSL=false "), "jdbc:mysql://x/y?useSSL=false");
        assert_eq!(jdbc_url("h2", "", 0, "", ""), "", "H2 没写 URL 拼不出来");
    }

    #[test]
    fn 驱动表_路径和文件名对得上_sha256_是_64_位() {
        for d in DRIVERS {
            assert!(d.path.ends_with(".jar") && d.path.contains(&d.path.rsplit('/').next().unwrap().replace(".jar", "")), "{}", d.path);
            assert_eq!(d.sha256.len(), 64, "{}", d.kind);
        }
        assert!(driver("mysql").is_some() && driver("oracle").is_none());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
    fn 驱动_下来的校验和不对不用_对的改名进来() {
        let d = std::env::temp_dir().join(format!("dbsvc-drv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let repo = d.join("repo");
        let fake = DriverDef { kind: "x", name: "X", class: "x.D", path: "x/x/1/x-1.jar", sha256: "", size: 0, license: "", default_port: 0 };
        std::fs::create_dir_all(repo.join("x/x/1")).unwrap();
        std::fs::write(repo.join("x/x/1/x-1.jar"), b"jar bytes").unwrap();
        let sum = sha256_of(&repo.join("x/x/1/x-1.jar")).unwrap();
        let base = format!("file://{}", repo.display());
        let bad = DriverDef { sha256: "0000000000000000000000000000000000000000000000000000000000000000", ..fake };
        let e = ensure_driver(&d.join("jdbc"), &bad, &base).unwrap_err();
        assert!(e.contains("校验和不符"), "{e}");
        assert!(!d.join("jdbc/x-1.jar").exists() && !d.join("jdbc/x-1.jar.part").exists(), "校验没过：既不留下也不改名进来");
        let good = DriverDef { sha256: Box::leak(sum.into_boxed_str()), ..bad };
        let p = ensure_driver(&d.join("jdbc"), &good, &base).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"jar bytes");
        // 第二次：在了、对得上，不再下（仓库删了也照样拿得到）
        std::fs::remove_dir_all(&repo).unwrap();
        assert_eq!(ensure_driver(&d.join("jdbc"), &good, &base).unwrap(), p);
        let _ = std::fs::remove_dir_all(&d);
    }
}
