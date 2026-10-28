//! 请求历史（#77，IDEA 的 `http-requests-log.http`）：发过的请求按时间留着，能翻回去、能照文件此刻的写法再发一次。
//!
//! **只记「是哪个请求、结果怎样」，不记头和体**：那里面多半是 token、密码、登录用的请求体，IDEA 那份是明文写进项目的 `.idea/httpRequests/`。
//! URL 照记（翻历史时要看得出是哪个请求），但查询参数里像密钥的值换成 `***`（[`mask_url`]）。
//! 存在应用数据目录，按项目一份（文件名是项目路径的指纹，同 Cookie 罐），只留最近 [`KEEP`] 条，不往项目里写。

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// 留多少条：IDEA 留 50 次请求的文件，这里只记一行摘要，多留一些
pub const KEEP: usize = 200;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// 毫秒时间戳
    pub at: u64,
    /// 哪个 `.http` 文件（绝对路径）
    pub file: String,
    /// 请求的名字（`### 名字` / `# @name`）；没有是空串
    pub name: String,
    pub method: String,
    /// 文件里写的 URL（`{{base}}/orders` 那样，没替换）：回到文件里找这个请求用
    pub template: String,
    /// 替换好、发出去的 URL（查询参数里像密钥的已经打码）
    pub url: String,
    /// 用的哪个环境；没选是空串
    pub env: String,
    /// 状态码；没发成是 0
    pub status: u16,
    pub ms: u64,
    /// 没发成的原因（连不上、取消）
    pub error: String,
}

// 一行一条 JSON。手写读写不派生：这个 crate 只依赖 serde_json，为一个结构体不多拉一个 serde
impl Entry {
    fn to_json(&self) -> String {
        serde_json::json!({
            "at": self.at, "file": self.file, "name": self.name, "method": self.method, "template": self.template,
            "url": self.url, "env": self.env, "status": self.status, "ms": self.ms, "error": self.error,
        })
        .to_string()
    }

    fn from_json(line: &str) -> Option<Entry> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        Some(Entry {
            at: v["at"].as_u64()?,
            file: s("file"),
            name: s("name"),
            method: s("method"),
            template: s("template"),
            url: s("url"),
            env: s("env"),
            status: v["status"].as_u64().unwrap_or(0) as u16,
            ms: v["ms"].as_u64().unwrap_or(0),
            error: s("error"),
        })
    }
}

/// 这个项目的历史文件
pub fn path(dir: &Path, root: &str) -> PathBuf {
    dir.join("history").join(format!("{:016x}.jsonl", crate::fnv_pub(root)))
}

/// 查询参数里的值像密钥的换成 `***`：键名里带 token / key / secret / password / pwd / sig / auth / session 的
pub fn mask_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else { return url.to_string() };
    let (query, frag) = match query.split_once('#') {
        Some((q, f)) => (q, Some(f)),
        None => (query, None),
    };
    let masked: Vec<String> = query
        .split('&')
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) if !v.is_empty() && secret_key(k) => format!("{k}=***"),
            _ => kv.to_string(),
        })
        .collect();
    let mut out = format!("{base}?{}", masked.join("&"));
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

fn secret_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    ["token", "key", "secret", "password", "passwd", "pwd", "sig", "auth", "session", "credential"].iter().any(|w| k.contains(w))
}

/// 记一条。URL 在这里打码（调用方给的是发出去的原样）。超过 2 × [`KEEP`] 行时整理成最近的 [`KEEP`] 条 —— 不是每次都重写整个文件
pub fn append(file: &Path, mut e: Entry) -> std::io::Result<()> {
    e.url = mask_url(&e.url);
    // 文件里写的 URL 也打码：写死在 .http 里的 token 不该再抄一份进应用数据目录（状态测试撞的：第一版只打了发出去的那个）
    e.template = mask_url(&e.template);
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    let line = e.to_json() + "\n";
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file)?;
    f.write_all(line.as_bytes())?;
    drop(f);
    let all = read_lines(file);
    if all.len() > 2 * KEEP {
        let keep = all[all.len() - KEEP..].join("\n") + "\n";
        let tmp = file.with_extension("jsonl.tmp");
        #[allow(clippy::disallowed_methods)] // 应用数据目录里自己的历史文件：写临时文件再改名盖过去（名字是我们算的）
        std::fs::write(&tmp, keep)?;
        #[allow(clippy::disallowed_methods)] // 同上，盖的是我们自己的历史文件
        std::fs::rename(&tmp, file)?;
    }
    Ok(())
}

fn read_lines(file: &Path) -> Vec<String> {
    let Ok(f) = std::fs::File::open(file) else { return Vec::new() };
    BufReader::new(f).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).collect()
}

/// 最近的在前，最多 [`KEEP`] 条。读坏了的行跳过（手改过、写到一半断电）
pub fn list(file: &Path) -> Vec<Entry> {
    let mut v: Vec<Entry> = read_lines(file).iter().filter_map(|l| Entry::from_json(l)).collect();
    v.reverse();
    v.truncate(KEEP);
    v
}

/// 清空这个项目的历史（是我们自己的文件，直接删）
pub fn clear(file: &Path) -> std::io::Result<()> {
    #[allow(clippy::disallowed_methods)] // 应用数据目录里自己的历史文件，名字是 path() 算出来的
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 像密钥的查询参数打码_别的原样() {
        assert_eq!(mask_url("http://a/x?page=2&access_token=abc&apiKey=k1&q=s#top"), "http://a/x?page=2&access_token=***&apiKey=***&q=s#top");
        assert_eq!(mask_url("http://a/x?Signature=zz&empty_token="), "http://a/x?Signature=***&empty_token=", "空值不用打码");
        assert_eq!(mask_url("http://a/login"), "http://a/login");
    }

    #[test]
    fn 追加_最近的在前_超过两倍整理成最近的_keep_条_坏行跳过() {
        let d = std::env::temp_dir().join(format!("httpsvc-hist-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let f = path(&d, "/proj");
        assert_ne!(f, path(&d, "/other"), "按项目分开");
        for i in 0..(2 * KEEP + 5) as u64 {
            append(&f, Entry { at: i, method: "GET".into(), url: format!("http://a/{i}?token=t{i}"), ..Default::default() }).unwrap();
        }
        let v = list(&f);
        assert_eq!((v.len(), v[0].at, v[KEEP - 1].at), (KEEP, 2 * KEEP as u64 + 4, KEEP as u64 + 5), "最近的在前、只留 KEEP 条");
        assert!(read_lines(&f).len() <= 2 * KEEP, "文件本身整理过、没有一直长");
        assert!(v.iter().all(|e| e.url.ends_with("token=***")), "写进去之前打码：盘上没有原样的 token");
        assert!(!std::fs::read_to_string(&f).unwrap().contains("token=t1"));
        append(&f, Entry { at: 9_999, template: "{{base}}/x?api_key=k9".into(), url: "http://a/x?api_key=k9".into(), ..Default::default() }).unwrap();
        assert_eq!(list(&f)[0].template, "{{base}}/x?api_key=***", "文件里写的 URL 也打码");
        std::fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"{\"at\":1,\n").unwrap();
        assert_eq!(list(&f).len(), KEEP, "读坏了的行跳过，不让整份读不出来");
        clear(&f).unwrap();
        assert!(list(&f).is_empty() && clear(&f).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }
}
