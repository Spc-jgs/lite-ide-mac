//! 数据库客户端的那一层（docs/DATABASE.md DB-1）：连接配置存哪、边车什么时候起、谁的连接关窗口时收。
//! 边车、驱动、钥匙串在 `dbsvc`。
//!
//! - **配置**按项目一份，存应用数据目录（`db/<项目根的指纹>.json`，同运行配置：不往项目里写）。**没有密码**：密码在钥匙串里，账号是连接 id
//! - **边车**全应用一个，第一次连的时候起（不开数据库工具窗就一个字节不加载）；死了下次用的时候重起
//! - **打开的连接**按窗口登记（`Owned`）：窗口关了、在边车里关掉那几个
//! - **控制台**（DB-2）：每个连接一份 `.sql`，在应用数据目录 `db/console/<连接 id>/<连接名>.sql`，就是普通的编辑器标签（高亮、撤销、本地历史都是现成的）
//! - **历史**（DB-2）：每个连接执行过的 SQL，`db/history/<连接 id>.jsonl` 一行一条，留最近 1000 条

use crate::owned::Owned;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

/// 一个连接的配置（存盘的那份；没有密码）
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Conf {
    pub id: String,
    pub name: String,
    /// mysql / postgresql / h2
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// 自己写的连接串（H2 必填；MySQL / PG 写了就不按主机端口拼）
    pub url: String,
    /// 标成生产库：执行改数据的语句前先确认（DB-3，docs/DATABASE.md 第 4 节）。旧配置里没有这个字段，读出来是 false
    #[serde(default)]
    pub prod: bool,
}

#[derive(Default)]
pub struct Db {
    side: Mutex<Option<Arc<dbsvc::Sidecar>>>,
    /// 打开的连接：键是边车里的连接 id（`窗口 项目 连接`），按窗口登记
    open: Owned<String, ()>,
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    h
}

fn db_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| format!("无法获取应用数据目录：{e}"))?.join("db"))
}

fn conf_file(app: &AppHandle, root: &str) -> Result<PathBuf, String> {
    Ok(db_dir(app)?.join(format!("{:016x}.json", fnv(root))))
}

/// 连接 id 只许是我们自己发的那种（`c` + 十六进制）：它要拼进路径
fn safe_id(id: &str) -> Result<&str, String> {
    if id.len() > 1 && id.starts_with('c') && id[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(id)
    } else {
        Err("连接标识无效".into())
    }
}

/// 这个连接的控制台文件，没有就建一个空的。文件名取连接名（标签上看得出是哪个库）；改过名的连接照旧用原来那份 —— 认目录里已有的 `.sql`
pub fn console(app: &AppHandle, root: &str, id: &str) -> Result<PathBuf, String> {
    let c = list(app, root).into_iter().find(|x| x.id == id).ok_or("连接配置不存在")?;
    let dir = db_dir(app)?.join("console").join(safe_id(id)?);
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
    let mut have: Vec<PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "sql")).collect()).unwrap_or_default();
    have.sort();
    if let Some(p) = have.into_iter().next() {
        return Ok(p);
    }
    let name: String = c.name.chars().map(|ch| if matches!(ch, '/' | ':' | '\\' | '\0') { '_' } else { ch }).collect();
    let p = dir.join(format!("{}.sql", name.trim_start_matches('.')));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
        Ok(_) => Ok(p),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(p),
        Err(e) => Err(format!("无法创建控制台文件：{e}")),
    }
}

/// 历史里的一条
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hist {
    pub sql: String,
    /// 执行的时刻（Unix 毫秒）
    pub at: u64,
    pub ms: u64,
    pub ok: bool,
    /// 更新语句影响的行数；查询是 None
    pub count: Option<i64>,
    /// 出错时数据库的原话（截短）
    pub error: Option<String>,
}

const HISTORY_KEEP: usize = 1000;

fn history_file(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(db_dir(app)?.join("history").join(format!("{}.jsonl", safe_id(id)?)))
}

