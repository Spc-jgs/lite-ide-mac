//! 自动更新的那一层（docs/TAKEOVER.md MVP-8，D8：不打断、不带遥测）：什么时候查、下到哪、状态告诉谁、点了「重启更新」之后怎么换。
//! 读 latest.json、比版本、验签、换 .app 在 `updsvc`。
//!
//! ```text
//! 启动 30 秒后、之后每 12 小时（设置 update.check 关掉就不查）
//!   → 读 latest.json → 比现在的版本新？→ 后台下到应用缓存目录 → 验签 → 「已就绪」，广播给每个窗口（状态栏出「重启更新」）
//! 点「重启更新」
//!   → 换 .app（旧的挪到旁边）→ 照 ⌘Q 的路退出（每个窗口存好现场）→ 退出最后一刻 `open -n` 起新的
//!   → 新的起来 30 秒后把旁边那个旧的扔进废纸篓
//! ```
//!
//! 为什么下好了才说：说「有新版」再让人等几分钟下载，等于打断两次；IDEA 是先说再下，VS Code 是下好了再说 —— 学后者。
//! 为什么换 .app 在退出之前：换失败了（装在没写权限的地方、包不对）还开着窗口，能把原因说给你看；退了再换，错了没人看得见。
//! 换完到退出这两三秒里，正在跑的这份的文件已经被挪走了 —— 没关系：进程攥着的是已经打开的文件，路径不再用；
//! 退出流程不再加载新东西。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// 每个窗口都收（同 `settings-changed`）：新版就绪是应用级的事
pub const EVENT: &str = "update-status";
/// 正式发布的 latest.json（测试构建里换成验收脚本的，见 [`source`]）：GitHub 的「最新一个 Release」的下载地址，发版时 release.yml 传上去（草稿不算，点了 Publish 才算）
#[cfg_attr(feature = "test-bridge", allow(dead_code))]
const MANIFEST: &str = "https://github.com/Spc-jgs/lite-ide-mac/releases/latest/download/latest.json";
/// 验签的公钥（`tauri signer generate` 生成，2026-10-27）。私钥不在仓库里：在发版的人手里、和 GitHub Actions 的 secret 里
#[cfg_attr(feature = "test-bridge", allow(dead_code))]
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDU1RUQ3NTVCNTBBMTI2MUUKUldRZUpxRlFXM1h0VmE4ZWxEWXY5Z1JUUEo0c2Q5Y3RnaEtMbjA0L3VMTGt4VERJbCtZU01KVTQK";
/// 包的上限：现在的 universal 包带两份裁过的 Java 运行时，约 200 MB；超过 1 GB 的不是我们发的
const MAX_BYTES: u64 = 1 << 30;
const FIRST_DELAY: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(12 * 3600);

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    /// 还没查过
    #[default]
    Idle,
    Checking,
    Downloading,
    /// 下好了、验过签名，等你点
    Ready,
    /// 查过了，没有更新的
    Latest,
    Failed,
    /// 开发构建（不是从 .app 里跑的）：没有东西可换
    Unsupported,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub phase: Phase,
    /// 新版本号（Ready / Downloading 时）
    pub version: String,
    pub notes: String,
    pub error: String,
    tarball: Option<PathBuf>,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
