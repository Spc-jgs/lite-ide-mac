//! 跨文件查找替换（issue #42）。设计、调研、实测数字都在 docs/REPLACE.md，这里只写「为什么代码长这样」。
//!
//! 四个入口：
//! - [`scan`]：找出全部命中（只读）。4 线程读文件 —— 5 万个小文件上比先问 rg 快（第 0 步实测：瓶颈在开文件读文件）
//! - [`Scan::after`]：替换串变了只重算「改后」那一行，不重新扫盘（一次完整扫描约 0.45s，每敲一个字都扫不行）
//! - [`apply`]：**重读 → 核对指纹 → 写日志 → 准备全部临时文件 → 逐个换上**（两段提交，12.2）
//! - [`undo`] / [`recover`]：撤销最近一次；下次启动时收拾上次中断的那一次
//!
//! 匹配只问 `searchsvc::Matcher`：预览给你看的和真正改掉的是同一个函数算的。

mod fp;
pub mod journal;

pub use fp::Fp;
pub use journal::State;
pub use searchsvc::Query;

use fsservice::eol::Eol;
use journal::{Entry, Kind, Manifest};
use searchsvc::Matcher;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// 命中超过这么多，预览照常列、**不许执行**（替换一半比不替换糟，docs/REPLACE.md 第 7 节）
pub const MAX_HITS: usize = 5_000;
/// 单个文件的上限，和搜索一致
pub const MAX_FILE_BYTES: u64 = 8 << 20;
/// 替换日志里改前全文的上限。超了要用户点头「这次撤销不了」才做
pub const JOURNAL_CAP: u64 = 64 << 20;
/// 扫描的线程数。第 0 步实测 4 最快：8 线程抢文件系统，反而慢
const THREADS: usize = 4;
/// 每一处命中带回去的那一行最多多少个字符（同搜索）
const LINE_CLIP: usize = 400;

/// 这份内容从哪来
#[derive(Debug, Clone)]
pub enum Source {
    /// 读盘。写回去时用同一套编码 / BOM / 换行符
    Disk { encoding: &'static str, bom: bool, eol: Eol },
    /// 开着、有未保存改动的标签：预览和执行都用编辑器里那份（用户看到的就是被改的），不写盘
    Editor,
}

#[derive(Debug, Clone)]
pub struct Hit {
    /// 在 `ScanFile::text`（换行统一成 `\n` 之后）里的字节区间
    pub start: usize,
    pub end: usize,
    pub line: u64,
    /// UTF-16，从 1 起（编辑器的列）
    pub col: u32,
    /// 这一行（截到 400 字），和这一处在行里的 UTF-16 区间
    pub text: String,
    pub spans: Vec<[u32; 2]>,
    /// 跨了几行（1 = 不跨行）。跨行的命中在列表里仍然只占一行，标「跨 N 行」（docs/REPLACE.md 12.3）
    pub lines: u32,
    /// 跨行时：改前那几行（整行，超过 200 行中间折叠），点开「跨 N 行」看的就是它
    pub block: Option<String>,
}

/// 跨行命中的预览片段：超过 [`FOLD_OVER`] 行只留头尾各 [`FOLD_KEEP`] 行 —— 一个几千行的命中把列表撑满没人看得完，
/// 执行不受影响（改的是整个命中）
const FOLD_OVER: usize = 200;
const FOLD_KEEP: usize = 20;

fn fold(block: &str) -> String {
    let lines: Vec<&str> = block.split('\n').collect();
    if lines.len() <= FOLD_OVER {
        return block.to_string();
    }
    let hidden = lines.len() - FOLD_KEEP * 2;
    let mut out: Vec<String> = lines[..FOLD_KEEP].iter().map(|l| l.to_string()).collect();
    out.push(format!("…（中间 {hidden} 行没显示，替换照样改）…"));
    out.extend(lines[lines.len() - FOLD_KEEP..].iter().map(|l| l.to_string()));
    out.join("\n")
}

/// 一处命中占的整行范围：从起头那一行的行首，到结尾那一行的行尾（跨行的命中跨几行就是几行）
fn block_bounds(text: &str, s: usize, e: usize) -> (usize, usize) {
    let (ls, le) = line_bounds(text, s);
    (ls, if e > le { line_bounds(text, e).1 } else { le })
}

#[derive(Debug, Clone)]
pub struct ScanFile {
    pub rel: String,
    pub abs: PathBuf,
    pub source: Source,
    pub hits: Vec<Hit>,
    text: String,
    /// `text` 的指纹（`Editor` 类核对用）
    text_fp: Fp,
    /// 盘上原样字节的指纹（`Disk` 类核对用：比字节，不比解码后的文本）
    raw_fp: Option<Fp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// 解码有损：带着它写回去会把解不出的字节永久换成 U+FFFD
    Lossy,
    /// 换行符混用（CRLF 和 LF 都有）：写回去会统一成 LF，批量替换里悄悄改掉换行符正是别的编辑器被骂的那个坑
    MixedEol,
    /// 超过 8MB
    TooBig,
    /// 和另一条路径是同一个文件（软链、硬链接）：只替换一次
    Same { as_rel: String },
    /// 预览之后被改过（盘上，或者编辑器里）—— 不在一份没给人看过的内容上动手
    Changed,
    /// 撤销时：替换之后又改过，不盲写
    EditedSince,
    /// 撤销时：替换只进了编辑器、没存就关了标签 —— 盘上那份本来就没被替换过
    NeverSaved,
    Unreadable(String),
}

impl Why {
    /// 给前端分类用的短代码
    pub fn code(&self) -> &'static str {
        match self {
            Why::Lossy => "lossy",
            Why::MixedEol => "mixed-eol",
            Why::TooBig => "too-big",
            Why::Same { .. } => "same",
            Why::Changed => "changed",
            Why::EditedSince => "edited-since",
            Why::NeverSaved => "never-saved",
            Why::Unreadable(_) => "unreadable",
        }
    }

