//! 文件树与文本读写。
//!
//! 与 logengine 同样的纪律：零 Tauri 依赖，可独立测试。
//!
//! 编码策略见 [`encoding`] 模块：**探测 → 记住 → 原样写回**。
//! 用什么编码读进来的就用什么编码存回去，保存不做「顺手转成 UTF-8」这种擅自决定。

pub mod encoding;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    /// 名字命中了 [`excludes::GENERATED_DIRS`] —— **这是「怀疑」，不是「判决」**。
    ///
    /// 界面据此把它压暗、不自动展开、也不预取子目录，但它**在树里**、
    /// 点得开、里面的文件打得开。判据见 `list_dir` 的说明。
    pub generated: bool,
}

/// 列出一层目录。不递归 —— 文件树按需展开，避免大仓库一次性遍历。
///
/// **点文件和点目录一律列出来。** 原来它们跟着 `show_hidden` 一起被藏了，
/// 于是 `.gitignore` `.github/` `.env` `.claude/` 这些**天天要改的项目文件**
/// 在文件树里根本不存在，只能靠 ⌘P 摸黑打开。
///
/// 藏它们的那个理由（「否则文件树被淹没」）说的其实是 `node_modules` 那一类，
/// 而那一类现在由 [`excludes::GENERATED_DIRS`] 单独挡着 —— 两件事本来就不该
/// 共用一个开关。`.git/` 也照列：树是懒展开的，不点开它就只是一行。
///
/// 那份名单是**和搜索共用的同一份**。原来这里自己有四个、searchsvc 自己有十四个，
/// 于是「树里看不见」和「⌘P 搜不到」是两套判据，各自演化。
///
/// # 生成物目录**列出来，只是压暗**（issue #13，2026-09-10 改）
///
/// 原来是 `continue` 掉的 —— 名字命中就当它不存在。那条判据有个说不清的后果：
///
/// > `dist` 和 `build` 是**常见的源码目录名**。CMake 项目的 `build/` 里放的
/// > 是构建脚本，有的项目 `dist/` 里放的是要提交的产物。这种项目在 lite-ide
/// > 里看到的是一个**凭空少了一个目录**的文件树，而且没有任何提示。
///
/// 「名字叫 build」只是**怀疑**，不是证据。真正的证据是 git 忽不忽略它，
/// 而这个 crate 零 git 依赖（也不该有 —— 起 git 必须走 `gitsvc::git_cmd` 那套
/// 加固，否则 `.git/config` 就能让一次列目录去执行任意命令）。
///
/// 所以这里不再替界面做决定：**照列，打上 `generated` 标记**，
/// 「压暗还是隐藏」由界面回答。信息不丢，噪声也不进来 —— IDEA 对
/// excluded 目录就是这么做的。
///
/// **搜索那半没跟着改**：`⌘P` / `⇧⌘F` 仍然按名字跳过这些目录。
/// 那半要做对得让 git 说话（rg 默认就认 `.gitignore`，而内置兜底实现不认，
/// 「装了 rg 和没装 rg 结果要一样」是那个模块的前提）。#13 还开着记这件事。
///
/// 排序：目录在前，同类按名称不区分大小写排列，与 Finder / IDEA 一致。
/// **生成物不单独排到末尾** —— 挪位置比压暗更让人意外，而且 `target/` 一旦
/// 换了位置，「它刚才还在这儿」这种困惑比看见它更贵。
pub fn list_dir(dir: impl AsRef<Path>) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for ent in fs::read_dir(dir.as_ref())? {
        let ent = ent?;
        let name = ent.file_name().to_string_lossy().into_owned();
        let meta = match ent.metadata() {
            Ok(m) => m,
            // 断掉的软链等：跳过而不是整个目录失败
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        out.push(Entry {
            // 只有目录才谈得上「生成物目录」。一个叫 `build` 的**文件**
            // （shell 脚本很常见）压暗它没有任何道理
            generated: is_dir && excludes::is_generated_dir(&name),
            name,
            path: ent.path(),
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
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

/**
 * 编辑模式一次能读多少。
 *
 * `logengine::probe` 的 `MAX_EDIT_BYTES`（32MB）决定的是「自动判定走哪种模式」，
 * 用户可以手动强制切回编辑模式 —— 那条逃生门要留着。这里是它上面的**硬顶**。
 *
 * 为什么必须有：`probe_path` 的体积判定只在**首次打开**时把关，而
 * 「外部改动后重读」「换编码重开」「工作树变了重读」三条路都是直接
 * `read_text`，一次都不再问体积。一个打开时 1KB、后来涨到 2GB 的文件
 * （日志正是这样）会从这几条路里进来，然后 `fs::read` 把它整份吞进内存 ——
 * 再加上解码一份、JSON 序列化一份、JS 字符串一份。
 *
 * 判据和 `gitsvc::MAX_DIFF_BYTES` 是同一条：**读子进程/读文件之前先问一句
 * 这东西有没有上限**。超了就报错，让用户用日志模式打开（mmap，多大都不占内存）。
 */
pub const MAX_READ_BYTES: u64 = 64 << 20;

/// 读取文本文件并自动探测编码。
///
/// `label` 非空时按指定编码读（用户在状态栏点了「以其他编码重新打开」）。
pub fn read_text_detect(path: impl AsRef<Path>, label: &str) -> io::Result<encoding::Decoded> {
    read_capped(path.as_ref(), label, MAX_READ_BYTES)
}

/// 上限可注入，测试用 —— 真造一个 64MB 的临时文件太贵，而这条闸必须被测到。
fn read_capped(path: &Path, label: &str, cap: u64) -> io::Result<encoding::Decoded> {
    use std::io::Read;
    // 边读边卡，不先 stat 再读：文件可能在这两步之间涨大（tail 里的日志就是），
    // 那样 stat 说没超、读进来的却超了。多读一个字节，是为了把
    // 「正好等于上限」和「后面还有」分开 —— 差这一个字节就会误报超限。
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(cap + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(io::Error::other(format!(
            "文件超过 {} MB，编辑模式读不下 —— 用日志模式打开（mmap，多大都不占内存）",
            cap >> 20
        )));
    }
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

/// 软链要写进它**指向的那个文件**。
///
/// 不解引用的话，「临时文件 + rename」会把**软链本身**换成一个普通文件 ——
/// 实测过：改动写不进真身（真身内容一字未动），而链接也没了。
/// 编辑 `~/.zshrc` 这种指向 dotfiles 仓库的软链，正好踩个正着。
///
/// 断掉的软链直接报错，不往下走：那种情况下 rename 会「成功」，
/// 代价是把一条链接悄悄变成普通文件。
fn resolve_link(path: &Path) -> io::Result<PathBuf> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        // 还不在盘上（新建文件的第一次保存），按原路径写
        return Ok(path.to_path_buf());
    };
    if !meta.file_type().is_symlink() {
        return Ok(path.to_path_buf());
    }
    fs::canonicalize(path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("{} 是一条断掉的软链，写不进去", path.display()),
        )
    })
}

/// 原地覆写。**只给硬链接用。**
///
/// 丢掉了「rename 是原子的」这条性质，换来的是「这一份还留在链接组里」。
/// vim 也是这么分的（`backupcopy=auto`：多个硬链接或软链时改用覆写），
/// 判据一样 —— 用户建硬链接就是要它们是同一个文件，
/// 保存一次把它摘出去，比崩在写一半更难发现。
fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new().write(true).truncate(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

fn write_bytes(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;

    let target = resolve_link(path.as_ref())?;
    let target = target.as_path();
    let meta = fs::symlink_metadata(target).ok();

    // 硬链接组里的一份：rename 会把它摘出去，只能原地覆写
    if meta.as_ref().is_some_and(|m| m.nlink() > 1) {
        return write_in_place(target, bytes);
    }

    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.lite-ide-tmp",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));

    // `create_new` 而不是 `fs::write`：临时文件名是可预测的，而
    // `fs::write` 对一条**已经在那儿的软链**会顺着它写到别处去。
    // 撞上了就先删掉再建（多半是上次崩在中间留下的残留）。
    let mut f = match fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(&tmp)?;
            fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?
        }
        Err(e) => return Err(e),
    };
    f.write_all(bytes)?;
    // **少了这一句，掉电之后拿到的可能是一个 0 字节文件盖掉了原文。**
    // rename 只保证「要么旧的要么新的」，不保证新的那份内容已经落盘 ——
    // 元数据操作可以先于数据落到日志里。进程崩溃本来就没这个问题
    // （内核已经收下了数据），掉电才有，所以这条测不出来，只能写在这儿。
    // 注：macOS 上 `fsync` 不保证刷穿硬盘自己的写缓存，那要 `F_FULLFSYNC`，
    // 而那需要引 libc。这里要的是**数据先于 rename**这个次序，fsync 够。
    f.sync_all()?;
    drop(f);

    // 权限跟着原文件走。少了这句，一个 0755 的脚本保存完变成 0644，
    // 当场就不能执行了 —— 临时文件是新建的，带的是 umask 的默认权限。
    // （xattr / ACL 这一套仍然带不过来，那是 rename 这条路的固有代价。）
    if let Some(m) = &meta {
        fs::set_permissions(&tmp, m.permissions())?;
    }

    // rename 在同一文件系统内是原子的
    match fs::rename(&tmp, target) {
        Ok(()) => {
            // 目录项也要落盘，否则掉电后可能连改名都没发生
            if let Ok(d) = fs::File::open(dir) {
                let _ = d.sync_all();
            }
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// `open -R` 的参数。
///
/// 抽成纯函数是为了能测 —— 真去 spawn 会把 Finder 弹到用户脸上，
/// 测试里不能干这事；而这里唯一值得钉死的性质（`--` 必须在路径之前）
/// 恰好是纯数据。
///
/// **`--` 不是防御性的摆设。** 实测一个名叫 `-Q` 的文件：
///
/// ```text
/// open -R "-Q"       →  open: invalid option -- Q
/// open -R -- "-Q"    →  正常显示
/// ```
///
/// 也就是说少了它，文件名就能变成命令行开关 —— 与 AGENTS.md 里
/// gitsvc / searchsvc 那条「路径前一律加 `--`」是同一条纪律。
fn reveal_args(path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("-R"),
        OsString::from("--"),
        path.as_os_str().to_os_string(),
    ]
}

/// 路径在不在盘上 —— **只看这个条目本身，不跟随符号链接**。
///
/// 抽出来是为了让 reveal 的存在性判定可测：直接测 `reveal_in_finder`
/// 就得让它真的把 Finder 弹出来，而不测的话，这行改成 `try_exists`
/// 也没人会发现（试过，测试照样绿）。
fn exists_for_reveal(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// 在 Finder 里选中并显示一个路径。
///
/// 路径不存在时**自己报错，不去起子进程**：`open` 对不存在的路径会打印
/// 一句英文并返回 1，那句话里带着完整绝对路径，糊在状态栏那一格里
/// 既读不完也说不清。自己判一次，给一句中文。
///
/// 用 `symlink_metadata` 而不是 `try_exists`：后者会跟随符号链接，
/// 于是一个指向已删除目标的坏链接会被判成"不存在" —— 但链接本身在盘上，
/// Finder 完全显示得出来，这种时候不该拦。
pub fn reveal_in_finder(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    if !exists_for_reveal(path) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} 不在盘上了", path.display()),
        ));
    }

    // 绝对路径而不是靠 PATH：`open` 在 macOS 上固定在这儿，
    // 而从终端启动 lite-ide 时 PATH 是用户的，不该让它决定我们调到哪个 open
    let st = Command::new("/usr/bin/open")
        .args(reveal_args(path))
        // 绝不让子进程卡住等输入（AGENTS.md）。open 本身不读 stdin，
        // 但 `-f` 那类开关是读的 —— 万一参数构造出了错，宁可它立刻 EOF
        .stdin(Stdio::null())
        .status()?;

    if st.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Finder 没能显示它（open 退出码 {}）",
            st.code().map_or_else(|| "被信号中断".to_string(), |c| c.to_string())
        )))
    }
}

