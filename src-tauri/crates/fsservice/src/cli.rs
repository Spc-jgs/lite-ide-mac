//! 安装 `lite` 命令：脚本写进应用数据目录、在 /usr/local/bin 做软链（只换我们自己的）

use super::*;

/// 从可执行文件的路径推出 `.app` 包的路径：`…/x.app/Contents/MacOS/x` → `…/x.app`。
/// 不在 `.app` 里（`cargo run`、直接跑 `target/release/lite-ide`）就是 `None` ——
/// 那种构建是开发用的，往 PATH 里装一个指向它的命令只会在下次 `cargo clean` 后断掉。
pub fn bundle_from_exe(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let ok = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    ok.then(|| app.to_path_buf())
}

/// 我们生成的 `lite` 脚本头上的那行。认「那个软链是不是我们的」时也靠它（以前装在别处的脚本）
pub const CLI_MARK: &str = "# lite-ide 的命令行入口";

/// `lite` 脚本的内容。
///
/// **走 `open -a`，不软链到二进制。** macOS 上「打开文件」是 Apple Event
/// （`odoc`），直接 exec 二进制只在冷启动时把 `argv` 送到；应用已经开着时
/// 会再起一个进程 —— 两个 lite-ide。`open -a` 让 Launch Services 决定
/// 发给已在运行的那个还是新起一个，冷热都对。USAGE 原来教的是软链，那是错的。
pub fn cli_script(app: &Path) -> String {
    // 路径里的 `"` 和 `$` 转义掉 —— .app 放在什么目录是用户定的
    let quoted = app
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");
    format!(
        "#!/bin/sh\n\
         {CLI_MARK}，由「文件 → 安装命令行工具…」生成。\n\
         # 走 open -a 而不是直接跑二进制：macOS 上「打开文件」是 Apple Event，\n\
         # 直接跑只在冷启动时有效，应用已经开着时会再起一个进程。\n\
         exec /usr/bin/open -a \"{quoted}\" \"$@\"\n"
    )
}

/// 安装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliInstall {
    /// 脚本真身落在哪（应用自己的数据目录，永远可写）
    pub script: PathBuf,
    /// 脚本原来就有，这次是覆盖
    pub replaced: bool,
    /// `/usr/local/bin/lite` 那头怎么样了
    pub link: CliLink,
}

/// `/usr/local/bin/lite` 那个位置的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliLink {
    /// 软链装上了，指向我们的脚本
    Linked,
    /// 没装上：那个目录写不进去（多半要 sudo，也可能目录不在）。`ours` = 那儿原来是我们的旧软链 ——
    /// 补的时候要 `-f` 换掉它；不是的话那儿什么都没有，`ln -s` 就够，**不给 `-f`**
    NoPermission { ours: bool },
    /// 那儿是别人的东西，没碰。说的是它是什么（给人看：「一个文件」「一个指向 /opt/x/lite 的软链」）
    Taken(String),
}

impl CliLink {
    /// 没装上时让人自己补的那一句。**被别人占着时没有** —— 原来这里一律给 `sudo ln -sf`，而 `-f` 会把
    /// install_cli 刚刚小心避开的那个用户文件当场盖掉：代码里的保护被我们自己给的命令绕过去了（2026-10-12）
    pub fn fix_cmd(&self, script: &Path, link: &Path) -> Option<String> {
        match self {
            CliLink::NoPermission { ours } => {
                Some(format!("sudo ln -s{} \"{}\" {}", if *ours { "f" } else { "" }, script.display(), link.display()))
            }
            _ => None,
        }
    }
}

/// `link` 处那个软链是不是我们放的：指向现在的脚本；指向某个 `.app` 里的 lite-ide 二进制（2026-09-15 之前 USAGE 教的装法）；
/// 或者指向的文件头上有 [`CLI_MARK`]（以前装在别处的脚本）。都不是 → 别人的，不碰
fn cli_link_is_ours(target: &Path, script: &Path) -> bool {
    if target == script || target.to_string_lossy().ends_with(".app/Contents/MacOS/lite-ide") {
        return true;
    }
    let mut head = [0u8; 512];
    let n = fs::File::open(target).and_then(|mut f| io::Read::read(&mut f, &mut head)).unwrap_or(0);
    String::from_utf8_lossy(&head[..n]).contains(CLI_MARK)
}

/// 把脚本写到 `script`（覆盖），再在 `link` 处做一个指向它的软链。
///
/// 分两步而不是直接往 `/usr/local/bin` 里写文件：那个目录常常要 sudo，
/// 而脚本真身放在应用自己的目录里**永远写得进去** —— 软链装不上时只差
/// 一句 `sudo ln -s`，比「装失败了，自己想办法」有用得多。
///
/// `link` 处已经有东西时：**是我们的软链才换**（`.app` 挪过位置之后要重装）；别的 —— 真文件、目录、
/// 指向别的程序的软链 —— 一概不碰，报 [`CliLink::Taken`]。原来是「是软链就换」，不看它指向哪：
/// 用户自己的 `lite` 要是一个指向别的工具的软链，会被盖掉（2026-10-12 标 clippy 的 allow 时看到的）
#[allow(clippy::disallowed_methods)] // 脚本真身在应用自己的目录里，覆盖是要的；link 处只换我们自己的软链
pub fn install_cli(script: &Path, content: &str, link: &Path) -> io::Result<CliInstall> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = script.parent() {
        fs::create_dir_all(dir)?;
    }
    let replaced = fs::symlink_metadata(script).is_ok();
    fs::write(script, content)?;
    fs::set_permissions(script, fs::Permissions::from_mode(0o755))?;

    let link_state = match fs::symlink_metadata(link) {
        Ok(m) if m.file_type().is_symlink() => match fs::read_link(link) {
            Ok(t) => {
                // 相对的指向按软链所在的目录解
                let t = if t.is_relative() { link.parent().map_or(t.clone(), |p| p.join(&t)) } else { t };
                if cli_link_is_ours(&t, script) {
                    match fs::remove_file(link).and_then(|_| std::os::unix::fs::symlink(script, link)) {
                        Ok(()) => CliLink::Linked,
                        Err(_) => CliLink::NoPermission { ours: true },
                    }
                } else {
                    CliLink::Taken(format!("一个指向 {} 的软链", t.display()))
                }
            }
            Err(_) => CliLink::Taken("一个读不出指向的软链".into()),
        },
        Ok(m) if m.is_dir() => CliLink::Taken("一个目录".into()),
        Ok(_) => CliLink::Taken("一个文件".into()),
        Err(_) => match std::os::unix::fs::symlink(script, link) {
            Ok(()) => CliLink::Linked,
            Err(_) => CliLink::NoPermission { ours: false },
        },
    };
    Ok(CliInstall { script: script.to_path_buf(), replaced, link: link_state })
}