    /// 给人看的一句话，界面原样显示（文案只在这儿写一份）
    pub fn text(&self) -> String {
        match self {
            Why::Lossy => "有这个编码解不出来的字节，写回去会把它们永久换成 �".into(),
            Why::MixedEol => "换行符混用（CRLF 和 LF 都有），写回去会被统一成 LF".into(),
            Why::TooBig => "超过 8 MB，没有看".into(),
            Why::Same { as_rel } => format!("和 {as_rel} 是同一个文件，只替换一次"),
            Why::Changed => "预览之后被改过，没动它".into(),
            Why::EditedSince => "替换之后又改过，没动它".into(),
            Why::NeverSaved => "替换只进了编辑器、没存就关了，盘上那份本来就没被替换".into(),
            Why::Unreadable(e) => format!("读不了：{e}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Skipped {
    pub rel: String,
    pub why: Why,
}

pub struct Scan {
    pub root: PathBuf,
    pub files: Vec<ScanFile>,
    pub skipped: Vec<Skipped>,
    /// 二进制文件只给个数，不列名字 —— 项目里的图片、jar 一个个列出来是噪音
    pub binary: usize,
    pub total: usize,
    /// 命中超过 [`MAX_HITS`]：后面的没列，也不许执行
    pub truncated: bool,
    /// 文件索引本身被截断（项目超过 5 万个文件）：之后的文件没看
    pub index_truncated: bool,
    matcher: Matcher,
}

/// 文件身份：`dev + ino`。软链和它指向的文件、硬链接的几份，身份都一样（跟随软链）
fn identity(p: &Path) -> Option<(u64, u64)> {
    fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}

/// 读一个文件，最多 [`MAX_FILE_BYTES`]。超了返回 None（多读一个字节才分得清「正好等于」和「后面还有」）
fn read_capped(p: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut b = Vec::new();
    fs::File::open(p)?.take(MAX_FILE_BYTES + 1).read_to_end(&mut b)?;
    Ok((b.len() as u64 <= MAX_FILE_BYTES).then_some(b))
}

enum One {
    Hit(ScanFile),
    Skip(Skipped),
    Binary,
    Nothing,
}

fn line_bounds(text: &str, at: usize) -> (usize, usize) {
    let ls = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let le = text[at..].find('\n').map_or(text.len(), |i| at + i);
    (ls, le)
}

fn hits_of(m: &Matcher, text: &str) -> Vec<Hit> {
    let ranges = m.find_all(text);
    let mut out = Vec::with_capacity(ranges.len());
    let mut line = 1u64;
    let mut last = 0usize;
    for (s, e) in ranges {
        line += text[last..s].bytes().filter(|&b| b == b'\n').count() as u64;
        last = s;
        let (ls, le) = line_bounds(text, s);
        let lt = &text[ls..le];
        let lines = text[s..e].matches('\n').count() as u32 + 1;
        let (_, be) = block_bounds(text, s, e);
        out.push(Hit {
            start: s,
            end: e,
            line,
            col: text[ls..s].encode_utf16().count() as u32 + 1,
            text: lt.chars().take(LINE_CLIP).collect(),
            spans: searchsvc::utf16_spans(lt, &[(s - ls, e.min(le) - ls)], LINE_CLIP),
            lines,
            block: (lines > 1).then(|| fold(&text[ls..be])),
        });
    }
    out
}

fn scan_one(m: &Matcher, root: &Path, rel: &str, dirty: &HashMap<PathBuf, String>) -> One {
    let abs = root.join(rel);
    if let Some(t) = dirty.get(&abs) {
        let hits = hits_of(m, t);
        if hits.is_empty() {
            return One::Nothing;
        }
        let text_fp = fp::of(t.as_bytes());
        return One::Hit(ScanFile { rel: rel.into(), abs, source: Source::Editor, hits, text: t.clone(), text_fp, raw_fp: None });
    }
    let bytes = match read_capped(&abs) {
        Ok(Some(b)) => b,
        // 超过 8MB：只有真的可能有命中才值得说 —— 但不读完判断不了，所以如实报「没看」
        Ok(None) => return One::Skip(Skipped { rel: rel.into(), why: Why::TooBig }),
        Err(e) => return One::Skip(Skipped { rel: rel.into(), why: Why::Unreadable(e.to_string()) }),
    };
    // 同搜索：头部有 NUL 就是二进制
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return One::Binary;
    }
    let d = fsservice::decode_bytes(&bytes, "");
    let hits = hits_of(m, &d.content);
    if hits.is_empty() {
        return One::Nothing;
    }
    // 有损、混用换行符：**只在真有命中时才说** —— 没命中的文件跳不跳过没有区别
    if d.lossy {
        return One::Skip(Skipped { rel: rel.into(), why: Why::Lossy });
    }
    if d.eol == Eol::Mixed {
        return One::Skip(Skipped { rel: rel.into(), why: Why::MixedEol });
    }
    let text_fp = fp::of(d.content.as_bytes());
    One::Hit(ScanFile {
        rel: rel.into(),
        abs,
        source: Source::Disk { encoding: d.encoding, bom: d.bom, eol: d.eol },
        hits,
        text: d.content,
        text_fp,
        raw_fp: Some(fp::of(&bytes)),
    })
}

/// 找出全部命中。`files` 是相对 `root` 的路径（⌘P 那份索引，同一套跳过规则）；`dirty` 是开着、有未保存改动的标签
/// （绝对路径 → 编辑器里的文本），它们用编辑器里那份。正则写错了返回那句话
pub fn scan(
    root: &Path,
    files: &[String],
    index_truncated: bool,
    q: &Query,
    dirty: &HashMap<PathBuf, String>,
) -> Result<Scan, String> {
    let m = Matcher::new(q)?;
    let per = files.len().div_ceil(THREADS).max(1);
    let results: Vec<One> = std::thread::scope(|sc| {
        let hs: Vec<_> = files
            .chunks(per)
            .map(|chunk| {
                let m = &m;
                sc.spawn(move || chunk.iter().map(|r| scan_one(m, root, r, dirty)).collect::<Vec<_>>())
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });

    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut binary = 0;
    for r in results {
        match r {
            One::Hit(f) => found.push(f),
            One::Skip(s) => skipped.push(s),
            One::Binary => binary += 1,
            One::Nothing => {}
        }
    }

    /*
     * **同一个文件只出现一次**（软链和它指向的文件、硬链接的几份）。不去重的话会各替换一次 ——
     * 第二次核对指纹时那份已经变了，于是被报成「预览之后被改过」，人完全看不懂。
     * 只对有命中的文件取身份：5 万个文件每个 stat 一次是几百毫秒，有命中的通常只有几十个。
     * 留哪一份：编辑器里开着的 > 不是软链的那份（路径就是真身）> 排在前面的
     */
    let rank = |f: &ScanFile| match f.source {
        Source::Editor => 0,
        _ if fs::symlink_metadata(&f.abs).is_ok_and(|m| !m.file_type().is_symlink()) => 1,
        _ => 2,
    };
    // 按身份分组；取不到身份的（读完就被删了之类）自成一组
    let mut groups: Vec<Vec<ScanFile>> = Vec::new();
    let mut by_id: HashMap<(u64, u64), usize> = HashMap::new();
    for f in found {
        match identity(&f.abs) {
            Some(id) if by_id.contains_key(&id) => groups[by_id[&id]].push(f),
            Some(id) => {
                by_id.insert(id, groups.len());
                groups.push(vec![f]);
            }
            None => groups.push(vec![f]),
        }
    }
    let mut files_out = Vec::new();
    for mut g in groups {
        g.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.rel.cmp(&b.rel)));
        let mut it = g.into_iter();
        let win = it.next().expect("每组至少一个");
        for lose in it {
            skipped.push(Skipped { rel: lose.rel, why: Why::Same { as_rel: win.rel.clone() } });
        }
        files_out.push(win);
    }
    files_out.sort_by(|a, b| a.rel.cmp(&b.rel));
    skipped.sort_by(|a, b| a.rel.cmp(&b.rel));

    // 上限：按文件顺序数，数到 5000 为止，后面的文件不列
    let mut total = 0;
    let mut truncated = false;
    let mut cut = files_out.len();
    for (i, f) in files_out.iter().enumerate() {
        if total + f.hits.len() > MAX_HITS {
            truncated = true;
            cut = i;
            break;
        }
        total += f.hits.len();
    }
    files_out.truncate(cut);

    Ok(Scan { root: root.to_path_buf(), files: files_out, skipped, binary, total, truncated, index_truncated, matcher: m })
}

/// 预览里「改后」那一行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct After {
    /// 改后的第一行（跨行的命中、或者替换串里有换行时，后面的在 `block` 里）
    pub text: String,
    /// 换上去的那一段在这一行里的 UTF-16 区间
    pub spans: Vec<[u32; 2]>,
    /// 改前或改后有一边跨行时：改后那几行（整行，超过 200 行中间折叠）
    pub block: Option<String>,
}

impl Scan {
    /// 替换串变了只重算这个 —— 不重新扫盘（docs/REPLACE.md 第 4 节）。顺序和 `files[i].hits[j]` 一一对应
    pub fn after(&self, repl: &str) -> Vec<Vec<After>> {
        self.files
            .iter()
            .map(|f| {
                f.hits
                    .iter()
                    .map(|h| {
                        let (ls, be) = block_bounds(&f.text, h.start, h.end);
                        let r = self.matcher.replacement(&f.text, (h.start, h.end), repl);
                        let whole = format!("{}{}{}", &f.text[ls..h.start], r, &f.text[h.end..be]);
                        let first = whole.split('\n').next().unwrap_or_default();
                        let at = h.start - ls;
                        After {
                            spans: searchsvc::utf16_spans(first, &[(at, (at + r.len()).min(first.len()))], LINE_CLIP),
                            text: first.chars().take(LINE_CLIP).collect(),
                            block: (h.lines > 1 || r.contains('\n')).then(|| fold(&whole)),
                        }
                    })
                    .collect()
            })
            .collect()
    }
}

/// 选中要替换的：哪个文件、它的第几处（`hits` 的下标）。按「第几处」认，不按行号 —— 前面的替换会让后面的行列挪位
#[derive(Debug, Clone)]
pub struct Pick {
    pub rel: String,
    pub hits: Vec<usize>,
}

/// 编辑器里的一笔改动（UTF-16 下标，原文里的位置）。前端 dispatch 成一笔事务
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub from: u32,
    pub to: u32,
    pub insert: String,
}