// ─────────────────── 新建 / 重命名 / 移到废纸篓 ───────────────────
//
// issue #6 把这三样和「复制路径」那批刻意分开：那批只是读，这批**会改盘上的东西**。
// 三条共同的纪律：
//
// 1. **绝不静默覆盖。** std 里最顺手的那两个 API（`File::create`、`fs::rename`）
//    默认都会吃掉已有文件，各自的注释里有实测对照。
// 2. **名字在这一层校验，前端不拼路径。** 前端只递「在哪个目录、叫什么」，
//    join 和校验都发生在这里 —— 少一个前端拼错路径把文件写到别处的机会。
// 3. **删除只进废纸篓。** 个人工具，误删一个目录没有任何补救手段：
//    没有回收站，未跟踪的文件 git 也救不回来。

/// 单个路径组件的字节上限。APFS / HFS+ 都是 255 **字节**，不是 255 个字符 ——
/// 中文名到 85 个字就顶到头了，而那时错误信息说「太长」得说清是按字节算的。
const MAX_NAME_BYTES: usize = 255;

/// 校验一个新名字。
///
/// 单独抽出来是因为它是这批改动里唯一**能穷举**的部分：其余三个函数都要碰盘，
/// 而这里全是纯判断，可以把每条规则连同它的理由一起钉死。
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        // 全是空白的名字在文件树里就是一行空的，点不着也删不掉 —— 一定是手滑
        return Err("名字不能为空".into());
    }
    if name.contains('/') {
        // macOS 上 `/` 是路径分隔符（Finder 里显示成 `:` 是另一回事）。
        // 放它过去，「新建 a/b」就变成了往别的目录里写东西
        return Err("名字里不能有 /".into());
    }
    if name.contains('\0') {
        // 到不了系统调用就会被 Rust 挡下，但错误是英文的 NulError
        return Err("名字里不能有空字符".into());
    }
    if name == "." || name == ".." {
        return Err("不能叫 . 或 ..".into());
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(format!(
            "名字太长（上限 {MAX_NAME_BYTES} 字节，这个 {} 字节）",
            name.len()
        ));
    }
    Ok(())
}