/// 追加一条；攒到 1200 条时截回最近 1000（不每次都重写整份）
fn record(app: &AppHandle, id: &str, h: &Hist) -> Result<(), String> {
    use std::io::Write;
    let p = history_file(app, id)?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let line = serde_json::to_string(h).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| format!("无法写入查询历史：{e}"))?;
    writeln!(f, "{line}").map_err(|e| format!("无法写入查询历史：{e}"))?;
    drop(f);
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let n = text.lines().count();
    if n > HISTORY_KEEP + HISTORY_KEEP / 5 {
        let keep: Vec<&str> = text.lines().skip(n - HISTORY_KEEP).collect();
        fsservice::write_text(&p, &(keep.join("\n") + "\n")).map_err(|e| format!("无法整理查询历史：{e}"))?;
    }
    Ok(())
}

/// 最近的在前。读坏的行跳过（手改过、写到一半断了电）
pub fn history(app: &AppHandle, id: &str) -> Vec<Hist> {
    let Ok(p) = history_file(app, id) else { return Vec::new() };
    let text = std::fs::read_to_string(p).unwrap_or_default();
    let mut v: Vec<Hist> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.reverse();
    v.truncate(HISTORY_KEEP);
    v
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub fn list(app: &AppHandle, root: &str) -> Vec<Conf> {
    conf_file(app, root).ok().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn store(app: &AppHandle, root: &str, all: &[Conf]) -> Result<(), String> {
    let p = conf_file(app, root)?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("无法创建 {}：{e}", d.display()))?;
    }
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    fsservice::write_text(&p, &text).map_err(|e| format!("无法保存连接配置：{e}"))
}

/// 钥匙串：服务名跟着应用的身份走（测试身份的 .app 不碰正式包存的密码）；测试构建可以指一个脚本建的临时钥匙串（`LITE_DB_KEYCHAIN`），不碰登录钥匙串
pub fn keychain(app: &AppHandle) -> dbsvc::Keychain {
    #[cfg(feature = "test-bridge")]
    let file = std::env::var_os("LITE_DB_KEYCHAIN").map(PathBuf::from);
    #[cfg(not(feature = "test-bridge"))]
    let file = None;
    dbsvc::Keychain { service: format!("{}.db", app.config().identifier), file }
}

/// 驱动从哪下：Maven Central；测试构建可以换成本机目录（`LITE_DB_MAVEN=file:///…`，验收不上网）
fn maven_base() -> String {
    #[cfg(feature = "test-bridge")]
    if let Ok(b) = std::env::var("LITE_DB_MAVEN") {
        return b;
    }
    "https://repo1.maven.org/maven2".into()
}

/// 校验、补默认值。新的给一个 id
pub fn save(app: &AppHandle, root: &str, mut c: Conf, password: Option<&str>) -> Result<Conf, String> {
    c.name = c.name.trim().to_string();
    if c.name.is_empty() {
        return Err("名称不能为空".into());
    }
    let d = dbsvc::driver(&c.kind).ok_or_else(|| format!("不支持的数据库类型：{}", c.kind))?;
    if c.port == 0 {
        c.port = d.default_port;
    }
    if dbsvc::jdbc_url(&c.kind, &c.host, c.port, &c.database, &c.url).is_empty() {
        return Err("请填写连接串（H2 需要完整的 JDBC URL，例如 jdbc:h2:~/data/app）".into());
    }
    let mut all = list(app, root);
    if c.id.is_empty() {
        c.id = format!("c{:x}", fnv(&format!("{root}\n{}\n{:?}", c.name, std::time::SystemTime::now())));
    }
    if all.iter().any(|x| x.id != c.id && x.name == c.name) {
        return Err(format!("已存在名为「{}」的连接", c.name));
    }
    match all.iter_mut().find(|x| x.id == c.id) {
        Some(x) => *x = c.clone(),
        None => all.push(c.clone()),
    }
    store(app, root, &all)?;
    // 密码：Some(非空) 存、Some("") 删、None 不动
    match password {
        Some("") => keychain(app).delete(&c.id),
        Some(pw) => keychain(app).set(&c.id, pw)?,
        None => {}
    }
    Ok(c)
}