#[derive(Debug, Clone)]
pub struct Changed {
    pub rel: String,
    pub abs: PathBuf,
    /// 这个文件里改了几处（撤销时是 1：整份换回去）
    pub count: usize,
    /// 开着这个文件的编辑器要跟着做的改动。关着的文件用不上，前端按路径找得到标签才用
    pub edits: Vec<Edit>,
    /// 改到盘上了（关着的、开着但干净的）。false = 只在编辑器里
    pub on_disk: bool,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub changed: Vec<Changed>,
    pub skipped: Vec<Skipped>,
    /// 有替换日志、撤销得了
    pub undo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// 命中超过上限：不替换一半
    Truncated,
    /// 上一次替换中断了还没收拾（启动时那张卡片还没点）：先处理它，不然那份日志会被盖掉
    Pending,
    /// 改前全文超过 [`JOURNAL_CAP`]：要用户点头「这次撤销不了」
    TooBigForUndo { bytes: u64 },
    /// 准备或提交失败。**原文件都已经是改前的样子**（准备段失败一个没动；提交段失败已经退回去了）
    Failed { rel: String, msg: String },
}

pub struct ApplyOpts<'a> {
    /// 替换日志的目录（应用数据目录底下）
    pub journal: &'a Path,
    pub replacement: &'a str,
    pub picks: &'a [Pick],
    /// 此刻开着、有未保存改动的标签（绝对路径 → 编辑器里的文本）。用来核对 `Editor` 类没被改过、
    /// 也用来发现「预览时是干净的，现在被改脏了」的 `Disk` 类
    pub dirty_now: &'a HashMap<PathBuf, String>,
    /// 用户已经点头「这次撤销不了，仍然替换」
    pub allow_no_undo: bool,
    /// 测试用：提交到第 k 个文件时假装崩溃（剩下的临时文件留在盘上，日志停在「提交中」）
    #[doc(hidden)]
    pub crash_after: Option<usize>,
    /// 测试用：替换日志的上限（不给就是 [`JOURNAL_CAP`]）。真造 64MB 的数据来测太贵，判据同 fsservice 的 `read_capped`
    #[doc(hidden)]
    pub journal_cap: Option<u64>,
}

