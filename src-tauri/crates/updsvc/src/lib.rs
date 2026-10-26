//! 自动更新（docs/TAKEOVER.md MVP-8，D8：不打断、不带遥测）。不依赖 Tauri：
//!
//! ```text
//! 检查    curl 读 GitHub Release 上的 latest.json（Tauri 更新器的格式：版本、说明、每个平台的包地址和签名）→ 比版本
//! 下载    curl 把 .app.tar.gz 下到应用缓存目录（设上限）
//! 校验    minisign 签名，公钥编在应用里 —— 包和 latest.json 都可能被换（下载地址、中间人），只有签名说了算
//! 装      解到 .app 旁边的临时目录（同一个卷，改名是原子的）→ 认一认是不是我们（CFBundleIdentifier）→ 旧的改名挪开、新的改名进来
//!         （第二步失败就挪回去）→ 旧的扔进废纸篓（能找回来）
//! ```
//!
//! 为什么不用 Tauri 的更新插件：它带进来 reqwest + 一整套 TLS（正式包变大），2.13 起还要 Tauri ^2.12（我们在 2.11）。
//! 下载走系统的 curl（`.http` 请求也是它，httpsvc），校验用 minisign 作者写的那个库 —— 签名格式和 Tauri 的一样，CI 用 `tauri signer sign` 签

use base64::Engine;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// latest.json 里给这台机器的那一份
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    /// Tauri 签名工具写的那一串（base64 过的 minisign 签名）
    pub signature: String,
}

/// 这台机器在 latest.json 里的键：`darwin-aarch64` / `darwin-x86_64`（Tauri 的叫法）
pub fn target() -> &'static str {
    if cfg!(target_arch = "aarch64") { "darwin-aarch64" } else { "darwin-x86_64" }
}

/// latest.json → 这台机器的那一份。先找本机架构的，没有找 `darwin-universal`（universal 包两种机器都能装）
pub fn parse_manifest(text: &str, target: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("更新信息不是 JSON：{e}"))?;
    let version = v["version"].as_str().ok_or("更新信息里没有版本号")?.trim_start_matches('v').to_string();
    let p = [target, "darwin-universal"].iter().map(|k| &v["platforms"][*k]).find(|p| p.is_object()).ok_or_else(|| format!("这次发布没有给 {target} 的包"))?;
    let url = p["url"].as_str().filter(|u| u.starts_with("https://")).ok_or("包的地址不是 https 的")?.to_string();
    let signature = p["signature"].as_str().filter(|s| !s.is_empty()).ok_or("这次发布的包没有签名")?.to_string();
    Ok(Release { version, notes: v["notes"].as_str().unwrap_or("").to_string(), url, signature })
}

/// `a` 比 `b` 新吗（`1.10.0` > `1.9.3`；按数字比，不按字符串）。带后缀的（`1.6.0-rc.1`）后缀不比，只比前三段 —— 我们不发预发布版
pub fn newer(a: &str, b: &str) -> bool {
    let nums = |s: &str| -> Vec<u64> { s.trim_start_matches('v').split(['-', '+']).next().unwrap_or("").split('.').map(|x| x.parse().unwrap_or(0)).collect() };
    let (x, y) = (nums(a), nums(b));
    for i in 0..3 {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}

/// 校验签名：`signature`、`pubkey` 都是 Tauri 签名工具写的 base64（里面是 minisign 的文本格式）
pub fn verify(data: &[u8], signature: &str, pubkey: &str) -> Result<(), String> {
    let b64 = |s: &str| -> Result<String, String> {
        let raw = base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|e| format!("签名 / 公钥不是 base64：{e}"))?;
        String::from_utf8(raw).map_err(|_| "签名 / 公钥解出来不是文本".to_string())
    };
    let key = minisign_verify::PublicKey::decode(&b64(pubkey)?).map_err(|e| format!("公钥不对：{e}"))?;
    let sig = minisign_verify::Signature::decode(&b64(signature)?).map_err(|e| format!("签名格式不对：{e}"))?;
    key.verify(data, &sig, true).map_err(|_| "签名对不上：这个包不是我们发的，或者下载坏了 —— 没装".to_string())
}

