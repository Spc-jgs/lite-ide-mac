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

/// 移到废纸篓。**不做真删除** —— 整个应用里没有任何一条路会 `remove_file`。
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

        let names: Vec<String> = list_dir(&d, false).unwrap().into_iter().map(|e| e.name).collect();
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
