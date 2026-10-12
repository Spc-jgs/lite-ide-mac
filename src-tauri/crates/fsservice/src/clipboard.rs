//! 读剪贴板（pbpaste，有上限）

use super::*;

/// 剪贴板最多读这么多。超过就**报错不粘**：静默截断等于往代码里插半截东西。
/// 8 MB 远超手动复制的正常量；真要粘那么大的，⌘V 那条路不经过这里（WebView 自己处理）。
pub const CLIPBOARD_MAX_BYTES: usize = 8 * 1024 * 1024;

/// 剪贴板里的纯文本 —— 编辑器右键菜单的「粘贴」用。
///
/// # 为什么不在前端 `navigator.clipboard.readText()`
///
/// WKWebView 对网页**主动读**剪贴板每次都要人再确认：点了菜单里的「粘贴」之后还会弹一个系统的
/// 「Paste」气泡，得再点一下（2026-09-23 真机验的）。⌘V 走的是浏览器交给页面的粘贴事件，
/// 不经过这道检查 —— 所以只有右键菜单这条路要绕到本地进程来读。
///
/// # 子进程纪律（rust.md）
///
/// - 绝对路径 `/usr/bin/pbpaste`，不靠 PATH（同 `reveal_in_finder`）
/// - stdin 接空、stderr 丢掉：绝不卡住等输入，也不让一条没人读的 stderr 管道写满
/// - **输出有上限**（`CLIPBOARD_MAX_BYTES`），多读一个字节来分辨「正好满」和「还有」
/// - **显式给 UTF-8 locale**：从 Finder 启动的 .app 没有 `LANG`，这时 pbpaste 按系统的旧编码吐 ——
///   简体中文系统上是 GB18030（「订单」出来是 `B6 A9 B5 A5`，2026-09-23 实测），
///   下面的 `from_utf8` 就会报「不是 UTF-8」
pub fn clipboard_text() -> io::Result<String> {
    read_text_capped(pbpaste_cmd(), CLIPBOARD_MAX_BYTES)
}

/// 单拎出来给测试看它带没带 locale —— 真去读剪贴板的测试会冲掉跑测试那个人的剪贴板
pub(crate) fn pbpaste_cmd() -> Command {
    let mut cmd = Command::new("/usr/bin/pbpaste");
    cmd.args(["-Prefer", "txt"])
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8");
    cmd
}

/// 跑一个命令、读它的 stdout 当 UTF-8 文本，最多 `cap` 字节；超了就杀掉并报错。
/// 单拎出来是为了能用普通命令测上限那几条（剪贴板在 CI 上不一定有东西）。
pub(crate) fn read_text_capped(mut cmd: Command, cap: usize) -> io::Result<String> {
    // 超上限就杀掉、收尸（procutil：stdin 接空、stderr 并发排空 —— 原来 stderr 接的是 null，现在留一点给报错用不上也无妨）
    let c = procutil::run_capped(&mut cmd, cap, procutil::ERR_CAP, procutil::Overflow::Kill)?;
    if c.truncated {
        return Err(io::Error::other(format!(
            "剪贴板内容超过 {} MB，没有粘贴 —— 用 ⌘V",
            cap / 1024 / 1024
        )));
    }
    if !c.status.success() {
        return Err(io::Error::other(format!(
            "读不到剪贴板（退出码 {}）",
            c.status.code().map_or_else(|| "被信号中断".to_string(), |c| c.to_string())
        )));
    }
    String::from_utf8(c.stdout).map_err(|_| io::Error::other("剪贴板里的文字不是 UTF-8"))
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
