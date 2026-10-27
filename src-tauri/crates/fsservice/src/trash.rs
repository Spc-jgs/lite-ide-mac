//! 删用户的文件只走这里：交给 Finder 扔进废纸篓（rust.md「删除只走废纸篓」）

use super::*;

// 外部的 trash crate 一律写 `::trash::` —— 这个模块自己也叫 trash，不带前导 `::` 的 `trash::delete` 找的是这个模块自己

/// 移到废纸篓。**不做真删除** —— 除了 [`discard_empty_scratch`]
/// 那条只碰 0 字节草稿的窄路，应用里没有第二条 `remove_file`。
///
/// 交给 Finder 去扔（trash crate 在 macOS 上的默认做法：起一个 `osascript`，
/// `tell application "Finder" to delete …`），而不是自己往 `~/.Trash` 里 rename：
/// Finder 的「放回原处」依赖一份它自己维护的元数据，外部卷的废纸篓在卷自己的
/// `.Trashes` 里，同名冲突还要按 Finder 的规则改名。这几条规则的定义方是 Finder ——
/// 和「.gitignore 的优先级规则以 git 为准，所以起 git 子进程」是同一条判据。
///
/// 代价（crate 自己的文档列的）：可能要用户给一次「控制 Finder」的权限、有 Finder 的音效、慢一点。
/// 另一条路是 `DeleteMethod::NsFileManager`（`trashItemAtURL:`）：没有这三样，但 crate 说有的系统上
/// 「放回原处」不出现。**留着 Finder 是有意的**（2026-10-12）—— 这里原来写的是「走 `NSFileManager`」，
/// 和实际做的相反，读 crate 源码才发现
pub fn move_to_trash(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    // 自己判一次存在性，理由同 reveal_in_finder：「文件已经不在了」是最常见的
    // 失败（刚在终端里删过、切了分支），而 trash 的错误是英文的
    if fs::symlink_metadata(path).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} 已不存在", path.display()),
        ));
    }
    ::trash::delete(path).map_err(|e| io::Error::other(format!("移到废纸篓失败：{e}")))
}

/// 一次扔一批（丢弃一堆未跟踪的文件）。走 Finder 每次要起一个 `osascript`，实测和 Finder 往返一趟约 0.11 秒 ——
/// 一个一个扔，50 个文件就是 5 秒多；`trash::delete_all` 一次 AppleScript 扔完。
///
/// 已经不在盘上的跳过（要的就是它不在了）。整批失败（Finder 不让、其中一个扔不动）时**逐个重试**，
/// 把真没扔掉的那几个连同原因报出来 —— 只报一句「失败了」的话，用户不知道哪几个还在
pub fn move_all_to_trash(paths: &[PathBuf]) -> Result<(), String> {
    let present: Vec<&PathBuf> = paths.iter().filter(|p| fs::symlink_metadata(p).is_ok()).collect();
    if present.is_empty() || ::trash::delete_all(&present).is_ok() {
        return Ok(());
    }
    let failed: Vec<String> = present
        .iter()
        .filter(|p| fs::symlink_metadata(p).is_ok())
        .filter_map(|p| move_to_trash(p).err().map(|e| format!("{}（{e}）", p.display())))
        .collect();
    match failed.len() {
        0 => Ok(()),
        n if n <= 5 => Err(format!("未能移到废纸篓：{}", failed.join("；"))),
        n => Err(format!("{n} 个项目未能移到废纸篓，例如：{}", failed[..5].join("；"))),
    }
}
