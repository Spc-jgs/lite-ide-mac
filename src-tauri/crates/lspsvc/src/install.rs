//! 把 jdtls 装进应用数据目录（docs/IDEA.md 第 3 节第 4 条：第一次用时下载，先问、说清多大从哪来、校验 sha256）。
//!
//! 为什么是这个样子（IDEA.md 第 10 节有全文）：
//!
//! - **版本和校验和钉死在代码里**（[`DIST`]）：下来的东西对不上就不装。换版本 = 下一个发行版改这一处，下到新目录。
//! - **一个版本一个目录**（`<base>/1.61.0/`）：新的装好之前旧的不动，下到一半的不会盖掉能用的。
//! - **装是原子的**：下到临时文件 → 校验 → 解到 `<版本>.partial` → 认一遍目录结构 → 改名成 `<版本>`。哪一步死了，都不会留下
//!   「看着完整、其实缺东西」的目录；下次从头来，残留的 `.partial` / `.download` 先清掉（那是我们自己生成的文件名，只清这两种）。
//! - **下载、校验、解包都用系统自带的** `/usr/bin/curl`、`shasum`、`tar`（绝对路径：从 Finder 起的应用 PATH 里不一定有 Homebrew 的），
//!   不加依赖；走 procutil（输出设闸、丢掉就收尸）。

use crate::jdtls;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 一个钉死的 jdtls 发行包
pub struct Dist {
    pub version: &'static str,
    pub url: &'static str,
    /// 官方发布页给的 `.sha256`（下载时一起核过，IDEA.md 第 7 节）
    pub sha256: &'static str,
    /// 字节数：进度按它算，下完先比它（比 sha256 便宜，断在半截的先在这儿露馅）
    pub size: u64,
}

/// 现在用的这一版。选 1.61.0 不选当时最新的 1.62.0：后者发布才一天，没人替我们试过（IDEA.md 第 7 节）
pub const DIST: Dist = Dist {
    version: "1.61.0",
    url: "https://download.eclipse.org/jdtls/milestones/1.61.0/jdt-language-server-1.61.0-202609031315.tar.gz",
    sha256: "338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64",
    size: 51_037_522,
};

/// 装好了没有：`<base>/<版本>` 认得出是 jdtls（[`jdtls::layout`]）
pub fn installed(base: &Path, d: &Dist) -> Option<PathBuf> {
    let dir = base.join(d.version);
    jdtls::layout(&dir).ok().map(|_| dir)
}

/// 下载、校验、装好，返回装好的目录。已经装好就直接返回。
/// `url` 平常就是 `d.url`；测试传 `file://` 的（不上网）。`progress(已下, 总共)` 大约 5 次一秒；`cancel` 置位就停（杀掉 curl、清掉临时文件）
pub fn install(d: &Dist, url: &str, base: &Path, progress: impl Fn(u64, u64) + Sync, cancel: &AtomicBool) -> Result<PathBuf, String> {
    if let Some(dir) = installed(base, d) {
        return Ok(dir);
    }
    std::fs::create_dir_all(base).map_err(|e| format!("建不了 {}：{e}", base.display()))?;
    let file = base.join(format!("{}.download", d.version));
    let partial = base.join(format!("{}.partial", d.version));
    let fin = base.join(d.version);
    clear(&file);
    clear(&partial);
    let r = (|| {
        download(url, &file, d.size, &progress, cancel)?;
        verify(&file, d)?;
        unpack(&file, &partial)?;
        jdtls::layout(&partial).map_err(|e| format!("解开之后不像 jdtls：{e}"))?;
        if fin.symlink_metadata().is_ok() {
            // 认不出来的旧目录（上一次装坏了、或者别人放进来的）：不覆盖，说出来
            return Err(format!("{} 已经在了但不是能用的 jdtls，先把它挪走", fin.display()));
        }
        #[allow(clippy::disallowed_methods)] // 改名自己的临时目录，上一行确认过目标不在
        std::fs::rename(&partial, &fin).map_err(|e| format!("装不进 {}：{e}", fin.display()))?;
        Ok(fin.clone())
    })();
    clear(&file);
    if r.is_err() {
        clear(&partial);
    }
    r
}

/// 删我们自己生成的那两种（`<版本>.download` / `<版本>.partial`）。别的一概不碰
fn clear(p: &Path) {
    let Some(name) = p.file_name().and_then(|n| n.to_str()) else { return };
    if !(name.ends_with(".download") || name.ends_with(".partial")) {
        return;
    }
    let Ok(m) = p.symlink_metadata() else { return };
    #[allow(clippy::disallowed_methods)] // 自己生成的临时文件 / 目录，文件名在上面卡死了
    let _ = if m.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
}

