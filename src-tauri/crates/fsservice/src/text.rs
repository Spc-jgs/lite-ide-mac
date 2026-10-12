//! 读写文本：按上限读、编码探测与解码、保存（软链解开、硬链接原地写、其余临时文件再改名）

use super::*;

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
pub(crate) fn read_capped(path: &Path, label: &str, cap: u64) -> io::Result<encoding::Decoded> {
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
    Ok(decode_bytes(&bytes, label))
}

/// 已经读进来的字节 → 文本：探编码（`label` 非空就按它）、记下换行符、统一成 `\n`。和 [`read_text_detect`] 同一套。
///
/// 拆出来给跨文件替换用（#42）：它要**只读一次**就同时拿到原始字节（进替换日志，撤销时一个字节不差地写回）
/// 和解码后的文本（拿去匹配）—— 读两次的话，文件可能正好在两次之间被改，日志里存的就不是被替换的那一份
pub fn decode_bytes(bytes: &[u8], label: &str) -> encoding::Decoded {
    let mut d = if label.is_empty() {
        encoding::decode(bytes)
    } else {
        encoding::decode_as(bytes, label)
    };
    // 换行符和编码同一条规矩：读进来记住是什么，前端只见 \n，写回去换回来（eol.rs）
    d.eol = eol::detect(&d.content);
    d.content = eol::normalize(d.content);
    d
}

/// 文本 → 要写进盘的字节：换行符换回原样、按原编码和 BOM 编回去。[`write_text_as`] 写的就是它
pub fn encode_text(content: &str, label: &str, bom: bool, line_ending: eol::Eol) -> Vec<u8> {
    encoding::encode(&eol::denormalize(content, line_ending), label, bom)
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
    line_ending: eol::Eol,
) -> io::Result<()> {
    prepare_text_as(path, content, label, bom, line_ending)?.commit()
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
    prepare_bytes(path, bytes)?.commit()
}

/// 准备好、还没换上去的一次写盘。**写盘拆成两半是给跨文件替换用的**（#42，docs/REPLACE.md 12.2）：
/// 先把所有文件的新内容都写成临时文件（这一步失败，一个原文件都没动），全部准备好了再逐个换上。
/// 普通保存还是「准备完马上提交」（[`write_bytes`]），三条保存语义（权限位、软链写真身、硬链接原地写）
/// 都在准备这一半里，两种用法拿到的是同一套。
///
/// 没提交就丢掉（`drop`）会把临时文件删掉 —— 准备了一半出错返回时，不留残留。
pub struct Prepared {
    target: PathBuf,
    how: How,
}

enum How {
    /// 临时文件已经写好、fsync、抄好权限，提交时 rename 过去。`None` = 已经提交或放弃
    Rename { tmp: Option<PathBuf> },
    /// 硬链接组里的一份：rename 会把它摘出去，只能原地覆写 —— **而原地覆写没有「先准备好」这回事**，
    /// 字节先拿着，提交时才写。替换把这一类排在提交段最后，中途崩溃靠替换日志恢复
    InPlace { bytes: Vec<u8> },
}

impl Prepared {
    /// 真正会被写的那个文件（软链已经解开）
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// 提交时是原地覆写（硬链接）—— 替换据此把它排到最后
    pub fn in_place(&self) -> bool {
        matches!(self.how, How::InPlace { .. })
    }

    #[allow(clippy::disallowed_methods)] // 保存的最后一步：自己的临时文件改名盖过目标（保存本来就是要覆盖它），失败收掉临时文件
    pub fn commit(mut self) -> io::Result<()> {
        match &mut self.how {
            How::InPlace { bytes } => write_in_place(&self.target, bytes),
            How::Rename { tmp } => {
                let Some(t) = tmp.take() else { return Ok(()) };
                // rename 在同一文件系统内是原子的
                match fs::rename(&t, &self.target) {
                    Ok(()) => {
                        // 目录项也要落盘，否则掉电后可能连改名都没发生
                        if let Some(dir) = self.target.parent() {
                            if let Ok(d) = fs::File::open(dir) {
                                let _ = d.sync_all();
                            }
                        }
                        Ok(())
                    }
                    Err(e) => {
                        let _ = fs::remove_file(&t);
                        Err(e)
                    }
                }
            }
        }
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let How::Rename { tmp: Some(t) } = &self.how {
            #[allow(clippy::disallowed_methods)] // 自己的临时文件
            let _ = fs::remove_file(t);
        }
    }
}

/// 这个文件写盘时用的临时文件名。名字是可预测的 —— 崩溃恢复靠它找到上次留下的残留（替换日志里只记目标文件）
pub fn tmp_path_for(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!(".{}.lite-ide-tmp", target.file_name().unwrap_or_default().to_string_lossy()))
}

/// 准备写一份字节：解软链、写临时文件、fsync、抄权限；硬链接只记下字节（见 [`How::InPlace`]）
pub fn prepare_bytes(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<Prepared> {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;

    let target = resolve_link(path.as_ref())?;
    let meta = fs::symlink_metadata(&target).ok();

    // 硬链接组里的一份：rename 会把它摘出去，只能原地覆写
    if meta.as_ref().is_some_and(|m| m.nlink() > 1) {
        return Ok(Prepared { target, how: How::InPlace { bytes: bytes.to_vec() } });
    }

    let tmp = tmp_path_for(&target);

    // `create_new` 而不是 `fs::write`：临时文件名是可预测的，而
    // `fs::write` 对一条**已经在那儿的软链**会顺着它写到别处去。
    // 撞上了就先删掉再建（多半是上次崩在中间留下的残留）。
    let mut f = match fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            #[allow(clippy::disallowed_methods)] // 自己的临时文件（上次崩在中间留下的）
            fs::remove_file(&tmp)?;
            fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?
        }
        Err(e) => return Err(e),
    };
    // 临时文件建出来了就归 Prepared 管：下面任何一步出错返回，drop 会把它删掉
    // （原来出错时留着一个 `.x.lite-ide-tmp` 在用户的目录里）
    let p = Prepared { target, how: How::Rename { tmp: Some(tmp.clone()) } };
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
    Ok(p)
}

/// 准备写一份文本：按原来的编码、BOM、换行符编回字节，再走 [`prepare_bytes`]
pub fn prepare_text_as(
    path: impl AsRef<Path>,
    content: &str,
    label: &str,
    bom: bool,
    line_ending: eol::Eol,
) -> io::Result<Prepared> {
    prepare_bytes(path, &encode_text(content, label, bom, line_ending))
}
