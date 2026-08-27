//! 文件树与文本读写。
//!
//! 与 logengine 同样的纪律：零 Tauri 依赖，可独立测试。
//!
//! 编码策略见 [`encoding`] 模块：**探测 → 记住 → 原样写回**。
//! 用什么编码读进来的就用什么编码存回去，保存不做「顺手转成 UTF-8」这种擅自决定。

pub mod encoding;

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

/// 读取文本文件并自动探测编码。
///
/// `label` 非空时按指定编码读（用户在状态栏点了「以其他编码重新打开」）。
pub fn read_text_detect(path: impl AsRef<Path>, label: &str) -> io::Result<encoding::Decoded> {
    let bytes = fs::read(path.as_ref())?;
    Ok(if label.is_empty() {
        encoding::decode(&bytes)
    } else {
        encoding::decode_as(&bytes, label)
    })
}

/// 只要内容的便捷版本，给不关心编码的调用方用（测试、内部工具）。
pub fn read_text(path: impl AsRef<Path>) -> io::Result<String> {
    Ok(read_text_detect(path, "")?.content)
}

/// 写回文本，按指定编码。先写临时文件再原子替换 —— 中途崩溃不会留下半个文件。
pub fn write_text_as(
    path: impl AsRef<Path>,
    content: &str,
    label: &str,
    bom: bool,
) -> io::Result<()> {
    let bytes = encoding::encode(content, label, bom);
    write_bytes(path, &bytes)
}

/// UTF-8 无 BOM 的便捷版本
pub fn write_text(path: impl AsRef<Path>, content: &str) -> io::Result<()> {
    write_bytes(path, content.as_bytes())
}

fn write_bytes(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.lite-ide-tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::write(&tmp, bytes)?;
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

    /// 用什么编码读进来，就该用什么编码写回去 —— 保存不该偷偷改变文件的编码
    #[test]
    fn 非utf8文件的读写往返不改变编码() {
        let d = sandbox("enc-roundtrip");
        let f = d.join("gbk.txt");
        let text = "订单处理失败\n重试中\n";

        // 造一个 GBK 文件
        fs::write(&f, encoding::encode(text, "GBK", false)).unwrap();

        let got = read_text_detect(&f, "").unwrap();
        assert_eq!(got.content, text);
        assert!(got.encoding == "GBK" || got.encoding == "gb18030", "探测成了 {}", got.encoding);
        assert!(!got.lossy);

        // 原样写回去，磁盘字节应当和原来一致
        let before = fs::read(&f).unwrap();
        write_text_as(&f, &got.content, got.encoding, got.bom).unwrap();
        assert_eq!(fs::read(&f).unwrap(), before, "保存改变了文件编码");

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
    /// M14 之前这里断言的是「非 UTF-8 一律报 InvalidData」。
    /// 那条策略被换掉了：现在探测编码并如实解码，`lossy` 才是「有损坏」的信号。
    /// 保留这条测试的位置，是为了守住换掉它之后的新契约。
    fn 非utf8不再被拒绝而是探测出编码() {
        let d = sandbox("gbk");
        let f = d.join("gbk.txt");
        // GBK 编码的「中文」
        fs::write(&f, [0xd6, 0xd0, 0xce, 0xc4]).unwrap();

        let got = read_text_detect(&f, "").unwrap();
        assert_ne!(got.encoding, "UTF-8", "不该判成 UTF-8");
        assert!(!got.lossy, "这四个字节是合法 GBK，不该报有损");
        // 短样本上 chardetng 未必能分辨 GBK / Big5 / EUC-KR，
        // 所以只断言「按 GBK 明确读能读对」，不去要求自动探测在 4 字节上也猜准
        assert_eq!(read_text_detect(&f, "GBK").unwrap().content, "中文");

        fs::remove_dir_all(d).ok();
    }
}
