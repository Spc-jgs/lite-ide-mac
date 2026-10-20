//! 本地历史（docs/IDEA.md 第 21 节，IDEA 的 Local History）：每次保存留一份快照，不提交也能回到十分钟前那一版。
//!
//! **什么时候存**：保存时存「存下去的那一版」。存之前先看盘上原来那份和上一次的快照一样不一样 —— 不一样（git 切了分支、别的编辑器改过、
//! 这个文件第一次在这儿保存）就把原来那份也存下来，标成「外部改动 / 改之前」。于是第一次改一个文件之前的样子也留得住，
//! 不用另外去盯文件变化。
//!
//! **存什么**：盘上的**原始字节**（读回来时按文件的编码解码，和打开文件同一条路）。存编辑器里的文字的话，GBK / CRLF 的文件
//! 每次都和盘上那份「不一样」，被误判成外部改动。
//!
//! **存在哪**：调用方给的目录（应用数据目录 `history/`，不往项目里写）。一个文件一个子目录（名字是路径的哈希），里面
//! `path`（原路径，给人查用）、`index`（一行一版：毫秒、内容哈希、字节数、种类）、`<哈希>.blob`（同样的内容只存一份）。
//!
//! **上限**：超过 [`FILE_MAX`] 的不存（日志那种，IDEA 也不存大文件）；每个文件最多 [`KEEP`] 版、[`MAX_AGE_MS`] 以内（照 IDEA 的默认几天放宽到 30 天），
//! 修剪时没人引用的 blob 一起删。
//!
//! **哈希用自己写的 FNV-1a**，不用 std 的 `DefaultHasher`：它的算法不保证跨 Rust 版本稳定，换了工具链之后同样的内容算出另一个名字，去重就失效了；
//! 也不为它加一个哈希库。64 位只用来去重和起文件名，**读回来时核对字节数**，对不上就不认（撞了也不会拿错的内容去恢复）。

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// 比这大的文件不存
pub const FILE_MAX: u64 = 4 << 20;
/// 每个文件最多留几版
pub const KEEP: usize = 200;
/// 留多久
pub const MAX_AGE_MS: u64 = 30 * 24 * 3600 * 1000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// 在这儿保存的
    Saved,
    /// 保存之前盘上那份和上一次的快照不一样：外面改过（或者是这个文件第一次在这儿保存，改之前的样子）
    Before,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub ms: u64,
    pub hash: u64,
    pub size: u64,
    pub kind: Kind,
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn dir_of(store: &Path, path: &str) -> PathBuf {
    store.join(format!("{:016x}", fnv1a(path.as_bytes())))
}

fn parse_index(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let ms = f.next()?.parse().ok()?;
            let hash = u64::from_str_radix(f.next()?, 16).ok()?;
            let size = f.next()?.parse().ok()?;
            let kind = match f.next()? {
                "saved" => Kind::Saved,
                "before" => Kind::Before,
                _ => return None,
            };
            Some(Entry { ms, hash, size, kind })
        })
        .collect()
}

fn line_of(e: &Entry) -> String {
    format!("{}\t{:016x}\t{}\t{}\n", e.ms, e.hash, e.size, if e.kind == Kind::Saved { "saved" } else { "before" })
}

/// 这个文件的全部版本，**最新的在前**
pub fn list(store: &Path, path: &str) -> Vec<Entry> {
    let mut v = parse_index(&fs::read_to_string(dir_of(store, path).join("index")).unwrap_or_default());
    v.reverse();
    v
}

/// 一版的原始字节。字节数对不上（哈希撞了、blob 被动过）就报错，不拿错的内容出去
pub fn read(store: &Path, path: &str, hash: u64) -> io::Result<Vec<u8>> {
    let d = dir_of(store, path);
    let e = list(store, path).into_iter().find(|e| e.hash == hash).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "没有这一版了（可能超过 30 天被清掉了）"))?;
    let mut buf = Vec::new();
    fs::File::open(d.join(format!("{hash:016x}.blob")))?.take(FILE_MAX + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 != e.size {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "这一版的内容对不上（存的文件被动过），没拿"));
    }
    Ok(buf)
}