fn download(url: &str, to: &Path, size: u64, progress: &(impl Fn(u64, u64) + Sync), cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("取消了".into());
    }
    // -f：HTTP 错误码当失败（不然 404 页面会被存下来，到校验那一步才露馅）；--proto：只许 https（和测试用的 file），重定向也不许降成 http
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args(["-fsSL", "--proto", "=https,file", "--proto-redir", "=https", "--connect-timeout", "20", "--max-time", "1800", "-o"])
        .arg(to)
        .arg(url);
    let run = procutil::spawn(&mut cmd, procutil::ERR_CAP).map_err(|e| format!("起不了 curl：{e}"))?;
    let pid = run.id() as i32;
    // 进度和取消在另一条线程上看：这条线程要在 finish 里等 curl 退出
    let done = Arc::new(AtomicBool::new(false));
    let killed = Arc::new(AtomicBool::new(false));
    std::thread::scope(|s| {
        let (done2, killed2) = (Arc::clone(&done), Arc::clone(&killed));
        s.spawn(move || {
            while !done2.load(Ordering::SeqCst) {
                progress(std::fs::metadata(to).map(|m| m.len()).unwrap_or(0), size);
                if cancel.load(Ordering::SeqCst) && !killed2.swap(true, Ordering::SeqCst) {
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        let fin = run.finish();
        done.store(true, Ordering::SeqCst);
        if killed.load(Ordering::SeqCst) {
            return Err("取消了".to_string());
        }
        let fin = fin.map_err(|e| format!("等不到 curl：{e}"))?;
        if !fin.status.success() {
            let why = String::from_utf8_lossy(&fin.stderr).trim().to_string();
            return Err(format!("下载失败（{}）：{}", fin.status, if why.is_empty() { "curl 没说原因".into() } else { why }));
        }
        Ok(())
    })?;
    let got = std::fs::metadata(to).map(|m| m.len()).unwrap_or(0);
    progress(got, size);
    if got != size {
        return Err(format!("下载不完整：{got} / {size} 字节"));
    }
    Ok(())
}

fn verify(file: &Path, d: &Dist) -> Result<(), String> {
    let mut cmd = Command::new("/usr/bin/shasum");
    cmd.args(["-a", "256"]).arg(file);
    let out = procutil::run_capped(&mut cmd, 4096, procutil::ERR_CAP, procutil::Overflow::Kill).map_err(|e| format!("算不了校验和：{e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let got = text.split_whitespace().next().unwrap_or("");
    if !out.status.success() || got.is_empty() {
        return Err(format!("算不了校验和：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    if !got.eq_ignore_ascii_case(d.sha256) {
        return Err(format!("校验和对不上（下来的是 {}…，应该是 {}…），没装", &got[..got.len().min(12)], &d.sha256[..12]));
    }
    Ok(())
}

fn unpack(file: &Path, into: &Path) -> Result<(), String> {
    std::fs::create_dir_all(into).map_err(|e| format!("建不了 {}：{e}", into.display()))?;
    // macOS 的 tar 是 bsdtar：默认不认带 `..` 的路径、去掉开头的 `/`，解不到目录外面去（再加上 sha256 钉死，来的就是那一份）
    let mut cmd = Command::new("/usr/bin/tar");
    cmd.arg("-xzf").arg(file).arg("-C").arg(into);
    let out = procutil::run_capped(&mut cmd, 64 << 10, procutil::ERR_CAP, procutil::Overflow::Drain).map_err(|e| format!("起不了 tar：{e}"))?;
    if !out.status.success() {
        return Err(format!("解不开：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lspsvc-inst-{name}-{}-{:?}", std::process::id(), std::time::Instant::now()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个「像 jdtls」的小发行包：plugins/ 里一个 launcher jar、两个架构的 config.ini。返回（包的路径、字节数、sha256）
    fn fake_dist(dir: &Path) -> (PathBuf, u64, String) {
        let src = dir.join("src");
        for f in ["plugins/org.eclipse.equinox.launcher_1.7.0.jar", "config_mac_arm/config.ini", "config_mac/config.ini"] {
            let p = src.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
            std::fs::write(&p, "x".repeat(4096)).unwrap();
        }
        let tgz = dir.join("jdtls.tar.gz");
        #[allow(clippy::disallowed_methods)] // 测试里造包，不读输出
        let st = Command::new("/usr/bin/tar").arg("-czf").arg(&tgz).arg("-C").arg(&src).arg(".").status().unwrap();
        assert!(st.success());
        let out = procutil::run_capped(Command::new("/usr/bin/shasum").args(["-a", "256"]).arg(&tgz), 4096, 4096, procutil::Overflow::Kill).unwrap();
        let sha = String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap().to_string();
        (tgz.clone(), std::fs::metadata(&tgz).unwrap().len(), sha)
    }

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    #[test]
    fn 下载校验解包改名_装好认得出_第二次直接用() {
        let dir = tmp("ok");
        let (tgz, size, sha) = fake_dist(&dir);
        let d = Dist { version: "9.9.9", url: "", sha256: leak(sha), size };
        let base = dir.join("jdtls");
        let seen = Mutex::new(Vec::new());
        let got = install(&d, &crate::uri::from_path(&tgz.to_string_lossy()), &base, |a, b| seen.lock().unwrap().push((a, b)), &AtomicBool::new(false)).unwrap();
        assert_eq!(got, base.join("9.9.9"));
        assert!(installed(&base, &d).is_some());
        assert_eq!(seen.lock().unwrap().last(), Some(&(size, size)), "最后一次进度是下满了");
        let left: Vec<_> = std::fs::read_dir(&base).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, vec!["9.9.9".to_string()], "临时文件和 .partial 都清掉了");
        // 第二次：不再下载（给一个不存在的地址也能成）
        assert_eq!(install(&d, "file:///no/such.tar.gz", &base, |_, _| {}, &AtomicBool::new(false)).unwrap(), base.join("9.9.9"));
    }

    #[test]
    fn 校验和对不上_不装_什么都不留() {
        let dir = tmp("bad");
        let (tgz, size, _) = fake_dist(&dir);
        let d = Dist { version: "9.9.9", url: "", sha256: "0000000000000000000000000000000000000000000000000000000000000000", size };
        let base = dir.join("jdtls");
        let e = install(&d, &crate::uri::from_path(&tgz.to_string_lossy()), &base, |_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("校验和对不上"), "{e}");
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 0, "一个文件都不留");
    }

    #[test]
    fn 字节数对不上_先在这儿拦下() {
        let dir = tmp("short");
        let (tgz, size, sha) = fake_dist(&dir);
        let d = Dist { version: "9.9.9", url: "", sha256: leak(sha), size: size + 1 };
        let e = install(&d, &crate::uri::from_path(&tgz.to_string_lossy()), &dir.join("jdtls"), |_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("下载不完整"), "{e}");
    }

    #[test]
    fn 下载失败说原因_上次留下的半截先清掉_不认识的目录不覆盖() {
        let dir = tmp("fail");
        let base = dir.join("jdtls");
        std::fs::create_dir_all(base.join("9.9.9.partial/plugins")).unwrap();
        let d = Dist { version: "9.9.9", url: "", sha256: "00", size: 1 };
        let e = install(&d, "file:///no/such/jdtls.tar.gz", &base, |_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("下载失败"), "{e}");
        assert!(!base.join("9.9.9.partial").exists(), "上次的半截被清掉了");
        // 已经有一个认不出的 9.9.9：不盖
        let (tgz, size, sha) = fake_dist(&dir);
        std::fs::create_dir_all(base.join("9.9.9/something")).unwrap();
        let d = Dist { version: "9.9.9", url: "", sha256: leak(sha), size };
        let e = install(&d, &crate::uri::from_path(&tgz.to_string_lossy()), &base, |_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("先把它挪走"), "{e}");
        assert!(base.join("9.9.9/something").exists(), "别人的目录没被动");
    }

    #[test]
    fn 取消了_不下() {
        let dir = tmp("cancel");
        let (tgz, size, sha) = fake_dist(&dir);
        let d = Dist { version: "9.9.9", url: "", sha256: leak(sha), size };
        let e = install(&d, &crate::uri::from_path(&tgz.to_string_lossy()), &dir.join("jdtls"), |_, _| {}, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(e, "取消了");
        assert!(installed(&dir.join("jdtls"), &d).is_none());
    }

    #[test]
    fn 钉的那一版_地址和校验和对得上发布页的格式() {
        assert!(DIST.url.starts_with("https://download.eclipse.org/jdtls/milestones/"));
        assert!(DIST.url.contains(DIST.version));
        assert_eq!(DIST.sha256.len(), 64);
        assert!(DIST.sha256.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