/// 两份元数据指的是不是同一个盘上条目。
///
/// 用 dev + ino 而不是比较路径字符串：`a.txt` 和 `A.txt` 在 macOS 默认的
/// APFS 卷上是同一个文件，而在大小写敏感的卷上是两个 —— 问系统要 inode
/// 就不用先判断「这个卷敏不敏感」。
fn same_entry(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

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
    let dir = dir.as_ref();
    fs::create_dir_all(dir)?;
    for n in 1..=99u32 {
        let name = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{n}.md")
        };
        match create_entry(dir, &name, false) {
            Ok(p) => return Ok(p),
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

/// 在 `dir` 里新建一个文件或目录，返回新路径。
///
/// 撞名一律失败，**不覆盖也不复用**。
pub fn create_entry(dir: impl AsRef<Path>, name: &str, is_dir: bool) -> io::Result<PathBuf> {
    validate_name(name).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m))?;
    let path = dir.as_ref().join(name);

    // 预检查只为了给一句中文 —— 真正的保护是下面两个 API 自带的原子性。
    // 两者不能互相替代：光有预检查会被 TOCTOU 绕过，光有原子保护则会
    // 把「File exists (os error 17)」这句英文糊到状态栏里
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("这里已经有一个叫「{name}」的了"),
        ));
    }

    if is_dir {
        // `create_dir` 而不是 `create_dir_all`：后者对**已存在**的目录返回 Ok，
        // 于是「新建一个已经有的文件夹」会静悄悄地什么都不做，界面还报「已新建」
        fs::create_dir(&path)?;
    } else {
        // `create_new(true)` 而不是 `File::create`：后者对已存在的文件是
        // **截断成 0 字节**（实测见测试 `新建文件绝不截断已有文件`）。
        // 新文件手滑取成一个已有文件的名字是最容易发生的手滑，
        // 而那份内容当场就没了 —— 连废纸篓都进不了
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
    }
    Ok(path)
}

