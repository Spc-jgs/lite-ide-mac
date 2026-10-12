//! 草稿：新建（带锚点）、frontmatter、列表与摘要、丢弃空草稿

use super::*;

/// 在草稿目录里新建一份草稿，返回新路径。
///
/// `stem` 是不带扩展名的名字，由前端按**本地时间**生成（`2026-09-09 1030`）——
/// 时区这件事只有前端知道，std 里没有本地时区，为它拽一个日期库进来不值。
///
/// 撞名往后加序号（`… 1030-2.md`）：一分钟内建第二份草稿是很平常的事，
/// 而 [`create_entry`] 撞名是直接失败的 —— 那个行为对「新建文件」是对的
/// （名字是人取的，撞了要让人知道），对这里是错的（名字是机器取的，
/// 撞了该机器自己让开）。
///
/// 目录用 `create_dir_all` 而不是 `create_dir`：这里的语义就是「确保它在」，
/// 和 [`create_entry`] 里那条「新建一个已存在的文件夹要报错」不是一回事。
pub fn create_scratch(dir: impl AsRef<Path>, stem: &str) -> io::Result<PathBuf> {
    create_scratch_with(dir, stem, None)
}

/// 草稿的锚点（M10）：记这条是**在哪儿**写的。四样都可以为空。
///
/// 存在文件头的 frontmatter 里（见 [`frontmatter`]），不存旁车文件：
/// 用 Sublime 打开这份草稿要一样能读，而 frontmatter 是 markdown 世界公认的写法。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anchor {
    /// 项目根的绝对路径
    pub project: String,
    pub branch: String,
    /// HEAD 的短 sha
    pub head: String,
    /// `相对路径:行`，光标当时停在哪
    pub at: String,
}

impl Anchor {
    pub fn is_empty(&self) -> bool {
        self.project.is_empty() && self.branch.is_empty() && self.head.is_empty() && self.at.is_empty()
    }

    /// 写成文件头。空字段不写；全空返回空串（不写头）。
    /// 结尾多一个空行 —— 光标要落在头**下面**，不是贴着 `---`。
    pub fn frontmatter(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut s = String::from("---\n");
        for (k, v) in [("project", &self.project), ("branch", &self.branch), ("head", &self.head), ("at", &self.at)] {
            if !v.is_empty() {
                s.push_str(k);
                s.push_str(": ");
                s.push_str(v);
                s.push('\n');
            }
        }
        s.push_str("---\n\n");
        s
    }
}

/// 同 [`create_scratch`]，带锚点时把 frontmatter 写进去。
pub fn create_scratch_with(dir: impl AsRef<Path>, stem: &str, anchor: Option<&Anchor>) -> io::Result<PathBuf> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir)?;
    let head = anchor.map(Anchor::frontmatter).unwrap_or_default();
    for n in 1..=99u32 {
        let name = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{n}.md")
        };
        match create_entry(dir, &name, false) {
            Ok(p) => {
                if !head.is_empty() {
                    // 刚 create_new 出来的空文件，直接写；失败就把空壳收掉，别留一份没头的
                    #[allow(clippy::disallowed_methods)] // 刚 create_new 出来的空文件，写失败把空壳收掉
                    if let Err(e) = fs::write(&p, head.as_bytes()) {
                        let _ = fs::remove_file(&p);
                        return Err(e);
                    }
                }
                return Ok(p);
            }
            // 只有撞名才换个名字再来，别的错误（没权限、盘满）原样上抛 ——
            // 吞掉它们的话，这里会变成一个转 99 圈再报「撞名太多」的死循环
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "同一分钟里已经有 99 份草稿了",
    ))
}

