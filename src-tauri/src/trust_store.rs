//! 仓库信任的账本（issue #24）：`<app_data_dir>/trust.json`。
//!
//! 一行一条：仓库根 → 信任时那份 config 的指纹（`gitsvc::trust::fingerprint`，
//! 就是规范化后的 `key=value` 列表本身）。config 一变指纹对不上，信任自动作废。
//!
//! # 为什么在 Rust 侧而不是 localStorage
//!
//! 它是安全决定，不是偏好。localStorage 和会话快照在一个篮子里 —— 配额爆了退一步
//! 丢草稿那套逻辑不该再多一个乘客；而且裸二进制和 `.app` 的 localStorage 不是同一份
//! （frontend.md 那条），信任跟着 WebKit 的存储走会让同一个仓库在两种启动方式下
//! 一个问一个不问。`app_data_dir` 两种启动方式是同一个。
//!
//! # 启动路径上不许抛
//!
//! 文件坏了、不是 JSON、版本对不上：一律当**没信任过任何仓库**。这条和
//! `session.parse` 同一个理由 —— 抛一次应用就打不开，而用户手里没有清它的办法。
//! 最坏的后果只是多问一次。
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    v: u32,
    /// 规范化过的仓库根 → 指纹
    repos: BTreeMap<String, String>,
}

fn load(path: &Path) -> File {
    let Ok(bytes) = std::fs::read(path) else { return File::default() };
    match serde_json::from_slice::<File>(&bytes) {
        Ok(f) if f.v == VERSION => f,
        _ => File::default(),
    }
}

/// 同一个仓库两种写法（软链、`..`）要落到同一条上；规范化失败就用原样
fn norm(root: &str) -> String {
    std::fs::canonicalize(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| root.to_string())
}

pub fn is_trusted(path: &Path, root: &str, fingerprint: &str) -> bool {
    load(path).repos.get(&norm(root)).map(|fp| fp == fingerprint).unwrap_or(false)
}

/// 记下信任。临时文件 + rename：写到一半掉电不能留下半个 JSON（那会被当成「没信任过」，
/// 只是多问一次，但一份写坏的账本没理由留着）
pub fn grant(path: &Path, root: &str, fingerprint: &str) -> std::io::Result<()> {
    let mut f = load(path);
    f.v = VERSION;
    f.repos.insert(norm(root), fingerprint.to_string());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp: PathBuf = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&f).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lite-ide-trust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("trust.json")
    }

    #[test]
    fn 没账本_坏账本_都当没信任过() {
        let p = tmp("bad");
        assert!(!is_trusted(&p, "/x", "fp"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{ not json").unwrap();
        assert!(!is_trusted(&p, "/x", "fp"));
        std::fs::write(&p, br#"{"v":99,"repos":{"/x":"fp"}}"#).unwrap();
        assert!(!is_trusted(&p, "/x", "fp"), "版本对不上也当没有");
    }

    #[test]
    fn 记下之后认_指纹一变不认() {
        let p = tmp("grant");
        grant(&p, "/x", "fp1").unwrap();
        assert!(is_trusted(&p, "/x", "fp1"));
        assert!(!is_trusted(&p, "/x", "fp2"), "config 变了信任作废");
        assert!(!is_trusted(&p, "/y", "fp1"), "别的仓库不沾光");
        grant(&p, "/x", "fp2").unwrap();
        assert!(is_trusted(&p, "/x", "fp2") && !is_trusted(&p, "/x", "fp1"), "重新信任盖掉旧的");
        assert!(!p.with_extension("json.tmp").exists(), "临时文件要收掉");
    }
}