/// 在原地改名（同一个父目录），返回新路径。
///
/// 换目录的移动**不在这里做** —— 那是拖拽的事，需要的判断完全不同
/// （跨卷、目标是不是自己的子目录）。
pub fn rename_entry(path: impl AsRef<Path>, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m))?;
    let path = path.as_ref();
    let from_meta = fs::symlink_metadata(path).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} 不在盘上了", path.display()),
        )
    })?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "这个路径没有上级目录，改不了名")
    })?;
    let to = parent.join(new_name);
    if to == path {
        return Ok(to); // 名字没变，别去惊动文件系统
    }

    /*
     * `fs::rename` 在 Unix 上**静默覆盖**已存在的目标 —— 这是 rename(2)
     * 的语义，不是 Rust 的选择。少了这道检查，把 a.txt 改名成一个已有的
     * b.txt，b.txt 就没了；而且它不进废纸篓，是真的没了。
     *
     * 用 `symlink_metadata` 而不是 `to.exists()`：后者跟随符号链接，
     * 于是一个指向已删除目标的坏链接会被判成"不存在"，然后被 rename 覆盖掉 ——
     * 丢的是链接本身。这条和 reveal 那边是同一个判据。
     */
    if let Ok(to_meta) = fs::symlink_metadata(&to) {
        // 例外：只改大小写。APFS 默认大小写不敏感，a.txt → A.txt 时
        // 目标"已存在"，而存在的正是源文件自己 —— 这时候必须放行
        if !same_entry(&to_meta, &from_meta) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("这里已经有一个叫「{new_name}」的了"),
            ));
        }
    }

    fs::rename(path, &to)?;
    Ok(to)
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
pub fn discard_empty_scratch(dir: impl AsRef<Path>, path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "只丢得掉普通文件",
        ));
    }
    if meta.len() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "这份草稿里有东西，不能这么丢",
        ));
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