pub fn delete(app: &AppHandle, root: &str, id: &str) -> Result<(), String> {
    let mut all = list(app, root);
    all.retain(|x| x.id != id);
    store(app, root, &all)?;
    keychain(app).delete(id);
    Ok(())
}

impl Db {
    /// 边车：没起、或者死了就起一个。开发构建没有内置的 Java 和 lite-db.jar
    fn sidecar(&self, app: &AppHandle) -> Result<Arc<dbsvc::Sidecar>, String> {
        let mut g = self.side.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = g.as_ref().filter(|s| s.alive()) {
            return Ok(Arc::clone(s));
        }
        let (java, jar) = crate::java::db_sidecar(app).ok_or("当前构建未包含数据库服务（开发构建），请使用 pnpm app:bundle 打包的版本")?;
        let s = Arc::new(dbsvc::Sidecar::start(&java, &jar)?);
        // 换了一个新的边车：旧的那些连接都不在了
        drop(self.open.drain());
        *g = Some(Arc::clone(&s));
        Ok(s)
    }

    fn driver_jar(app: &AppHandle, kind: &str) -> Result<(PathBuf, &'static str), String> {
        let d = dbsvc::driver(kind).ok_or_else(|| format!("不支持的数据库类型：{kind}"))?;
        let dir = app.path().app_data_dir().map_err(|e| format!("无法获取应用数据目录：{e}"))?.join("jdbc");
        Ok((dbsvc::ensure_driver(&dir, d, &maven_base())?, d.class))
    }

    fn connect_as(&self, app: &AppHandle, key: &str, c: &Conf, password: &str) -> Result<Value, String> {
        let s = self.sidecar(app)?;
        let (jar, class) = Self::driver_jar(app, &c.kind)?;
        s.connect(key, &jar, class, &dbsvc::jdbc_url(&c.kind, &c.host, c.port, &c.database, &c.url), &c.user, password)
    }

    /// 测试连接：用对话框里此刻填的（密码没填就用钥匙串里存的），连上就关
    pub fn test(&self, app: &AppHandle, c: &Conf, password: Option<&str>) -> Result<Value, String> {
        let pw = match password {
            Some(p) => p.to_string(),
            None => keychain(app).get(&c.id).unwrap_or_default(),
        };
        let key = format!("test {:?}", std::time::SystemTime::now());
        let r = self.connect_as(app, &key, c, &pw);
        if r.is_ok() {
            if let Ok(s) = self.sidecar(app) {
                s.close(&key);
            }
        }
        r
    }

    fn key(owner: &str, root: &str, id: &str) -> String {
        format!("{owner}\n{root}\n{id}")
    }

    pub fn open(&self, app: &AppHandle, owner: &str, root: &str, id: &str) -> Result<Value, String> {
        let c = list(app, root).into_iter().find(|x| x.id == id).ok_or("连接配置不存在")?;
        let key = Self::key(owner, root, id);
        let pw = keychain(app).get(id).unwrap_or_default();
        let r = self.connect_as(app, &key, &c, &pw)?;
        self.open.insert(owner, key, ());
        Ok(r)
    }

    pub fn meta(&self, app: &AppHandle, owner: &str, root: &str, id: &str, what: &str, catalog: Option<&str>, schema: Option<&str>, table: Option<&str>) -> Result<Vec<Value>, String> {
        let key = Self::key(owner, root, id);
        if self.open.get(&key, |_| ()).is_none() {
            self.open(app, owner, root, id)?;
        }
        self.sidecar(app)?.meta(&key, what, catalog, schema, table)
    }

    /// 跑一条 SQL（连接没开的先开），跑完记一条历史。不设超时，停它走 [`Db::cancel`]
    pub fn query(&self, app: &AppHandle, owner: &str, root: &str, id: &str, sql: &str) -> Result<Value, String> {
        let key = Self::key(owner, root, id);
        let t0 = std::time::Instant::now();
        let r = (|| {
            if self.open.get(&key, |_| ()).is_none() {
                self.open(app, owner, root, id)?;
            }
            self.sidecar(app)?.query(&key, sql)
        })();
        let h = Hist {
            sql: sql.to_string(),
            at: now_ms(),
            ms: r.as_ref().ok().and_then(|v| v["ms"].as_u64()).unwrap_or(t0.elapsed().as_millis() as u64),
            ok: r.is_ok(),
            count: r.as_ref().ok().and_then(|v| v["count"].as_i64()),
            error: r.as_ref().err().map(|e| e.chars().take(300).collect()),
        };
        if let Err(e) = record(app, id, &h) {
            applog::write(applog::Level::Warn, "db", &e);
        }
        r
    }