/// 读 latest.json（小文件，读前 64 KB）
pub fn fetch_text(url: &str) -> Result<String, String> {
    let mut c = Command::new("/usr/bin/curl");
    c.args(["-fsSL", "--max-time", "20", "--proto", "=https,file", "--max-filesize", "65536", "--", url]);
    let o = procutil::run_capped(&mut c, 64 << 10, procutil::ERR_CAP, procutil::Overflow::Kill).map_err(|e| format!("起不了 curl：{e}"))?;
    if !o.status.success() || o.truncated {
        return Err(format!("读不到更新信息：{}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// 把包下到 `dest`（curl 直接写文件，`--max-filesize` 设上限：一个 .app 包几百 MB 封顶）。只许 https（测试里许 file://）
pub fn download(url: &str, dest: &Path, max_bytes: u64) -> Result<(), String> {
    let mut c = Command::new("/usr/bin/curl");
    c.args(["-fsSL", "--proto", "=https,file", "--max-filesize", &max_bytes.to_string(), "-o"]).arg(dest).arg("--").arg(url);
    let o = procutil::run_capped(&mut c, 16 << 10, procutil::ERR_CAP, procutil::Overflow::Kill).map_err(|e| format!("起不了 curl：{e}"))?;
    if !o.status.success() {
        return Err(format!("下载没成：{}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(())
}

/// 一个 .app 的 `CFBundleIdentifier`（读 Info.plist 的文本；Tauri 写的是 XML plist）
pub fn bundle_id(app: &Path) -> Option<String> {
    let t = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let i = t.find("<key>CFBundleIdentifier</key>")?;
    let rest = &t[i..];
    let a = rest.find("<string>")? + "<string>".len();
    let b = rest[a..].find("</string>")?;
    Some(rest[a..a + b].trim().to_string())
}

/// 装：`tarball` 解到 `app` 旁边的临时目录，认过身份，换掉 `app`。旧的交给 `trash`（扔进废纸篓，能找回来）。
/// 每一步失败都停在「旧的还在原处」的样子：解不开 / 身份不对 → 不动；新的挪不进来 → 旧的挪回去
pub fn install(app: &Path, tarball: &Path, trash: &dyn Fn(&Path) -> Result<(), String>) -> Result<(), String> {
    let id = bundle_id(app).ok_or("认不出现在这个应用（不是从 .app 里跑的？）")?;
    let dir = app.parent().ok_or("找不到应用所在的目录")?;
    let staging = dir.join(format!(".lite-ide-update-{}", std::process::id()));
    // 自己建的临时目录：上次崩在半路留下的同名目录只会是我们的（名字带 pid，基本撞不上）
    std::fs::create_dir(&staging).map_err(|e| format!("在 {} 里建不了临时目录（没有写权限？装在 /Applications 的话要你自己换）：{e}", dir.display()))?;
    let r = (|| {
        let mut c = Command::new("/usr/bin/tar");
        c.arg("-xzf").arg(tarball).arg("-C").arg(&staging);
        let o = procutil::run_capped(&mut c, 64 << 10, procutil::ERR_CAP, procutil::Overflow::Kill).map_err(|e| format!("起不了 tar：{e}"))?;
        if !o.status.success() {
            return Err(format!("包解不开：{}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        let new_app: PathBuf = std::fs::read_dir(&staging)
            .map_err(|e| format!("{e}"))?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "app"))
            .ok_or("包里没有 .app")?;
        if bundle_id(&new_app).as_deref() != Some(id.as_str()) {
            return Err(format!("包里的应用不是这个（{} ≠ {id}），没装", bundle_id(&new_app).unwrap_or_default()));
        }
        let old = dir.join(format!(".lite-ide-old-{}.app", std::process::id()));
        #[allow(clippy::disallowed_methods)] // 挪开我们自己（目标是自己起的、带 pid 的名字，不会盖掉别的东西）
        std::fs::rename(app, &old).map_err(|e| format!("挪不开现在这个应用：{e}"))?;
        #[allow(clippy::disallowed_methods)] // 新的挪进原来的位置：原来那个刚挪走，这个路径此刻是空的
        if let Err(e) = std::fs::rename(&new_app, app) {
            #[allow(clippy::disallowed_methods)] // 新的没挪进来：把旧的挪回原处（同一个空位置）
            let _ = std::fs::rename(&old, app);
            return Err(format!("新版本挪不进去，已经换回原来的：{e}"));
        }
        // 旧的进废纸篓：万一新版本有问题，能从废纸篓里拖回来
        if let Err(e) = trash(&old) {
            return Err(format!("装好了，但旧版本没扔进废纸篓（在 {}，可以自己删）：{e}", old.display()));
        }
        Ok(())
    })();
    // 临时目录这时应该是空的（新的 .app 挪走了）；不空就是失败了一半，留着里面的东西给人看，不递归删
    let _ = std::fs::remove_dir(&staging);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `tauri signer generate` 出的测试公钥（私钥没进仓库），和它对 `payload` 这几个字签的名（2026-10-26 生成）
    const PUB: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDlDNkM3Q0E1NTRCMjUyNTQKUldSVVVySlVwWHhzbkdBRlB0Z1VSL1ptNG50K1pzRHI5N3FsVjJySTd2Znh6QXVNendyaTlTb3kK";
    const SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSVVVySlVwWHhzbkplcWMySUxpNU1jcDZzZ2J1Z3FwTGtoUFUzU2p4blp1elVaamRybmxBWW5vaU9EcDVvU09YQVRIRkxrVlZ5QmwyZ21IS3FjOG5ITitqdTEvblF0dWc4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzMDA1ODEyCWZpbGU6cGF5bG9hZC5iaW4KUEtScVBQeDFXR0VkbG5FVHpvc2Y2R3c2VzFsdkZhSndNeHVSanNEa3piSGh6ZSt0WnpKUzNTMGlRVVc2WVNWTFg2bnZXZTVvN0UxR2tINjVVUndLREE9PQo=";
    const PAYLOAD: &[u8] = b"lite-ide update payload\n";

    #[test]
    fn 签名_对的过_改一个字节不过_换个公钥不过() {
        verify(PAYLOAD, SIG, PUB).unwrap();
        let mut bad = PAYLOAD.to_vec();
        bad[0] ^= 1;
        assert!(verify(&bad, SIG, PUB).unwrap_err().contains("签名对不上"));
        // 别的密钥对的公钥（同一个工具另生成的）
        let other = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDJCMDM3MTJFMTY4MkQzNDcKUldSSDA0SVdMbkVESytyYS96SEhTWEZXSnhaZW9PZGI2emxBekd4MUlwTTFhK01NNXBBTjgrcVoK";
        assert!(verify(PAYLOAD, SIG, other).unwrap_err().contains("签名对不上"), "是真的另一把钥匙（不是格式坏了）");
    }

    /// 造一个假的 .app：目录 + Info.plist（只有 CFBundleIdentifier）+ 一个标记文件
    #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
    fn fake_app(at: &Path, id: &str, mark: &str) {
        std::fs::create_dir_all(at.join("Contents")).unwrap();
        std::fs::write(at.join("Contents/Info.plist"), format!("<plist><dict><key>CFBundleIdentifier</key>\n<string>{id}</string></dict></plist>")).unwrap();
        std::fs::write(at.join("Contents/mark"), mark).unwrap();
    }

    #[test]
    fn 装_换成新的_旧的交给废纸篓_包里的应用不是这个就不动() {
        let d = std::env::temp_dir().join(format!("lite-upd-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let app = d.join("dir/lite-ide.app");
        fake_app(&app, "com.liteide.app", "old");
        // 新版本的包：tar 里是 lite-ide.app/…
        let src = d.join("src");
        fake_app(&src.join("lite-ide.app"), "com.liteide.app", "new");
        let tgz = d.join("new.tar.gz");
        assert!(Command::new("/usr/bin/tar").arg("-czf").arg(&tgz).arg("-C").arg(&src).arg("lite-ide.app").status().unwrap().success());
        let trashed = std::sync::Mutex::new(Vec::<PathBuf>::new());
        install(&app, &tgz, &|p| {
            trashed.lock().unwrap().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(app.join("Contents/mark")).unwrap(), "new", "原来的位置上是新版本");
        let t = trashed.lock().unwrap().clone();
        assert_eq!(t.len(), 1);
        assert_eq!(std::fs::read_to_string(t[0].join("Contents/mark")).unwrap(), "old", "旧的交给了废纸篓（没自己删）");
        assert!(std::fs::read_dir(d.join("dir")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".lite-ide-update")), "临时目录收掉了");

        // 包里的应用不是这个：一个字都不动
        let other = d.join("other");
        fake_app(&other.join("lite-ide.app"), "com.evil.app", "evil");
        let tgz2 = d.join("evil.tar.gz");
        assert!(Command::new("/usr/bin/tar").arg("-czf").arg(&tgz2).arg("-C").arg(&other).arg("lite-ide.app").status().unwrap().success());
        let e = install(&app, &tgz2, &|_| Ok(())).unwrap_err();
        assert!(e.contains("不是这个"), "{e}");
        assert_eq!(std::fs::read_to_string(app.join("Contents/mark")).unwrap(), "new", "没动");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 版本按数字比_带v的照认() {
        assert!(newer("1.10.0", "1.9.3"));
        assert!(newer("v1.5.1", "1.5.0"));
        assert!(!newer("1.5.0", "1.5.0"));
        assert!(!newer("1.4.9", "1.5.0"));
        assert!(newer("2.0", "1.99.99"), "少一段当 0");
    }

    #[test]
    fn 更新信息_挑本机架构的_没有挑_universal_不是_https_的不认() {
        let j = r#"{ "version": "v1.6.0", "notes": "改了些东西", "platforms": {
            "darwin-aarch64": { "url": "https://github.com/x/y/releases/download/v1.6.0/lite-ide.app.tar.gz", "signature": "sig-a" },
            "darwin-universal": { "url": "https://github.com/x/y/releases/download/v1.6.0/lite-ide-universal.app.tar.gz", "signature": "sig-u" } } }"#;
        let r = parse_manifest(j, "darwin-aarch64").unwrap();
        assert_eq!((r.version.as_str(), r.signature.as_str(), r.notes.as_str()), ("1.6.0", "sig-a", "改了些东西"));
        assert_eq!(parse_manifest(j, "darwin-x86_64").unwrap().signature, "sig-u");
        let http = j.replace("https://github.com/x/y/releases/download/v1.6.0/lite-ide.app", "http://evil/lite-ide.app");
        assert!(parse_manifest(&http, "darwin-aarch64").is_err(), "明文 http 的不认（签名照样会挡，但没必要去下）");
    }
}