/// 按位置把选中的几处换掉，同时算出编辑器要做的改动（UTF-16）
fn splice(m: &Matcher, text: &str, hits: &[&Hit], repl: &str) -> (String, Vec<Edit>) {
    let mut out = String::with_capacity(text.len());
    let mut edits = Vec::with_capacity(hits.len());
    let (mut at, mut at16) = (0usize, 0u32);
    for h in hits {
        let r = m.replacement(text, (h.start, h.end), repl);
        out.push_str(&text[at..h.start]);
        let from = at16 + text[at..h.start].encode_utf16().count() as u32;
        let to = from + text[h.start..h.end].encode_utf16().count() as u32;
        out.push_str(&r);
        edits.push(Edit { from, to, insert: r });
        at = h.end;
        at16 = to;
    }
    out.push_str(&text[at..]);
    (out, edits)
}

struct Plan<'a> {
    f: &'a ScanFile,
    count: usize,
    edits: Vec<Edit>,
    new_text: String,
    /// `Disk`：改前的原样字节 / 改后要写的字节；`Editor`：改前的文本（UTF-8）
    before: Vec<u8>,
    after: Option<Vec<u8>>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// 执行替换。顺序就是这个函数存在的理由（docs/REPLACE.md 12.2）：
/// 1. 每个文件重读、核对指纹，算出新内容 —— 变了的剔出去
/// 2. 写替换日志（准备中）：改前的字节先落盘
/// 3. 准备段：全部写成临时文件。任何一个失败 → 收掉全部临时文件、丢掉日志，**一个原文件都没动**
/// 4. 日志改成「提交中」，逐个 rename 换上去（硬链接最后，它只能原地写）。中途失败 → 用内存里的改前字节退回去
/// 5. 日志改成「完成」
pub fn apply(scan: &Scan, o: &ApplyOpts) -> Result<Outcome, ApplyError> {
    if scan.truncated {
        return Err(ApplyError::Truncated);
    }
    if journal::load(o.journal).is_some_and(|m| m.state != State::Done) {
        return Err(ApplyError::Pending);
    }

    // ── 1. 重读、核对、算新内容
    let mut plans = Vec::new();
    let mut skipped = Vec::new();
    for p in o.picks {
        let Some(f) = scan.files.iter().find(|f| f.rel == p.rel) else { continue };
        let mut idx: Vec<usize> = p.hits.iter().copied().filter(|&i| i < f.hits.len()).collect();
        idx.sort_unstable();
        idx.dedup();
        if idx.is_empty() {
            continue;
        }
        let hits: Vec<&Hit> = idx.iter().map(|&i| &f.hits[i]).collect();
        let changed = |why| Skipped { rel: f.rel.clone(), why };
        match &f.source {
            Source::Editor => {
                let Some(now) = o.dirty_now.get(&f.abs) else {
                    // 预览时是脏的，现在不脏了：要么存了（盘上那份可能已经不同）、要么关了 —— 都不是预览时那份
                    skipped.push(changed(Why::Changed));
                    continue;
                };
                if fp::of(now.as_bytes()) != f.text_fp {
                    skipped.push(changed(Why::Changed));
                    continue;
                }
                let (new_text, edits) = splice(&scan.matcher, &f.text, &hits, o.replacement);
                plans.push(Plan { f, count: hits.len(), edits, new_text, before: now.clone().into_bytes(), after: None });
            }
            Source::Disk { encoding, bom, eol } => {
                if o.dirty_now.contains_key(&f.abs) {
                    // 预览之后在编辑器里改脏了：盘上那份虽然没变，用户眼前的已经不是预览时那份
                    skipped.push(changed(Why::Changed));
                    continue;
                }
                let bytes = match read_capped(&f.abs) {
                    Ok(Some(b)) => b,
                    _ => {
                        skipped.push(changed(Why::Changed));
                        continue;
                    }
                };
                if Some(fp::of(&bytes)) != f.raw_fp {
                    skipped.push(changed(Why::Changed));
                    continue;
                }
                let (new_text, edits) = splice(&scan.matcher, &f.text, &hits, o.replacement);
                let after = fsservice::encode_text(&new_text, encoding, *bom, *eol);
                plans.push(Plan { f, count: hits.len(), edits, new_text, before: bytes, after: Some(after) });
            }
        }
    }

    // ── 2. 替换日志
    let size: u64 = plans.iter().map(|p| p.before.len() as u64).sum();
    let journaled = if size > o.journal_cap.unwrap_or(JOURNAL_CAP) {
        if !o.allow_no_undo {
            return Err(ApplyError::TooBigForUndo { bytes: size });
        }
        // 用户点了头：没有日志，也就没有撤销和中断恢复。旧的那份也作废（它说的「最近一次」已经不是最近了）
        let _ = journal::remove(o.journal);
        false
    } else {
        true
    };
    let mut manifest = Manifest {
        version: journal::VERSION,
        state: State::Preparing,
        root: scan.root.to_string_lossy().into_owned(),
        at_ms: now_ms(),
        entries: Vec::new(),
    };
    let mut befores = Vec::new();
    for (i, p) in plans.iter().enumerate() {
        let name = format!("{:04}", i + 1);
        let (kind, target, encoding, bom, eol, after) = match &p.f.source {
            Source::Disk { encoding, bom, eol } => (
                Kind::Disk,
                // 真正会被写的那个文件：软链解开（prepare_bytes 里也是这么解的）
                fs::canonicalize(&p.f.abs).unwrap_or_else(|_| p.f.abs.clone()),
                encoding.to_string(),
                *bom,
                eol.as_str().to_string(),
                fp::of(p.after.as_deref().unwrap_or_default()),
            ),
            Source::Editor => (Kind::Editor, p.f.abs.clone(), String::new(), false, String::new(), fp::of(p.new_text.as_bytes())),
        };
        manifest.entries.push(Entry {
            rel: p.f.rel.clone(),
            kind,
            target: target.to_string_lossy().into_owned(),
            before: name.clone(),
            after,
            after_text: fp::of(p.new_text.as_bytes()),
            encoding,
            bom,
            eol,
        });
        befores.push((name, p.before.clone()));
    }
    if journaled {
        journal::create(o.journal, &manifest, &befores).map_err(|e| ApplyError::Failed { rel: String::new(), msg: format!("写替换日志失败：{e}") })?;
    }

    // ── 3. 准备段。失败 → prepared 全部 drop（临时文件自己收掉），丢掉日志
    let mut prepared = Vec::new();
    for p in &plans {
        let Some(bytes) = &p.after else { continue };
        match fsservice::prepare_bytes(&p.f.abs, bytes) {
            Ok(x) => prepared.push((p, x)),
            Err(e) => {
                drop(prepared);
                if journaled {
                    let _ = journal::remove(o.journal);
                }
                return Err(ApplyError::Failed { rel: p.f.rel.clone(), msg: e.to_string() });
            }
        }
    }

    // ── 4. 提交段。硬链接排最后：它只能原地写，没有「准备好再一下子换上」
    if journaled {
        manifest.state = State::Committing;
        journal::save(o.journal, &manifest).map_err(|e| ApplyError::Failed { rel: String::new(), msg: format!("写替换日志失败：{e}") })?;
    }
    prepared.sort_by_key(|(_, x)| x.in_place());
    let mut committed: Vec<&Plan> = Vec::new();
    let mut rest = prepared.into_iter();
    while let Some((p, x)) = rest.next() {
        if o.crash_after == Some(committed.len()) {
            // 假装进程在这一刻没了：剩下的临时文件不收（真崩溃也收不了），日志停在「提交中」
            for (_, y) in rest {
                std::mem::forget(y);
            }
            std::mem::forget(x);
            return Err(ApplyError::Failed { rel: p.f.rel.clone(), msg: "（测试）在这里崩溃".into() });
        }
        if let Err(e) = x.commit() {
            // 运行中的失败用不着等下次启动：改前的字节就在内存里，当场退回去
            drop(rest);
            for c in &committed {
                let _ = fsservice::prepare_bytes(&c.f.abs, &c.before).and_then(|y| y.commit());
            }
            if journaled {
                let _ = journal::remove(o.journal);
            }
            return Err(ApplyError::Failed { rel: p.f.rel.clone(), msg: format!("{e}（已经改了的 {} 个文件退回去了）", committed.len()) });
        }
        committed.push(p);
    }

    // ── 5. 完成
    if journaled {
        manifest.state = State::Done;
        let _ = journal::save(o.journal, &manifest);
    }
    let changed = plans
        .iter()
        .map(|p| Changed {
            rel: p.f.rel.clone(),
            abs: p.f.abs.clone(),
            count: p.count,
            edits: p.edits.clone(),
            on_disk: p.after.is_some(),
        })
        .collect();
    Ok(Outcome { changed, skipped, undo: journaled })
}

