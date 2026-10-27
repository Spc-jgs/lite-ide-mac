//! 文件树上的新建、改名、移动：名字校验，目标已经在就拒（rust.md「改盘的 std API 默认都会吃掉已有文件」）

use super::*;

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
#[allow(clippy::disallowed_methods)] // 改名前查过目标不在（大小写改名按 inode 判，rust.md「改盘的 std API」）
pub fn rename_entry(path: impl AsRef<Path>, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m))?;
    let path = path.as_ref();
    let from_meta = fs::symlink_metadata(path).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} 已不存在", path.display()),
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

/// 把 `path` 挪进 `dest_dir`（文件树里拖拽，issue #33 ⑨）。名字不变，返回新路径。
///
/// 和 [`rename_entry`] 同一套判据：目标已存在就拒绝（`fs::rename` 会静默覆盖），
/// 用 `symlink_metadata` 不跟随链接。另外两条这里独有：
///
/// - **不能把目录挪进它自己（或它的子目录）**：`rename(2)` 对这个报 EINVAL，
///   但那句英文没人看得懂，先自己判。
/// - 目标目录必须真的是目录。
///
/// 跨卷的 `rename` 会失败（EXDEV）—— 项目里拖来拖去都在同一个卷上，
/// 真撞上了报 git 那句原话就够，不做复制 + 删除的兜底：那条路中途失败会留下两份。
#[allow(clippy::disallowed_methods)] // 挪之前查过目标不在（rust.md「改盘的 std API」）
pub fn move_entry(path: impl AsRef<Path>, dest_dir: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = path.as_ref();
    let dest = dest_dir.as_ref();
    let from_meta = fs::symlink_metadata(path)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, format!("{} 已不存在", path.display())))?;
    if !dest.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} 不是目录", dest.display())));
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "这个路径没有名字，挪不了"))?;
    let to = dest.join(name);
    if to == path {
        return Ok(to); // 拖到了它原来的目录里
    }
    if from_meta.is_dir() && dest.starts_with(path) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "不能把目录移到它自己里面"));
    }
    if fs::symlink_metadata(&to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} 里已经有一个叫「{}」的了", dest.display(), name.to_string_lossy()),
        ));
    }
    fs::rename(path, &to)?;
    Ok(to)
}