/// 丢掉一份**一个字都没写过**的草稿。
///
/// # 这是整个应用里唯一一条真删除，所以它自己校验
///
/// 点了加号又没用它，留下一个 0 字节的文件是纯噪音；而让它走废纸篓，
/// 是把噪音换个地方堆。所以这里真删 —— 但**只删不可能有内容的东西**，
/// 三条判据缺一不可，全在这个函数里，调用方说什么都不算数：
///
/// 1. `symlink_metadata` + `is_file()` —— **不跟随符号链接**
/// 2. 大小必须是 0
/// 3. **父目录**规范化之后必须仍在草稿目录里（挡掉 `..` 拼出来的路径）
///
/// 第 1 条真正在防的是 `symlink_metadata`：换成跟随链接的 `metadata`，
/// 一条指向草稿目录里另一个空文件的链接就能让三条判据全过，
/// 而 `remove_file` 删掉的是**链接指向的那个文件**（测试里有这一条，验过会红）。
///
/// `is_file()` 本身是纵深防御,**单独去掉它测试不会红** —— 目录和符号链接的
/// `symlink_metadata().len()` 恰好都不是 0，会被第 2 条拦下。留着它是因为
/// 「只删普通文件」是这里的真实意图，不该靠另一条判据的巧合来兑现。
///
/// # 规范化只用来**判断**，绝不用来**指定删谁**（issue #26）
///
/// 上一版是 `let real = path.canonicalize()?;` 然后 `remove_file(real)`。
/// 那个 `real` 是**跟随了符号链接之后**的路径，于是判据和动作作用在
/// 两个不同的东西上，中间隔着一次路径解析：
///
/// > 三条判据过完、`canonicalize` 之前，把这个空文件换成一条指向草稿目录里
/// > 另一份文件的符号链接 —— `canonicalize` 跟到目标，目标在草稿目录里所以
/// > `starts_with` 也过，`remove_file` 删掉的是那个**非空**的目标。
///
/// 所以现在删的是**调用方给的那个名字**：`unlink` 摘掉的是目录项本身，
/// 最后一级是符号链接时它删链接、不碰目标。要判「在不在草稿目录里」，
/// 就只规范化**父目录** —— 父目录的解析不会跟随最后那一级。
///
/// 剩下的缝要说清楚，别当成没有：检查和 `unlink` 之间那个名字仍然可能被换掉，
/// 换成非空文件的话我们会把它摘掉。但换进来的东西**只能在草稿目录里**
/// （父目录已经钉死了），也就是应用自己数据目录下的一份草稿；
/// 原来那条「顺着链接删到目录外面 / 删掉非空文件」的路已经不存在。
/// 要连这条缝一起堵得上 `O_NOFOLLOW` + `fstat` 绑同一个 fd，那要引 `libc`。
///
/// 判据写在这儿而不是前端，是同一条老规矩：**前端少一个把东西删到别处去的机会**。
///
/// 用户改了又删光再关闭的那种文件走不到这里 —— 那时 `dirty` 是真的，
/// 界面会先弹「保存并关闭 / 丢弃改动」，根本不会静默走到这一步。
#[allow(clippy::disallowed_methods)] // 唯一一条真删用户文件的窄路：只删 0 字节的草稿（上面那几道判断）
pub fn discard_empty_scratch(dir: impl AsRef<Path>, path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "只丢得掉普通文件",
        ));
    }
    /*
     * 「空」= 正文一个字都没有。带锚点的草稿一建出来就有几十字节的头（M10），
     * 按 `len() == 0` 判的话，⌘N 之后一个字没写就关掉的那份永远收不掉。
     * 只读头 4KB：头本身几十字节，正文超过 4KB 的必然不是空的。
     */
    if meta.len() != 0 {
        use std::io::Read;
        let mut buf = vec![0u8; SCRATCH_PREVIEW_BYTES];
        let n = fs::File::open(path).and_then(|mut f| f.read(&mut buf))?;
        buf.truncate(n);
        let text = std::str::from_utf8(&buf).unwrap_or("");
        let (anchor, body) = frontmatter(text);
        let blank = anchor.is_some() && (meta.len() as usize) <= n && text[body..].trim().is_empty();
        if !blank {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "这份草稿里有东西，不能这么丢",
            ));
        }
    }
    // 只规范化父目录：最后一级留着不解析，否则又回到「判一个、删另一个」
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "没有父目录"))?
        .canonicalize()?;
    let root = dir.as_ref().canonicalize()?;
    if !parent.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "不在草稿目录里",
        ));
    }
    fs::remove_file(path)
}

/// 把文件头的 frontmatter 切出来：`(锚点, 正文起点的字节偏移)`。
///
/// 只认最窄的一种形状：第一行恰好是 `---`，接着若干 `key: value` 行，
/// 再一行 `---`。不是这个形状就当没有头（偏移 0）—— 正文里自己写的 `---`
/// 分隔线不会被误认，因为它不在第一行。
/// 认不出的 key 跳过不报错：以后加字段，老版本打开新草稿要照常。
/// 头最多找 32 行：没找到收尾的 `---` 就当没有头，别把一整篇正文当成头吃掉。
pub fn frontmatter(text: &str) -> (Option<Anchor>, usize) {
    let mut lines = text.split_inclusive('\n');
    match lines.next() {
        Some("---\n") | Some("---\r\n") => {}
        _ => return (None, 0),
    }
    let mut off = 4 + usize::from(text.starts_with("---\r"));
    let mut a = Anchor::default();
    for (i, l) in lines.enumerate() {
        off += l.len();
        let t = l.trim_end_matches(['\n', '\r']);
        if t == "---" {
            // 头后面紧跟的那个空行也算头的一部分 —— 正文从第一行有字的地方起
            if text[off..].starts_with('\n') {
                off += 1;
            }
            return (Some(a), off);
        }
        if i >= 32 {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            let v = v.trim().to_string();
            match k.trim() {
                "project" => a.project = v,
                "branch" => a.branch = v,
                "head" => a.head = v,
                "at" => a.at = v,
                _ => {}
            }
        }
    }
    (None, 0)
}