/// 同一时刻只查一次：自动的和菜单「检查更新…」撞上时，后来的直接拿当前状态
static BUSY: AtomicBool = AtomicBool::new(false);
/// 换好了，退出的最后一刻起这个（`relaunch_on_exit`）
static RELAUNCH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn status() -> Status {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

fn set(app: &AppHandle, s: Status) {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    let _ = app.emit(EVENT, crate::commands::update_dto(app));
}

/// 正在跑的这份 .app；开发构建（`target/release/lite-ide` 直接跑）没有
pub fn bundle() -> Option<PathBuf> {
    fsservice::bundle_from_exe(&std::env::current_exe().ok()?)
}

/// 去哪查、用哪把公钥认、包的地址许不许 `file://`。测试构建里验收脚本换成自己签的本机文件（`LITE_UPDATE_URL` / `LITE_UPDATE_PUBKEY`），
/// 没设的话测试构建一概不查（不往 GitHub 发请求）；正式包里没有这条路
fn source() -> Option<(String, String, bool)> {
    #[cfg(feature = "test-bridge")]
    {
        let url = std::env::var("LITE_UPDATE_URL").ok().filter(|u| !u.is_empty())?;
        let key = std::env::var("LITE_UPDATE_PUBKEY").unwrap_or_default();
        Some((url, key, true))
    }
    #[cfg(not(feature = "test-bridge"))]
    {
        Some((MANIFEST.to_string(), PUBKEY.to_string(), false))
    }
}

/// 启动后多久动手。测试构建里验收脚本可以缩短（`LITE_UPDATE_DELAY_MS`），不然要干等 30 秒
fn first_delay() -> Duration {
    #[cfg(feature = "test-bridge")]
    if let Some(ms) = std::env::var("LITE_UPDATE_DELAY_MS").ok().and_then(|v| v.parse().ok()) {
        return Duration::from_millis(ms);
    }
    FIRST_DELAY
}

/// 换下来的旧版本扔进废纸篓。测试构建里扔进验收脚本给的目录（`LITE_UPDATE_TRASH`），没给就不动 —— 验收不往你的废纸篓里扔东西
fn trash_old(p: &std::path::Path) -> Result<(), String> {
    #[cfg(feature = "test-bridge")]
    {
        let Some(dir) = std::env::var_os("LITE_UPDATE_TRASH") else { return Err("测试构建没设 LITE_UPDATE_TRASH，不扔".into()) };
        let to = std::path::Path::new(&dir).join(p.file_name().unwrap_or_default());
        #[allow(clippy::disallowed_methods)] // 测试构建：挪进验收脚本自己的临时目录（目标是它新建的空目录）
        return std::fs::rename(p, to).map_err(|e| e.to_string());
    }
    #[cfg(not(feature = "test-bridge"))]
    fsservice::move_to_trash(p).map_err(|e| e.to_string())
}

/// 查一次：有新的就下、验签，到「就绪」为止。阻塞（网络、下载），调用方放到阻塞池或者自己的线程里
pub fn check(app: &AppHandle) -> Status {
    let cur = status();
    if cur.phase == Phase::Ready || BUSY.swap(true, Ordering::SeqCst) {
        return cur;
    }
    let s = check_now(app);
    set(app, s.clone());
    BUSY.store(false, Ordering::SeqCst);
    s
}

fn check_now(app: &AppHandle) -> Status {
    let fail = |error: String| {
        applog::write(applog::Level::Warn, "update", &error);
        Status { phase: Phase::Failed, error, ..Default::default() }
    };
    if bundle().is_none() {
        return Status { phase: Phase::Unsupported, error: "开发构建不支持自动更新".into(), ..Default::default() };
    }
    let Some((url, key, allow_file)) = source() else {
        return Status { phase: Phase::Unsupported, error: "测试构建未设置 LITE_UPDATE_URL".into(), ..Default::default() };
    };
    set(app, Status { phase: Phase::Checking, ..Default::default() });
    let rel = match updsvc::fetch_text(&url).and_then(|t| updsvc::parse_manifest(&t, updsvc::target(), allow_file)) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let now = app.package_info().version.to_string();
    if !updsvc::newer(&rel.version, &now) {
        return Status { phase: Phase::Latest, version: now, ..Default::default() };
    }
    let Ok(dir) = app.path().app_cache_dir().map(|d| d.join("updates")) else { return fail("无法获取应用缓存目录".into()) };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(format!("无法创建 {}：{e}", dir.display()));
    }
    let name = format!("lite-ide-{}.app.tar.gz", rel.version);
    let file = dir.join(&name);
    prune(&dir, &name);
    // 上次下好了（下好、验过才会改成这个名字）：再验一遍就行，不重下
    if updsvc::verify_file(&file, &rel.signature, &key).is_err() {
        set(app, Status { phase: Phase::Downloading, version: rel.version.clone(), ..Default::default() });
        let part = dir.join(format!("{name}.part"));
        if let Err(e) = updsvc::download(&rel.url, &part, MAX_BYTES).and_then(|_| updsvc::verify_file(&part, &rel.signature, &key)) {
            return fail(e);
        }
        #[allow(clippy::disallowed_methods)] // 改名我们自己下的文件：目标是缓存目录里自己起的名字，旧的那份刚验过签名不对
        if let Err(e) = std::fs::rename(&part, &file) {
            return fail(format!("无法保存下载的安装包：{e}"));
        }
    }
    applog::write(applog::Level::Info, "update", &format!("新版 {} 就绪（现在 {now}）", rel.version));
    Status { phase: Phase::Ready, version: rel.version, notes: rel.notes, tarball: Some(file), ..Default::default() }
}