/// 移到废纸篓。**不做真删除** —— 除了 [`discard_empty_scratch`]
/// 那条只碰 0 字节草稿的窄路，应用里没有第二条 `remove_file`。
///
/// 走系统 API（macOS 上是 `NSFileManager` 的 `trashItemAtURL:`，由 trash crate
/// 包装）而不是自己往 `~/.Trash` 里 rename：Finder 的「放回原处」依赖一份
/// 系统维护的元数据，外部卷的废纸篓在卷自己的 `.Trashes` 里，同名冲突还要
/// 按 Finder 的规则改名。这几条规则的定义方是系统 —— 和「.gitignore 的
/// 优先级规则以 git 为准，所以起 git 子进程」是同一条判据。
pub fn move_to_trash(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    // 自己判一次存在性，理由同 reveal_in_finder：「文件已经不在了」是最常见的
    // 失败（刚在终端里删过、切了分支），而 trash 的错误是英文的
    if fs::symlink_metadata(path).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} 不在盘上了", path.display()),
        ));
    }
    trash::delete(path).map_err(|e| io::Error::other(format!("移到废纸篓失败：{e}")))
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

    /*
     * 读文件的硬顶。上限可注入是有意的 —— 真造一个 64MB 的临时文件来测
     * 太贵（跑测试的人的盘和时间都要付账），而这条闸必须被测到：
     * 它挡的是「打开时 1KB、后来涨到 2GB」那类文件从重读路径进来。
     */
    #[test]
    fn 超过上限的文件不读进内存() {
        let d = std::env::temp_dir().join("fsservice-cap");
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let f = d.join("big.txt");
        fs::write(&f, "0123456789ABCDEF").unwrap(); // 16 字节

        // 正好等于上限：要能读，不能误报
        let ok = read_capped(&f, "", 16).expect("正好等于上限应当读得出来");
        assert_eq!(ok.content, "0123456789ABCDEF");

        // 超一个字节：必须报错，而且话要说得让人知道下一步干什么
        let err = read_capped(&f, "", 15).expect_err("超限必须报错");
        let msg = err.to_string();
        assert!(msg.contains("读不下"), "错误里要说明读不下：{msg}");
        assert!(msg.contains("日志模式"), "要指出下一步该怎么办：{msg}");

        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 目录在前_同类按名称排序() {
        let d = sandbox("sort");
        fs::write(d.join("beta.txt"), "x").unwrap();
        fs::write(d.join("Alpha.txt"), "x").unwrap();
        fs::create_dir(d.join("zeta")).unwrap();
        fs::create_dir(d.join("Mid")).unwrap();

        let got: Vec<String> = list_dir(&d)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(got, vec!["Mid", "zeta", "Alpha.txt", "beta.txt"]);
        fs::remove_dir_all(d).ok();
    }

    /// 点文件要列出来，生成物目录不列。
    ///
    /// 这两件事以前共用一个 `show_hidden` 开关，于是 `.gitignore` 这类
    /// 天天要改的文件跟着 `node_modules` 一起消失了。
    ///
    /// 造目录是**按 [`excludes::GENERATED_DIRS`] 循环**，不是照抄四个名字：
    /// 往那份名单里加一个名字，这条测试自动就覆盖到了 —— 而写死名字的话，
    /// 名单长了测试却还只验老那几个，正是当初两份名单分岔的形状。
    #[test]
    fn 点文件和生成物目录都要列出来_生成物带标记() {
        let d = sandbox("hidden");
        fs::write(d.join("visible.rs"), "x").unwrap();
        fs::write(d.join(".env"), "x").unwrap();
        fs::create_dir(d.join(".github")).unwrap();
        fs::create_dir(d.join(".git")).unwrap();
        fs::create_dir(d.join("build")).unwrap();
        fs::create_dir(d.join("node_modules")).unwrap();
        // **一个叫 build 的文件不是生成物目录。** shell 脚本里这个名字很常见，
        // 而 `generated` 只该跟着「里面全是工具生成的东西」这件事走
        fs::write(d.join("target"), "#!/bin/sh\n").unwrap();

        let got: Vec<(String, bool)> = list_dir(&d)
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.generated))
            .collect();
        /*
         * 目录在前、同类不区分大小写排序 —— 点目录也照这条规矩排。
         *
         * **生成物不单独排到末尾**（issue #13）：挪位置比压暗更让人意外，
         * 而且 `target/` 一旦换了位置，「它刚才还在这儿」这种困惑比看见它更贵。
         */
        assert_eq!(
            got,
            vec![
                (".git".into(), false),
                (".github".into(), false),
                ("build".into(), true),
                ("node_modules".into(), true),
                (".env".into(), false),
                ("target".into(), false), // 它是文件
                ("visible.rs".into(), false),
            ]
        );
        fs::remove_dir_all(d).ok();
    }

    /// **保存不许改掉文件的权限位。**
    ///
    /// 「临时文件 + rename」这条路上，临时文件是新建的，带的是 umask 的默认
    /// 权限；不把原文件的权限抄过去，一个 `0755` 的脚本保存完就变成 `0644`，
    /// **当场不能执行了**，而界面还报「已保存」。
    #[test]
    fn 保存不改变文件权限() {
        use std::os::unix::fs::PermissionsExt;
        let d = sandbox("perm");
        let f = d.join("run.sh");
        write_text(&f, "#!/bin/sh\necho hi\n").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();

        write_text(&f, "#!/bin/sh\necho hi2\n").unwrap();

        let mode = fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "保存之后权限变成了 {mode:o}，这个脚本已经不能执行了");
        fs::remove_dir_all(d).ok();
    }

    /// **保存一条软链，要写进它指向的那个文件。**
    ///
    /// 不解引用的话，rename 会把软链本身换成一个普通文件：改动写在了
    /// 一个新文件里，真身一字未动，而链接没了。`~/.zshrc` 指向 dotfiles
    /// 仓库这种最常见的用法正好踩中。
    #[test]
    fn 保存写进软链指向的文件_而不是把软链换掉() {
        let d = sandbox("symlink");
        let real = d.join("real.txt");
        let link = d.join("link.txt");
        write_text(&real, "原文\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_text(&link, "改过了\n").unwrap();

        assert!(
            fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
            "软链被换成普通文件了"
        );
        assert_eq!(read_text(&real).unwrap(), "改过了\n", "改动没写进真身");
        fs::remove_dir_all(d).ok();
    }

    /// **保存不许把文件从硬链接组里摘出去。**
    ///
    /// rename 换的是目录项，原来那个 inode 还被另一个名字拿着 ——
    /// 于是「同一个文件」悄悄变成了两个，另一头再也收不到改动。
    /// 这条走的是原地覆写那条分支（判据同 vim 的 `backupcopy=auto`）。
    #[test]
    fn 保存不拆掉硬链接() {
        use std::os::unix::fs::MetadataExt;
        let d = sandbox("hardlink");
        let a = d.join("a.txt");
        let b = d.join("b.txt");
        write_text(&a, "原文\n").unwrap();
        fs::hard_link(&a, &b).unwrap();

        write_text(&a, "改过了\n").unwrap();

        assert_eq!(read_text(&b).unwrap(), "改过了\n", "另一个名字还看着旧内容，链接被拆了");
        assert_eq!(fs::metadata(&a).unwrap().nlink(), 2, "链接数掉了");
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
        let leftovers: Vec<String> = list_dir(&d)
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

    // ── reveal_in_finder ──
    //
    // 这里**不测真的弹 Finder**：那既会打断跑测试的人，也依赖 GUI 会话
    // （CI 上是没有的）。测的是两件不用起子进程就能定死的事。

    #[test]
    fn reveal的路径前面一定紧挨着双横线() {
        // 一个名叫 -Q 的文件。少了 `--`，open 会把它当开关：
        // 实测 `open -R "-Q"` → `open: invalid option -- Q`
        let a = reveal_args(Path::new("-Q"));
        assert_eq!(
            a,
            vec![
                OsString::from("-R"),
                OsString::from("--"),
                OsString::from("-Q")
            ]
        );
        // 真正要守的性质：路径是最后一个参数，而它前面紧挨着 `--`
        assert_eq!(a.last().unwrap(), &OsString::from("-Q"));
        assert_eq!(a[a.len() - 2], OsString::from("--"), "路径前必须有 --");
    }

    #[test]
    fn reveal的路径整个只占一个参数() {
        // 带空格和引号的路径不能被拆开 —— Command::arg 本来就不过 shell，
        // 这条钉的是「别哪天改成拼字符串」
        let weird = "/tmp/a b/c\"d\"/e f.txt";
        let a = reveal_args(Path::new(weird));
        assert_eq!(a.len(), 3, "参数个数必须恒为 3，路径不该被拆");
        assert_eq!(a[2], OsString::from(weird));
    }

    #[test]
    fn reveal不存在的路径不去起子进程() {
        let e = reveal_in_finder("/这个路径/根本/不存在.txt").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(
            e.to_string().contains("不在盘上了"),
            "要给一句中文，而不是把 open 的英文原样透出来，实得：{e}"
        );
    }

    #[test]
    fn reveal坏掉的符号链接仍然算存在() {
        // 链接本身在盘上，Finder 显示得出来 —— 不该被"目标不存在"拦掉。
        // 这条是 try_exists 和 symlink_metadata 的分水岭
        let d = sandbox("reveal-symlink");
        let link = d.join("断链");
        std::os::unix::fs::symlink(d.join("目标早没了"), &link).unwrap();
        assert!(!link.try_exists().unwrap(), "前提：try_exists 判它不存在");
        assert!(
            exists_for_reveal(&link),
            "但 reveal 的判定必须放它过去 —— 链接本身在盘上，Finder 显示得出来"
        );
        fs::remove_dir_all(d).ok();
    }

    // ── 新建 / 重命名 / 废纸篓 ──
    //
    // 这一批里有三条**先证明前提**再断言：std 的默认行为（截断、覆盖、复用）
    // 正是这些保护存在的理由，不把它演示一遍，读代码的人会以为
    // `create_new` 和那道存在性检查是可有可无的防御性代码。

    #[test]
    fn 名字校验挡住五类坏名字() {
        assert!(validate_name("正常.txt").is_ok());
        assert!(validate_name(" 前导空格也放行").is_ok(), "首尾空格是合法文件名，不该越权拦");

        let bad = |n: &str| validate_name(n).unwrap_err();
        assert!(bad("").contains("不能为空"));
        assert!(bad("   ").contains("不能为空"), "全空白的名字在树里就是一行空的");
        assert!(bad("a/b").contains("不能有 /"), "放它过去就是往别的目录写东西");
        assert!(bad("a\0b").contains("空字符"));
        assert!(bad(".").contains(". 或 .."));
        assert!(bad("..").contains(". 或 .."));

        // 255 是**字节**不是字符：85 个中文正好 255 字节，86 个就超
        assert!(validate_name(&"中".repeat(85)).is_ok());
        let e = bad(&"中".repeat(86));
        assert!(e.contains("258 字节"), "错误里要说清按字节算，实得：{e}");
    }

    #[test]
    fn 新建文件绝不截断已有文件() {
        let d = sandbox("create-file");

        // 前提：std 最顺手的那个写法会把已有文件截成 0 字节
        let victim = d.join("victim.txt");
        write_text(&victim, "本来有内容").unwrap();
        fs::File::create(&victim).unwrap();
        assert_eq!(
            fs::metadata(&victim).unwrap().len(),
            0,
            "前提不成立：File::create 不再截断了？那这条保护的理由要重写"
        );

        // 我们的：撞名报错，内容一个字节都不动
        let keep = d.join("keep.txt");
        write_text(&keep, "别动我").unwrap();
        let e = create_entry(&d, "keep.txt", false).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert!(e.to_string().contains("已经有一个叫"), "实得：{e}");
        assert_eq!(read_text(&keep).unwrap(), "别动我");

        // 正常路径：新文件是空的，路径带回来
        let p = create_entry(&d, "新建.java", false).unwrap();
        assert_eq!(p, d.join("新建.java"));
        assert_eq!(read_text(&p).unwrap(), "");

        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 同一分钟建第二份草稿要自己让开() {
        let d = sandbox("scratch").join("scratches");

        // 目录还不存在 —— create_scratch 得自己把它建出来，
        // 「第一次记东西」正是这条路唯一走过的一次
        assert!(!d.exists());
        let a = create_scratch(&d, "2026-09-09 1030").unwrap();
        assert_eq!(a.file_name().unwrap(), "2026-09-09 1030.md");
        assert_eq!(read_text(&a).unwrap(), "");

        // 同一分钟再来一份：不能失败，也不能覆盖掉上一份
        write_text(&a, "第一份的内容").unwrap();
        let b = create_scratch(&d, "2026-09-09 1030").unwrap();
        assert_eq!(b.file_name().unwrap(), "2026-09-09 1030-2.md");
        assert_eq!(read_text(&a).unwrap(), "第一份的内容", "第一份被盖了");

        let c = create_scratch(&d, "2026-09-09 1030").unwrap();
        assert_eq!(c.file_name().unwrap(), "2026-09-09 1030-3.md");

        fs::remove_dir_all(d.parent().unwrap()).ok();
    }

    #[test]
    fn 只丢得掉空的且在草稿目录里的东西() {
        let base = sandbox("discard");
        let d = base.join("scratches");
        let 空的 = create_scratch(&d, "2026-09-09 1030").unwrap();

        // 有内容的：一个字节都不许碰
        let 有内容 = create_scratch(&d, "2026-09-09 1031").unwrap();
        write_text(&有内容, "记了一半").unwrap();
        let e = discard_empty_scratch(&d, &有内容).unwrap_err();
        assert!(e.to_string().contains("有东西"), "实得：{e}");
        assert!(有内容.exists());

        // 草稿目录外面的：哪怕是空的也不行 ——
        // 判据是「在不在草稿目录里」，不是「是不是空的」，两条都要
        let 外面 = base.join("别人的空文件.md");
        fs::write(&外面, "").unwrap();
        let e = discard_empty_scratch(&d, &外面).unwrap_err();
        assert!(e.to_string().contains("不在草稿目录"), "实得：{e}");
        assert!(外面.exists());

        // 用 .. 拼出来的也一样：canonicalize 之后就露馅了
        let 绕路 = d.join("../别人的空文件.md");
        assert!(discard_empty_scratch(&d, &绕路).is_err());
        assert!(外面.exists());

        // 目录不行
        assert!(discard_empty_scratch(&d, &d).is_err());

        /*
         * **符号链接：绝不能顺着它去删别人。**
         *
         * 这条是 `symlink_metadata`（而不是 `metadata`）唯一的存在理由。
         * 改成跟随链接的 `metadata` 之后：链接的 `is_file()` 变成 true、
         * 跟随后的大小是 0、`canonicalize` 又把它解析回草稿目录里 ——
         * 三条判据全过，`remove_file` 删掉的是**链接指向的那个文件**，
         * 而链接本身还在。测这一条才拦得住那次改动。
         */
        let 目标 = create_scratch(&d, "2026-09-09 1032").unwrap();
        let 链接 = d.join("链接.md");
        std::os::unix::fs::symlink(&目标, &链接).unwrap();
        assert!(discard_empty_scratch(&d, &链接).is_err());
        assert!(目标.exists(), "顺着符号链接把目标文件删了");

        // 正常路径：真的没了
        discard_empty_scratch(&d, &空的).unwrap();
        assert!(!空的.exists());

        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn 新建文件夹撞名要报错而不是复用() {
        let d = sandbox("create-dir");
        let sub = d.join("已有目录");
        fs::create_dir(&sub).unwrap();

        // 前提：create_dir_all 对已存在的目录返回 Ok —— 用它就会「新建成功」
        // 一个早就存在的目录，界面报了「已新建」而盘上什么都没发生
        assert!(fs::create_dir_all(&sub).is_ok(), "前提不成立");

        let e = create_entry(&d, "已有目录", true).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);

        let p = create_entry(&d, "新目录", true).unwrap();
        assert!(p.is_dir());
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 新建撞的是一个文件还是目录都要拦() {
        // 撞名检查看的是「这个名字被占了没有」，不该只在同类之间比
        let d = sandbox("create-cross");
        fs::create_dir(d.join("x")).unwrap();
        // 断言的是**我们自己那句中文**，不只是 is_err()：std 对跨类型撞名
        // 本来就会 EEXIST，只断言"报错了"的话，把预检查改成只在同类之间比
        // 也照样绿（试过）
        let e = create_entry(&d, "x", false).unwrap_err();
        assert!(e.to_string().contains("已经有一个叫"), "同名目录占着，实得：{e}");

        write_text(d.join("y"), "内容").unwrap();
        let e = create_entry(&d, "y", true).unwrap_err();
        assert!(e.to_string().contains("已经有一个叫"), "同名文件占着，实得：{e}");
        assert_eq!(read_text(d.join("y")).unwrap(), "内容");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 改名不覆盖已存在的目标() {
        let d = sandbox("rename-clobber");

        // 前提：fs::rename 静默覆盖 —— 这是 rename(2) 的语义，不是 Rust 的选择
        let a = d.join("a0.txt");
        let b = d.join("b0.txt");
        write_text(&a, "源").unwrap();
        write_text(&b, "本来的 b").unwrap();
        fs::rename(&a, &b).unwrap();
        assert_eq!(read_text(&b).unwrap(), "源", "前提不成立：rename 不再覆盖了？");
        assert!(!a.exists());

        // 我们的：拦住，两边内容都不变
        let x = d.join("x.txt");
        let y = d.join("y.txt");
        write_text(&x, "我是 x").unwrap();
        write_text(&y, "我是 y").unwrap();
        let e = rename_entry(&x, "y.txt").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(read_text(&x).unwrap(), "我是 x");
        assert_eq!(read_text(&y).unwrap(), "我是 y", "被覆盖的话这里就是「我是 x」");

        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 改名之后旧路径没了新路径在() {
        let d = sandbox("rename-ok");
        let a = d.join("旧名.java");
        write_text(&a, "内容").unwrap();

        let to = rename_entry(&a, "新名.java").unwrap();
        assert_eq!(to, d.join("新名.java"));
        assert!(!a.exists(), "旧路径还在");
        assert_eq!(read_text(&to).unwrap(), "内容", "改名不该动内容");

        // 名字没变：直接返回，不去惊动文件系统
        let same = rename_entry(&to, "新名.java").unwrap();
        assert_eq!(same, to);
        assert_eq!(read_text(&to).unwrap(), "内容");

        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 只改大小写要能成功() {
        // APFS 默认大小写不敏感：a.txt → A.txt 时目标"已存在"，
        // 而存在的正是源文件自己。靠 dev+ino 认出这一点，不然这条永远失败 ——
        // 而「把 readme.md 改成 README.md」是真实需求
        let d = sandbox("rename-case");
        let a = d.join("readme.md");
        write_text(&a, "内容").unwrap();

        let to = rename_entry(&a, "README.md").unwrap();
        assert_eq!(to, d.join("README.md"));

        let names: Vec<String> = list_dir(&d).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["README.md"], "盘上的名字没跟着换大小写");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 改名的目标是坏软链也不许覆盖() {
        // to.exists() 会跟随软链，于是一个指向已删除目标的坏链接被判成
        // "不存在"，然后 rename 把链接本身覆盖掉。这条是 exists 和
        // symlink_metadata 的分水岭，和 reveal 那边同一个判据
        let d = sandbox("rename-symlink");
        let src = d.join("src.txt");
        write_text(&src, "源").unwrap();
        let link = d.join("断链");
        std::os::unix::fs::symlink(d.join("目标早没了"), &link).unwrap();
        assert!(!link.try_exists().unwrap(), "前提：try_exists 判它不存在");

        let e = rename_entry(&src, "断链").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink(), "链接被覆盖掉了");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 改名先校验名字() {
        let d = sandbox("rename-name");
        let a = d.join("a.txt");
        write_text(&a, "x").unwrap();
        // 带斜杠的名字要在碰盘之前就被挡下，否则就是「改名」变成「移动到别处」
        let e = rename_entry(&a, "../跑出去.txt").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        assert!(a.exists());
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 改不存在的东西要报中文() {
        let d = sandbox("rename-missing");
        let e = rename_entry(d.join("没有这个.txt"), "新名").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(e.to_string().contains("不在盘上了"), "实得：{e}");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 废纸篓拒绝不存在的路径() {
        // 和 reveal 一样：自己判一次给一句中文，而不是把 trash 的英文透出来
        let e = move_to_trash("/这个路径/根本/不存在.txt").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(e.to_string().contains("不在盘上了"), "实得：{e}");
    }

    /// **默认不跑**：它会往跑测试的人的废纸篓里真的扔一个文件进去。
    ///
    /// 但这条路必须有人验过 —— 上面那条只测了守卫，一行 `trash::delete`
    /// 换成 `Ok(())` 它照样绿。手动跑：
    ///
    /// ```bash
    /// cargo test -p fsservice -- --ignored 真的把文件移进废纸篓
    /// ```
    #[test]
    #[ignore = "会往用户的废纸篓里扔文件"]
    fn 真的把文件移进废纸篓() {
        let d = sandbox("trash-real");
        let f = d.join("lite-ide-废纸篓测试.txt");
        write_text(&f, "这个文件应该出现在废纸篓里").unwrap();

        move_to_trash(&f).unwrap();
        assert!(
            fs::symlink_metadata(&f).is_err(),
            "文件还在原处 —— trash::delete 什么都没做"
        );
        fs::remove_dir_all(d).ok();
    }
}