/// 写我们自己的文件（blob、index）：名字是我们起的，在我们自己的目录里
fn put(p: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = p.with_extension("tmp");
    #[allow(clippy::disallowed_methods)] // 本地历史自己的临时文件（`<名字>.tmp`），写完改名盖到自己的文件上，不是用户的
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, p)
    }
}

/// 保存之后记一笔。`old`：保存之前盘上那份（新文件给 None）；`new`：存下去的那份（读回来的盘上字节）。
/// 和最新那版一样的不重复记（连按 ⌘S）。返回记了几版（0 / 1 / 2）
pub fn record(store: &Path, path: &str, old: Option<&[u8]>, new: &[u8], now_ms: u64) -> io::Result<usize> {
    if new.len() as u64 > FILE_MAX && old.is_none_or(|o| o.len() as u64 > FILE_MAX) {
        return Ok(0);
    }
    let d = dir_of(store, path);
    fs::create_dir_all(&d)?;
    let path_file = d.join("path");
    if !path_file.exists() {
        put(&path_file, path.as_bytes())?;
    }
    let mut entries = parse_index(&fs::read_to_string(d.join("index")).unwrap_or_default());
    let mut added = 0;
    let mut add = |entries: &mut Vec<Entry>, bytes: &[u8], kind: Kind| -> io::Result<()> {
        if bytes.len() as u64 > FILE_MAX {
            return Ok(());
        }
        let hash = fnv1a(bytes);
        if entries.last().is_some_and(|e| e.hash == hash && e.size == bytes.len() as u64) {
            return Ok(());
        }
        let blob = d.join(format!("{hash:016x}.blob"));
        if !blob.exists() {
            put(&blob, bytes)?;
        }
        entries.push(Entry { ms: now_ms, hash, size: bytes.len() as u64, kind });
        added += 1;
        Ok(())
    };
    if let Some(o) = old {
        add(&mut entries, o, Kind::Before)?;
    }
    add(&mut entries, new, Kind::Saved)?;
    prune(&d, &mut entries, now_ms)?;
    let text: String = entries.iter().map(line_of).collect();
    put(&d.join("index"), text.as_bytes())?;
    Ok(added)
}