/// 文件头占几行（含收尾的 `---`）。没有头是 0。
/// 搜草稿内容时用它把头里的命中滤掉 —— 搜 `main` 不该把所有 main 分支上写的草稿都翻出来。
pub fn frontmatter_lines(path: &Path) -> usize {
    use std::io::Read;
    let mut buf = vec![0u8; SCRATCH_PREVIEW_BYTES];
    let n = match fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => n,
        Err(_) => return 0,
    };
    buf.truncate(n);
    let text = match std::str::from_utf8(&buf) {
        Ok(t) => t,
        Err(e) => std::str::from_utf8(&buf[..e.valid_up_to()]).unwrap_or(""),
    };
    match frontmatter(text) {
        (Some(_), off) => text[..off].trim_end_matches('\n').lines().count(),
        _ => 0,
    }
}

/// 草稿列表里的一条（issue #40）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scratch {
    pub name: String,
    pub path: PathBuf,
    /// 修改时间（Unix 毫秒）。取不到时为 0
    pub mtime_ms: u64,
    /// 第一行有字的内容，截到 [`SCRATCH_PREVIEW_CHARS`] 个字符。空文件是空串
    pub first_line: String,
    /// 文件头里的锚点（M10）。没有头就是 None
    pub anchor: Option<Anchor>,
}

/// 摘要最多几个字符。侧边栏一行 240px 也就放得下这么多，多读只是白读
pub const SCRATCH_PREVIEW_CHARS: usize = 80;
/// 为了那一行摘要最多读多少字节。草稿可能很大（贴一段日志进去），不能整份读
const SCRATCH_PREVIEW_BYTES: usize = 4096;

/// 草稿目录里的文件，**按名字倒序** —— 名字是 `2026-09-09 1030.md` 这种时间戳，
/// 倒序就是最近的在上面，而且不依赖 mtime（拷来拷去的文件 mtime 不可信）。
///
/// 不递归、只列普通文件（`.md`）：目录里除了草稿不该有别的东西，
/// 真有（手动放的），当它不存在比列一个打不开的条目强。
/// 目录还不存在 = 一条都没记过，返回空列表**不是错误**。
pub fn list_scratches(dir: impl AsRef<Path>) -> io::Result<Vec<Scratch>> {
    let dir = dir.as_ref();
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for ent in rd {
        let ent = ent?;
        let meta = match ent.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".md") || name.starts_with('.') {
            continue;
        }
        let path = ent.path();
        let (anchor, first_line) = scratch_preview(&path);
        out.push(Scratch {
            first_line,
            anchor,
            mtime_ms: stamp(&path).map(|s| s.mtime_ms).unwrap_or(0),
            name,
            path,
        });
    }
    // 倒序，但同一分钟里的第 2、3 份（`… 1103-2.md`）要排在第 1 份**上面**：
    // 按字符比 '.' > '-'，`1103.md` 会压在 `1103-2.md` 前面，后建的反而在下
    out.sort_by(|a, b| scratch_sort_key(&b.name).cmp(&scratch_sort_key(&a.name)));
    Ok(out)
}

/// `2026-09-16 1103-2.md` → `("2026-09-16 1103", 2)`；没有序号的算第 1 份
fn scratch_sort_key(name: &str) -> (String, u32) {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    if let Some((base, n)) = stem.rsplit_once('-') {
        if let Ok(n) = n.parse::<u32>() {
            // 只认 `create_scratch` 加的那种「空格时间-序号」，别把日期里的 `-` 当序号
            if base.contains(' ') {
                return (base.to_string(), n);
            }
        }
    }
    (stem.to_string(), 1)
}

/// 第一行有字的内容。只读头 [`SCRATCH_PREVIEW_BYTES`]，按 UTF-8 尽量解
/// （截在多字节字符中间的那半个丢掉），跳过空行和 markdown 的 `#` 前缀。
/// 顺便把头里的锚点也解出来 —— 反正这 4KB 已经读进来了。
fn scratch_preview(path: &Path) -> (Option<Anchor>, String) {
    use std::io::Read;
    let mut buf = vec![0u8; SCRATCH_PREVIEW_BYTES];
    let n = match fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => n,
        Err(_) => return (None, String::new()),
    };
    buf.truncate(n);
    let text = match std::str::from_utf8(&buf) {
        Ok(t) => t,
        Err(e) => std::str::from_utf8(&buf[..e.valid_up_to()]).unwrap_or(""),
    };
    let (anchor, body) = frontmatter(text);
    let first = text[body..]
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(SCRATCH_PREVIEW_CHARS).collect())
        .unwrap_or_default();
    (anchor, first)
}

// ─────────────── 命令行工具 `lite`（issue #40 第二层） ───────────────