/// 缓存目录里别的版本的包（以前下的、下了一半的）删掉 —— 只认我们自己起的那两种名字
fn prune(dir: &std::path::Path, keep: &str) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let ours = n.starts_with("lite-ide-") && (n.ends_with(".app.tar.gz") || n.ends_with(".app.tar.gz.part"));
        if ours && n != keep {
            #[allow(clippy::disallowed_methods)] // 应用自己下的更新包，不是用户的文件（rust.md「删除只走废纸篓」那条的例外）
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// 点了「重启更新」：换 .app，照 ⌘Q 的路退出，退出最后一刻起新的。换失败就不退、把原因交回去
pub fn apply(app: &AppHandle) -> Result<(), String> {
    let s = status();
    let (Phase::Ready, Some(tarball)) = (&s.phase, &s.tarball) else { return Err("没有已下载的新版本".into()) };
    let bundle = bundle().ok_or("开发构建不支持自动更新")?;
    let old = updsvc::install(&bundle, tarball)?;
    applog::write(applog::Level::Info, "update", &format!("换成 {}，旧的在 {}，退出后重启", s.version, old.display()));
    *RELAUNCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(bundle);
    crate::winctl::quit(app);
    Ok(())
}

/// `RunEvent::Exit` 里调：换过了就起新的。`open -n`：旧进程这时还没完全退，不带 `-n` 的话 LaunchServices 会以为它已经开着、只把它叫到前面
pub fn relaunch_on_exit() {
    let Some(app) = RELAUNCH.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let mut c = std::process::Command::new("/usr/bin/open");
    c.arg("-n").arg(&app).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[allow(clippy::disallowed_methods)] // 起完就走：不读它的输出（三路都接空），我们自己马上就退了
    let _ = c.spawn();
}

/// 启动时挂上：等 30 秒（别跟启动抢），把上一次换下来的旧版本扔进废纸篓，之后按设置每 12 小时查一次
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
        std::thread::sleep(first_delay());
        if let Some(b) = bundle() {
            for old in updsvc::leftovers(&b, std::process::id()) {
                match trash_old(&old) {
                    Ok(()) => applog::write(applog::Level::Info, "update", &format!("上一版扔进了废纸篓：{}", old.display())),
                    Err(e) => applog::write(applog::Level::Warn, "update", &format!("上一版没扔进废纸篓（{}）：{e}", old.display())),
                }
            }
        }
        loop {
            if app.state::<AppState>().settings.current().update_check {
                check(&app);
            }
            std::thread::sleep(INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 编进去的公钥是一把能用的钥匙() {
        // 拿别的钥匙签的名去验：对得上公钥的格式，才会走到「签名校验失败」；公钥本身坏了是「公钥无效」
        let other_sig = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSVVVySlVwWHhzbkplcWMySUxpNU1jcDZzZ2J1Z3FwTGtoUFUzU2p4blp1elVaamRybmxBWW5vaU9EcDVvU09YQVRIRkxrVlZ5QmwyZ21IS3FjOG5ITitqdTEvblF0dWc4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzMDA1ODEyCWZpbGU6cGF5bG9hZC5iaW4KUEtScVBQeDFXR0VkbG5FVHpvc2Y2R3c2VzFsdkZhSndNeHVSanNEa3piSGh6ZSt0WnpKUzNTMGlRVVc2WVNWTFg2bnZXZTVvN0UxR2tINjVVUndLREE9PQo=";
        let e = updsvc::verify(b"lite-ide update payload\n", other_sig, PUBKEY).unwrap_err();
        assert!(e.contains("签名校验失败"), "{e}");
        assert!(MANIFEST.starts_with("https://github.com/") && MANIFEST.ends_with("/releases/latest/download/latest.json"));
    }
}