/// 此刻开着的标签（撤销时用）：编辑器里的文本、有没有未保存改动
#[derive(Debug, Clone)]
pub struct OpenNow {
    pub text: String,
    pub dirty: bool,
}

/// 编辑器里的文本整份换成 `to`（撤销时用）
fn replace_all(from: &str, to: &str) -> Vec<Edit> {
    vec![Edit { from: 0, to: from.encode_utf16().count() as u32, insert: to.to_string() }]
}

/// 盘上此刻的字节和它解码后的文本（读不了、太大都是 None）
fn disk_now(p: &Path) -> Option<(Vec<u8>, fsservice::encoding::Decoded)> {
    let b = read_capped(p).ok()??;
    let d = fsservice::decode_bytes(&b, "");
    Some((b, d))
}

/// 撤销最近一次替换（docs/REPLACE.md 第 6 节）。**此刻的内容还等于「改后」才写回「改前」**，否则跳过并说明。
/// 走哪条路按此刻算：开着就比编辑器里的、改编辑器（干净的顺带写盘）；没开就比盘上的、写盘。
/// `open`：此刻开着的标签，绝对路径 → 文本和脏不脏。
pub fn undo(dir: &Path, open: &HashMap<PathBuf, OpenNow>) -> Result<Outcome, String> {
    let m = journal::load(dir).ok_or("没有可以撤销的替换")?;
    if m.state != State::Done {
        return Err("上一次替换没有做完，先在启动时的卡片上处理它".into());
    }
    // 开着的标签按身份认：它可能开的是软链那条路径，日志里记的是真身
    let open_by_id: HashMap<(u64, u64), (&PathBuf, &OpenNow)> =
        open.iter().filter_map(|(p, o)| identity(p).map(|id| (id, (p, o)))).collect();

    let mut changed = Vec::new();
    let mut skipped = Vec::new();
    let mut writes: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for e in &m.entries {
        let target = PathBuf::from(&e.target);
        let before = journal::read_before(dir, e).map_err(|x| format!("读替换日志失败：{x}"))?;
        let before_text = match e.kind {
            Kind::Disk => fsservice::decode_bytes(&before, &e.encoding).content,
            Kind::Editor => String::from_utf8_lossy(&before).into_owned(),
        };
        let skip = |why| Skipped { rel: e.rel.clone(), why };
        if let Some((path, now)) = identity(&target).and_then(|id| open_by_id.get(&id)) {
            if fp::of(now.text.as_bytes()) != e.after_text {
                skipped.push(skip(Why::EditedSince));
                continue;
            }
            // 干净的标签：盘上那份和编辑器里一样，一起退回去（替换时也是一起改的）
            let mut on_disk = false;
            if !now.dirty {
                if let Some((bytes, d)) = disk_now(&target) {
                    if e.kind == Kind::Disk && fp::of(&bytes) == e.after {
                        writes.push((target.clone(), before.clone()));
                        on_disk = true;
                    } else if e.kind == Kind::Editor && fp::of(d.content.as_bytes()) == e.after_text {
                        writes.push((target.clone(), fsservice::encode_text(&before_text, d.encoding, d.bom, d.eol)));
                        on_disk = true;
                    }
                }
            }
            changed.push(Changed { rel: e.rel.clone(), abs: (*path).clone(), count: 1, edits: replace_all(&now.text, &before_text), on_disk });
            continue;
        }
        match (e.kind, disk_now(&target)) {
            (Kind::Disk, Some((bytes, _))) if fp::of(&bytes) == e.after => {
                writes.push((target.clone(), before));
            }
            // 替换只进了编辑器、后来存了：盘上就是「改后」，按盘上的编码写回「改前」的文本
            (Kind::Editor, Some((_, d))) if fp::of(d.content.as_bytes()) == e.after_text => {
                writes.push((target.clone(), fsservice::encode_text(&before_text, d.encoding, d.bom, d.eol)));
            }
            (Kind::Editor, _) => {
                skipped.push(skip(Why::NeverSaved));
                continue;
            }
            _ => {
                skipped.push(skip(Why::EditedSince));
                continue;
            }
        }
        changed.push(Changed { rel: e.rel.clone(), abs: target, count: 1, edits: Vec::new(), on_disk: true });
    }

    // 撤销也走准备 / 提交：全部准备好了再换上。崩在中间的话日志还在（状态是完成），再撤一次 ——
    // 已经退回去的那些此刻不等于「改后」，会被跳过，剩下的接着退
    let mut prepared = Vec::new();
    for (t, b) in &writes {
        prepared.push(fsservice::prepare_bytes(t, b).map_err(|x| format!("撤销失败（一个文件都没动）：{}：{x}", t.display()))?);
    }
    prepared.sort_by_key(|x| x.in_place());
    for x in prepared {
        let t = x.target().to_path_buf();
        x.commit().map_err(|x| format!("撤销到一半失败：{}：{x}（再点一次撤销会接着退剩下的）", t.display()))?;
    }
    journal::remove(dir).map_err(|x| x.to_string())?;
    Ok(Outcome { changed, skipped, undo: false })
}

