//! 替换日志（docs/REPLACE.md 12.1）：**动任何一个文件之前**，先把「要改哪些、改前是什么」落盘。
//!
//! ```text
//! <目录>/manifest.json   状态、根目录、时间、每个文件一条
//! <目录>/before/0001     每个文件改前的原样字节（不是解码后的文本 —— 撤销要一个字节不差地写回去）
//! ```
//!
//! 有了它，三件事才做得到：应用崩了 / ⌘Q 之后还能撤销；提交到一半崩了，下次启动知道哪些改了、怎么退回去；
//! 准备到一半崩了，知道去哪儿收拾临时文件。只留最近一次。
//!
//! 「改后」只存指纹不存全文：撤销时要问的只是「此刻还是不是改后那份」，存全文体积翻倍而用不上。

use crate::fp::Fp;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 格式改了就加一。读到不认识的版本当没有日志（不猜）
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    /// 正在写临时文件，原文件一个都还没动。崩在这儿：收掉临时文件、丢掉日志
    Preparing,
    /// 正在逐个换上去。崩在这儿：下次启动问人「退回去 / 保留现状」
    Committing,
    /// 做完了。这时候日志的用处只剩撤销
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// 改在盘上（关着的文件、开着但没有未保存改动的文件）
    Disk,
    /// 只改在编辑器里（开着、有未保存改动的文件）—— 盘上那份没动过
    Editor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// 相对项目根，给人看
    pub rel: String,
    pub kind: Kind,
    /// 真正被写的那个文件（软链已解开）；`Editor` 类是编辑器里那个标签的路径
    pub target: String,
    /// `before/` 底下的文件名
    pub before: String,
    /// `Disk`：改前 / 改后的原样字节的指纹；`Editor`：改后文本的指纹（`after` 与 `after_text` 相同）
    pub after: Fp,
    /// 改后的文本（统一成 `\n` 之后）的指纹 —— 撤销时比的是编辑器里的文本，编辑器里没有字节
    pub after_text: Fp,
    /// 写回去用的编码 / BOM / 换行符（`Editor` 类撤销时如果标签关了、要写盘，用盘上那份的）
    pub encoding: String,
    pub bom: bool,
    pub eol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub state: State,
    pub root: String,
    /// 毫秒时间戳，卡片上显示「几分钟前」
    pub at_ms: u64,
    pub entries: Vec<Entry>,
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.json")
}

/// 开一份新日志：清掉上一份（**调用方先确认它是 Done 或者没有**），写好全部改前字节，再写 manifest。
/// 改前的字节先落盘、manifest 最后写：manifest 在 = 它说到的东西都在
pub fn create(dir: &Path, m: &Manifest, befores: &[(String, Vec<u8>)]) -> io::Result<()> {
    remove(dir)?;
    let bdir = dir.join("before");
    fs::create_dir_all(&bdir)?;
    for (name, bytes) in befores {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(bdir.join(name))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    sync_dir(&bdir);
    save(dir, m)
}

/// 换状态。整份 manifest 原子地替换（fsservice 的临时文件 + rename），崩在中间也是要么旧要么新
pub fn save(dir: &Path, m: &Manifest) -> io::Result<()> {
    let text = serde_json::to_string_pretty(m).map_err(io::Error::other)?;
    fsservice::write_text(manifest_path(dir), &text)?;
    sync_dir(dir);
    Ok(())
}

/// 读日志。没有、读不懂、版本不认识，都当没有
pub fn load(dir: &Path) -> Option<Manifest> {
    let text = fs::read_to_string(manifest_path(dir)).ok()?;
    let m: Manifest = serde_json::from_str(&text).ok()?;
    (m.version == VERSION).then_some(m)
}

pub fn read_before(dir: &Path, e: &Entry) -> io::Result<Vec<u8>> {
    fs::read(dir.join("before").join(&e.before))
}

/// 整个日志目录删掉。目录不在不算错
pub fn remove(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// 目录项落盘：新建的文件、rename 过来的 manifest，掉电后都还在
fn sync_dir(dir: &Path) {
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
}