/// 超过 [`KEEP`] 版、超过 [`MAX_AGE_MS`] 的扔掉；没人引用的 blob 删掉。只在刚记完一版时跑，所以刚记的那版（就是盘上现在的样子）总在
fn prune(d: &Path, entries: &mut Vec<Entry>, now_ms: u64) -> io::Result<()> {
    let keep_from = entries.len().saturating_sub(KEEP);
    let mut i = 0;
    entries.retain(|e| {
        let ok = i >= keep_from && now_ms.saturating_sub(e.ms) <= MAX_AGE_MS;
        i += 1;
        ok
    });
    let live: std::collections::HashSet<String> = entries.iter().map(|e| format!("{:016x}.blob", e.hash)).collect();
    for f in fs::read_dir(d)?.flatten() {
        let name = f.file_name().to_string_lossy().into_owned();
        if name.ends_with(".blob") && !live.contains(&name) {
            #[allow(clippy::disallowed_methods)] // 本地历史自己的 blob（`<哈希>.blob`），没人引用了；范围卡死在这个后缀上
            let _ = fs::remove_file(f.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("histsvc-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    const DAY: u64 = 24 * 3600 * 1000;

    #[test]
    fn 第一次保存连改之前的一起记_连按保存不重复_外面改过的补记一版() {
        let s = tmp("record");
        let p = "/proj/A.java";
        assert_eq!(record(&s, p, Some(b"v0"), b"v1", 1000).unwrap(), 2, "第一次在这儿保存：改之前的样子也记下来");
        assert_eq!(record(&s, p, Some(b"v1"), b"v1", 2000).unwrap(), 0, "连按 ⌘S：内容没变，不记");
        assert_eq!(record(&s, p, Some(b"v1"), b"v2", 3000).unwrap(), 1, "盘上那份就是上一版：只记新的");
        // 外面改成了 vX（git 切分支），再在这儿保存成 v3：vX 补记一版
        assert_eq!(record(&s, p, Some(b"vX"), b"v3", 4000).unwrap(), 2);
        let l = list(&s, p);
        assert_eq!(l.iter().map(|e| (e.ms, e.kind)).collect::<Vec<_>>(), vec![(4000, Kind::Saved), (4000, Kind::Before), (3000, Kind::Saved), (1000, Kind::Saved), (1000, Kind::Before)], "最新的在前");
        assert_eq!(read(&s, p, l[1].hash).unwrap(), b"vX");
        assert_eq!(read(&s, p, l[4].hash).unwrap(), b"v0");
        assert!(list(&s, "/proj/B.java").is_empty(), "别的文件各是各的");
        let _ = fs::remove_dir_all(&s);
    }

    #[test]
    fn 同样的内容只存一份_存的是原始字节() {
        let s = tmp("dedupe");
        let p = "/proj/gbk.txt";
        let gbk = [0xd6u8, 0xd0, 0xce, 0xc4, b'\r', b'\n']; // 「中文」GBK + CRLF
        record(&s, p, None, &gbk, 1000).unwrap();
        record(&s, p, Some(&gbk), b"x", 2000).unwrap();
        record(&s, p, Some(b"x"), &gbk, 3000).unwrap();
        let d = dir_of(&s, p);
        let blobs = fs::read_dir(&d).unwrap().flatten().filter(|f| f.file_name().to_string_lossy().ends_with(".blob")).count();
        assert_eq!((list(&s, p).len(), blobs), (3, 2), "三版、两份内容");
        assert_eq!(read(&s, p, fnv1a(&gbk)).unwrap(), gbk, "原样的字节（编码、换行符都不动）");
        let _ = fs::remove_dir_all(&s);
    }

    #[test]
    fn 修剪_超过上限和太老的扔掉_没人引用的_blob_删掉() {
        let s = tmp("prune");
        let p = "/proj/C.java";
        let mut prev: Option<Vec<u8>> = None;
        for i in 0..(KEEP + 5) {
            let b = format!("v{i}").into_bytes();
            record(&s, p, prev.as_deref(), &b, 1000 + i as u64).unwrap();
            prev = Some(b);
        }
        let l = list(&s, p);
        assert_eq!(l.len(), KEEP);
        assert_eq!(read(&s, p, l[0].hash).unwrap(), format!("v{}", KEEP + 4).into_bytes());
        assert!(read(&s, p, fnv1a(b"v0")).is_err(), "最早那几版被扔了");
        assert!(!dir_of(&s, p).join(format!("{:016x}.blob", fnv1a(b"v0"))).exists(), "它的 blob 也删了");
        // 40 天之后再存：老的全扔，只剩刚存的
        let last = prev.unwrap();
        record(&s, p, Some(&last), b"new", 1000 + 40 * DAY).unwrap();
        let l = list(&s, p);
        assert_eq!(l.len(), 1, "40 天前的照扔：{l:?}");
        let _ = fs::remove_dir_all(&s);
    }

    #[test]
    fn 太大的不存_字节数对不上的不认() {
        let s = tmp("big");
        let big = vec![b'x'; FILE_MAX as usize + 1];
        assert_eq!(record(&s, "/proj/app.log", None, &big, 1).unwrap(), 0);
        assert!(list(&s, "/proj/app.log").is_empty());
        let p = "/proj/D.java";
        record(&s, p, None, b"abc", 1).unwrap();
        let blob = dir_of(&s, p).join(format!("{:016x}.blob", fnv1a(b"abc")));
        #[allow(clippy::disallowed_methods)] // 测试：把存的内容改坏
        fs::write(&blob, b"abcd").unwrap();
        assert!(read(&s, p, fnv1a(b"abc")).is_err(), "内容被动过就不拿出去恢复");
        let _ = fs::remove_dir_all(&s);
    }
}
