//! 文件树与文本读写。
//!
//! 与 logengine 同样的纪律：零 Tauri 依赖，可独立测试。
//!
//! 编码策略：**只支持 UTF-8 编辑**。非 UTF-8 的文本（例如老 Java 项目里的 GBK 源码）
//! 会被明确拒绝，而不是用 lossy 解码硬开 —— 那样保存时会把原文件写坏，
//! 是比"打不开"糟糕得多的结果。这类文件可以用日志模式只读查看。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// 列出一层目录。不递归 —— 文件树按需展开，避免大仓库一次性遍历。
///
/// 排序：目录在前，同类按名称不区分大小写排列，与 Finder / IDEA 一致。
pub fn list_dir(dir: impl AsRef<Path>, show_hidden: bool) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for ent in fs::read_dir(dir.as_ref())? {
        let ent = ent?;
        let name = ent.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        // 构建产物目录默认不展示，否则文件树被淹没
        if !show_hidden && matches!(name.as_str(), "node_modules" | "target" | "dist" | "build") {
            continue;
        }
        let meta = match ent.metadata() {
            Ok(m) => m,
            // 断掉的软链等：跳过而不是整个目录失败
            Err(_) => continue,
        };
        out.push(Entry {
            name,
            path: ent.path(),
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// 文件的身份指纹，用来判断"是不是被外部改过"。
///
/// 用 mtime + size 而不是内容 hash：hash 要把整个文件读一遍，
/// 而这个检查在窗口每次获得焦点时都会对所有打开的标签跑一遍。
/// 两个字段一起看，实际使用中足够可靠。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// 修改时间（Unix 毫秒）。取不到时为 0
    pub mtime_ms: u64,
    pub size: u64,
}

pub fn stamp(path: impl AsRef<Path>) -> io::Result<Stamp> {
    let meta = fs::metadata(path.as_ref())?;
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(Stamp {
        mtime_ms,
        size: meta.len(),
    })
}

/// 读取文本文件。非 UTF-8 一律拒绝，理由见模块注释。
pub fn read_text(path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = fs::read(path.as_ref())?;
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "不是 UTF-8 编码的文本，暂不支持编辑（可用日志模式只读查看）",
        )
    })
}

/// 写回文本。先写临时文件再原子替换 —— 中途崩溃不会留下半个文件。
pub fn write_text(path: impl AsRef<Path>, content: &str) -> io::Result<()> {
    let path = path.as_ref();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.lite-ide-tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::write(&tmp, content)?;
    // rename 在同一文件系统内是原子的
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fsservice-test-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 目录在前_同类按名称排序() {
        let d = sandbox("sort");
        fs::write(d.join("beta.txt"), "x").unwrap();
        fs::write(d.join("Alpha.txt"), "x").unwrap();
        fs::create_dir(d.join("zeta")).unwrap();
        fs::create_dir(d.join("Mid")).unwrap();

        let got: Vec<String> = list_dir(&d, false)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(got, vec!["Mid", "zeta", "Alpha.txt", "beta.txt"]);
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 默认隐藏点文件与构建产物() {
        let d = sandbox("hidden");
        fs::write(d.join("visible.rs"), "x").unwrap();
        fs::write(d.join(".env"), "x").unwrap();
        fs::create_dir(d.join("node_modules")).unwrap();
        fs::create_dir(d.join("target")).unwrap();

        let got: Vec<String> = list_dir(&d, false)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(got, vec!["visible.rs"]);

        let all = list_dir(&d, true).unwrap().len();
        assert_eq!(all, 4, "show_hidden 时应全部列出");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 读写往返() {
        let d = sandbox("rw");
        let f = d.join("a.txt");
        write_text(&f, "第一行\n第二行\n").unwrap();
        assert_eq!(read_text(&f).unwrap(), "第一行\n第二行\n");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 保存不留临时文件() {
        let d = sandbox("atomic");
        let f = d.join("a.txt");
        write_text(&f, "content").unwrap();
        let leftovers: Vec<String> = list_dir(&d, true)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .filter(|n| n.contains("lite-ide-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件: {leftovers:?}");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 指纹能认出内容变化() {
        let d = sandbox("stamp");
        let f = d.join("a.txt");
        write_text(&f, "one").unwrap();
        let s1 = stamp(&f).unwrap();

        // 只改内容不改长度，靠 mtime 认出来；睡一下确保时间戳有差异
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_text(&f, "two").unwrap();
        let s2 = stamp(&f).unwrap();
        assert_ne!(s1, s2, "内容变了指纹却没变");

        // 长度变化也要认出来
        write_text(&f, "three-longer").unwrap();
        let s3 = stamp(&f).unwrap();
        assert_ne!(s2.size, s3.size);
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 文件不存在时取指纹报错() {
        let d = sandbox("stamp-missing");
        assert!(stamp(d.join("nope.txt")).is_err());
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 非_utf8_明确拒绝而不是静默损坏() {
        let d = sandbox("gbk");
        let f = d.join("gbk.txt");
        // GBK 编码的「中文」
        fs::write(&f, [0xd6, 0xd0, 0xce, 0xc4]).unwrap();
        let err = read_text(&f).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(d).ok();
    }
}