/// 启动时看一眼：有没有上次留下的替换日志
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub state: State,
    pub root: String,
    pub at_ms: u64,
    pub files: usize,
    /// 盘上此刻已经是「改后」的文件数（提交中断时：已改 N 个、未改 M 个）
    pub changed: usize,
}

pub fn pending(dir: &Path) -> Option<Pending> {
    let m = journal::load(dir)?;
    let disk: Vec<&Entry> = m.entries.iter().filter(|e| e.kind == Kind::Disk).collect();
    let changed = disk
        .iter()
        .filter(|e| disk_now(Path::new(&e.target)).is_some_and(|(b, _)| fp::of(&b) == e.after))
        .count();
    Some(Pending { state: m.state, root: m.root, at_ms: m.at_ms, files: m.entries.len(), changed })
}

/// 收拾上次中断的替换（docs/REPLACE.md 12.2）。`rollback`：退回改前；否则保留现状（之后还能撤销）。
/// - 准备中断的：原文件一个没动，收掉临时文件、丢掉日志
/// - 提交中断的：收掉临时文件；退回 = 把此刻是「改后」的那些写回改前，然后丢掉日志；保留 = 日志改成完成
pub fn recover(dir: &Path, rollback: bool) -> Result<Outcome, String> {
    let Some(mut m) = journal::load(dir) else { return Ok(Outcome { changed: vec![], skipped: vec![], undo: false }) };
    if m.state == State::Done {
        return Ok(Outcome { changed: vec![], skipped: vec![], undo: true });
    }
    for e in m.entries.iter().filter(|e| e.kind == Kind::Disk) {
        let tmp = fsservice::tmp_path_for(Path::new(&e.target));
        // 只删真是普通文件的：同名的软链之类不碰（它不会是我们留下的）
        if fs::symlink_metadata(&tmp).is_ok_and(|x| x.is_file()) {
            let _ = fs::remove_file(&tmp);
        }
    }
    if m.state == State::Preparing {
        journal::remove(dir).map_err(|x| x.to_string())?;
        return Ok(Outcome { changed: vec![], skipped: vec![], undo: false });
    }
    if !rollback {
        m.state = State::Done;
        journal::save(dir, &m).map_err(|x| x.to_string())?;
        return Ok(Outcome { changed: vec![], skipped: vec![], undo: true });
    }
    let mut changed = Vec::new();
    for e in m.entries.iter().filter(|e| e.kind == Kind::Disk) {
        let target = PathBuf::from(&e.target);
        if disk_now(&target).is_some_and(|(b, _)| fp::of(&b) == e.after) {
            let before = journal::read_before(dir, e).map_err(|x| x.to_string())?;
            fsservice::prepare_bytes(&target, &before).and_then(|x| x.commit()).map_err(|x| format!("{}：{x}", e.rel))?;
            changed.push(Changed { rel: e.rel.clone(), abs: target, count: 1, edits: vec![], on_disk: true });
        }
    }
    journal::remove(dir).map_err(|x| x.to_string())?;
    Ok(Outcome { changed, skipped: vec![], undo: false })
}