    /// 导出一条查询的结果到 `path`（保存面板选的，确认过覆盖）。连接没开的先开
    pub fn export(&self, app: &AppHandle, owner: &str, root: &str, id: &str, sql: &str, path: &str, format: &str, table: &str) -> Result<Value, String> {
        if !matches!(format, "csv" | "json" | "sql") {
            return Err(format!("不支持的导出格式：{format}"));
        }
        let key = Self::key(owner, root, id);
        if self.open.get(&key, |_| ()).is_none() {
            self.open(app, owner, root, id)?;
        }
        self.sidecar(app)?.export(&key, sql, std::path::Path::new(path), format, table)
    }

    /// 第一行第一格（生产库保护数影响的行数），有时限
    pub fn scalar(&self, app: &AppHandle, owner: &str, root: &str, id: &str, sql: &str, seconds: u32) -> Result<Option<String>, String> {
        let key = Self::key(owner, root, id);
        if self.open.get(&key, |_| ()).is_none() {
            self.open(app, owner, root, id)?;
        }
        self.sidecar(app)?.scalar(&key, sql, seconds)
    }

    pub fn fetch(&self, app: &AppHandle, owner: &str, root: &str, id: &str, cursor: u64) -> Result<Value, String> {
        self.sidecar(app)?.fetch(&Self::key(owner, root, id), cursor)
    }

    /// 停正在跑的那条。边车没起就没有可停的
    pub fn cancel(&self, owner: &str, root: &str, id: &str) {
        let side = self.side.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(s) = side {
            s.cancel(&Self::key(owner, root, id));
        }
    }

    pub fn close_cursor(&self, owner: &str, root: &str, id: &str) {
        let side = self.side.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(s) = side {
            s.close_cursor(&Self::key(owner, root, id));
        }
    }

    pub fn close(&self, owner: &str, root: &str, id: &str) {
        let key = Self::key(owner, root, id);
        if self.open.remove(&key).is_some() {
            if let Some(s) = self.side.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                s.close(&key);
            }
        }
    }

    /// 窗口关了：它打开的连接在边车里关掉（边车自己留着给别的窗口用）
    pub fn release(&self, owner: &str) {
        let keys = self.open.take_owner(owner);
        if keys.is_empty() {
            return;
        }
        let side = self.side.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(s) = side {
            std::thread::spawn(move || {
                for (k, _) in keys {
                    s.close(&k);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 存盘的配置里没有密码这个字段() {
        let c = Conf { id: "c1".into(), name: "订单库".into(), kind: "mysql".into(), host: "h".into(), port: 3306, database: "shop".into(), user: "u".into(), url: String::new(), prod: true };
        let t = serde_json::to_string(&c).unwrap();
        assert!(!t.to_lowercase().contains("password") && !t.contains("pw"), "{t}");
        let back: Conf = serde_json::from_str(&t).unwrap();
        assert_eq!(back, c);
        // DB-1 存下的配置没有 prod 这个字段：照样读得出来，当成不是生产库
        let old: Conf = serde_json::from_str(r#"{"id":"c1","name":"a","kind":"h2","host":"","port":0,"database":"","user":"sa","url":"jdbc:h2:mem:x"}"#).unwrap();
        assert!(!old.prod);
    }

    #[test]
    fn 连接标识要拼进路径_只认自己发的那种() {
        assert!(safe_id("c506d0500e5704c6f").is_ok());
        for bad in ["", "c", "../x", "c12/../../etc", "cg1", "d12"] {
            assert!(safe_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn 项目根的指纹_不同项目不同文件() {
        assert_ne!(fnv("/a"), fnv("/b"));
        assert_eq!(fnv("/a"), fnv("/a"));
    }
}
