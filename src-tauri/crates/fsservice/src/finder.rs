//! 在 Finder 中显示

use super::*;

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
pub(crate) fn reveal_args(path: &Path) -> Vec<OsString> {
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
pub(crate) fn exists_for_reveal(path: &Path) -> bool {
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
            format!("{} 已不存在", path.display()),
        ));
    }

    // 绝对路径而不是靠 PATH：`open` 在 macOS 上固定在这儿，
    // 而从终端启动 lite-ide 时 PATH 是用户的，不该让它决定我们调到哪个 open
    #[allow(clippy::disallowed_methods)] // 只要退出码：stdin 接空、不读输出，procutil 防的几样都碰不上
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

// ─────────────────── 剪贴板 ───────────────────
