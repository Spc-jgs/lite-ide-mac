//! 窗口的建、存、退出、重开（多窗口第 3 步，docs/MULTIWINDOW.md 3.5–3.8）。
//!
//! `windows.rs` 做决定（开着哪些窗口、路径该去哪、要不要拦退出），这里动手 ——
//! 建窗口、挂材质、写 `windows.json`、给每个窗口发 `flush`。拆成两个文件的理由同
//! 服务 crate 不依赖 Tauri：决定要能在裸单测里测全，动手的这一层薄到只剩接线。

use crate::state::AppState;
use crate::windows::{Frame, Saved, SavedWin};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// 新窗口从前台窗口往右下错开多少（macOS 自己的习惯是 20–30pt）
const CASCADE: f64 = 28.0;
/// 退出时最多等各窗口回话多久。超时照样退：一个卡死的窗口不能让应用退不掉
const QUIT_WAIT: Duration = Duration::from_secs(2);

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("windows.json"))
}

/// 读上次存的。读不到、读坏了一律当没有（`parse_saved` 不抛）
pub fn load(app: &AppHandle) -> Saved {
    file(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| crate::windows::parse_saved(&t))
        .unwrap_or_default()
}

/// 立刻写一次
pub fn save_now(app: &AppHandle) {
    let Some(path) = file(app) else { return };
    save_to(&path, || serde_json::to_string_pretty(&app.state::<AppState>().windows.snapshot()).ok());
}

/// 同一时刻只许一个线程写 `windows.json`。
///
/// 写它的有三条线程：存盘线程（`save_soon` 攒够 400ms）、退出线程（`quit` 等完回话）、
/// 主线程（`RunEvent::Exit`）。⌘Q 时后两条几乎同时到，存盘线程也可能正好醒着。
/// 它们共用同一个临时文件名：A 写到一半，B 把它截断重写，A 先 rename 过去 ——
/// 落盘的就是 B 写了一半的 JSON，`parse_saved` 读到坏文件当没有，**窗口列表和「最近打开」一起没了**。
static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 拿着锁拍快照、写盘。拍快照也在锁里：两个线程先后进来时，后写的那份一定是后拍的，旧的盖不掉新的。
/// `save_now` 和测试走的是这同一个函数 —— 测试要是只测一个「带锁的写」而 `save_now` 自己另拿锁，
/// 把 `save_now` 里那把锁删掉测试照样绿（审查修完之后编译器的「没人用」警告提醒的）
fn save_to(path: &std::path::Path, text: impl FnOnce() -> Option<String>) {
    let _g = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = text() {
        write_locked(path, &t);
    }
}

fn write_locked(path: &std::path::Path, text: &str) {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let ok = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.sync_all()
    });
    if ok.is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

static SAVER: OnceLock<mpsc::Sender<()>> = OnceLock::new();

/// 起一条存盘线程：`save_soon` 叫一下，它等 400ms 把这期间的都合并成一次写。
///
/// 拖窗口时 `Moved` 一秒几十次，每次都同步写盘是在主线程上做 IO（rust.md：碰盘的不留在主线程）。
pub fn start_saver(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    if SAVER.set(tx).is_err() {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("windows-save".into()).spawn(move || {
        while rx.recv().is_ok() {
            std::thread::sleep(Duration::from_millis(400));
            while rx.try_recv().is_ok() {}
            save_now(&app);
        }
    });
}

pub fn save_soon() {
    if let Some(tx) = SAVER.get() {
        let _ = tx.send(());
    }
}

/// 物理像素 → 逻辑像素的位置和大小
pub fn frame_from(pos: tauri::PhysicalPosition<i32>, size: tauri::PhysicalSize<u32>, scale: f64) -> Frame {
    let s = if scale > 0.0 { scale } else { 1.0 };
    Frame { x: pos.x as f64 / s, y: pos.y as f64 / s, w: size.width as f64 / s, h: size.height as f64 / s }
}

/// 记下一个窗口现在在哪：`Moved` / `Resized` 时，以及**窗口刚建好时**（[`note_frame`]）。
///
/// 只靠 `Moved` / `Resized` 不够：一个建出来之后没被拖过的窗口一次都不会触发它们，
/// 登记表里就没有它的位置，下次启动只能按「从前台错开」去摆 —— 第 3 步真机验收里
/// 没挪过的那个窗口重开后跑到了别处，就是这么来的。
pub fn track(app: &AppHandle, w: &tauri::Window) {
    if let (Ok(p), Ok(s), Ok(k)) = (w.outer_position(), w.inner_size(), w.scale_factor()) {
        app.state::<AppState>().windows.set_frame(w.label(), frame_from(p, s, k));
        save_soon();
    }
}

/// 刚建好的窗口（手上拿的是 `WebviewWindow`）记一次位置。和 [`track`] 是同一件事，只换个入口
fn note_frame(app: &AppHandle, w: &tauri::WebviewWindow) {
    track(app, &w.as_ref().window());
}

/// 存下来的位置还在某块屏幕上吗。外接显示器拔掉之后，上次在副屏上的窗口
/// 照原样摆回去就是一扇看不见的窗 —— 那种位置宁可丢掉，用默认的。
/// 判据是标题栏左端那一小块落在某块屏幕里（整扇窗都在屏内太严：贴边的窗口很常见）。
fn on_screen(app: &AppHandle, f: &Frame) -> bool {
    let Ok(mons) = app.available_monitors() else { return true };
    let (px, py) = (f.x + 40.0, f.y + 12.0);
    mons.iter().any(|m| {
        let k = m.scale_factor();
        let (x, y) = (m.position().x as f64 / k, m.position().y as f64 / k);
        let (w, h) = (m.size().width as f64 / k, m.size().height as f64 / k);
        px >= x && px < x + w && py >= y && py < y + h
    })
}

fn title_for(root: Option<&str>) -> String {
    root.and_then(|r| std::path::Path::new(r).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "lite-ide".into())
}

/// 窗口标题 = 项目名。标题栏本身是隐藏的，但「窗口」菜单、Mission Control、⌘` 都用它 ——
/// 全叫 lite-ide 的话开三个就分不清谁是谁
pub fn set_title(w: &tauri::Window, root: Option<&str>) {
    let _ = w.set_title(&title_for(root));
}

/// 「最近打开」变了：重建原生菜单、告诉每个窗口（标题栏的下拉、空态卡片、快照清理都要用）、落盘。
///
/// **广播，不定向**：每个窗口都要知道。前端用 `listenHere` 也收得到 —— 筛选只对定了向的事件起作用
pub fn recent_changed(app: &AppHandle) {
    let dto = recent_dto(app);
    let _ = crate::menu::refresh_recent(app, &dto.projects);
    let _ = app.emit("recent-changed", &dto);
    save_soon();
}

pub fn recent_dto(app: &AppHandle) -> crate::commands::RecentDto {
    let st = app.state::<AppState>();
    crate::commands::RecentDto { projects: st.windows.recent(), keep: st.windows.keep_roots() }
}

/// 建一个新窗口：和 `main` 同一份配置（`tauri.conf.json` 的 `windows[0]`），换个 label。
///
/// `paths` 进它的收件箱，它的前端起来调 `initial_paths` 时取走 —— **新窗口靠这个知道
/// 自己该开哪个项目**，不用拼 URL 参数。返回新窗口的 label；建不出来返回 None。
pub fn create(app: &AppHandle, root: Option<String>, frame: Option<Frame>, paths: Vec<String>) -> Option<String> {
    let st = app.state::<AppState>();
    let label = st.windows.next_label();
    let mut cfg = app.config().app.windows.first()?.clone();
    cfg.label = label.clone();
    cfg.title = title_for(root.as_deref());

    let frame = frame.filter(|f| on_screen(app, f)).or_else(|| {
        // 没有存下来的位置：从前台窗口往右下错开一点，不要正好盖住它
        st.windows
            .front()
            .and_then(|l| st.windows.frame(&l))
            .map(|f| Frame { x: f.x + CASCADE, y: f.y + CASCADE, ..f })
            .filter(|f| on_screen(app, f))
    });
    let mut b = match tauri::WebviewWindowBuilder::from_config(app, &cfg) {
        Ok(b) => b,
        Err(e) => {
            applog::write(applog::Level::Error, "window", &format!("新窗口的配置不对：{e}"));
            return None;
        }
    };
    if let Some(f) = frame {
        b = b.position(f.x, f.y).inner_size(f.w, f.h);
    }
    let w = match b.build() {
        Ok(w) => w,
        Err(e) => {
            applog::write(applog::Level::Error, "window", &format!("建不出新窗口：{e}"));
            return None;
        }
    };
    // 材质层每个窗口都要挂一次，不挂就是一扇透明的空窗（lib.rs 那段注释）
    crate::apply_window_material(&w);
    st.windows.register(&label);
    if st.windows.set_root(&label, root) {
        recent_changed(app);
    }
    note_frame(app, &w);
    // 极少数情况下它的前端已经抢先取过收件箱了：那就直接发
    if st.windows.assign(&label, paths.clone()) && !paths.is_empty() {
        let _ = app.emit_to(label.as_str(), crate::open::EVENT, &paths);
    }
    crate::diag!("新窗口 {label} paths={paths:?}");
    save_soon();
    Some(label)
}

/// 启动时把上次开着的窗口开回来（3.5 第 4 条）。
///
/// 第一个窗口 `main` 是配置自动建的，它拿计划里**最久没碰**的那个；其余的按顺序新建，
/// 最后建的那个（上次在前台的）落在最前面。没有计划（第一次跑、文件坏了）就什么都不做，
/// `main` 照老路自己恢复会话。
pub fn restore_at_launch(app: &AppHandle) {
    let saved = load(app);
    let plan = crate::windows::restore_plan(&saved, |r| std::path::Path::new(r).is_dir());
    let st = app.state::<AppState>();
    st.windows.load(&saved, &plan);
    let main = app.get_webview_window("main");
    let mut it = plan.into_iter();
    let Some(first) = it.next() else {
        // 没有可恢复的：main 照默认的位置，也要记下来（理由见 note_frame）
        if let Some(w) = &main {
            note_frame(app, w);
        }
        return;
    };
    if let Some(w) = main {
        if let Some(f) = first.frame.filter(|f| on_screen(app, f)) {
            let _ = w.set_position(tauri::LogicalPosition::new(f.x, f.y));
            let _ = w.set_size(tauri::LogicalSize::new(f.w, f.h));
        }
        note_frame(app, &w);
        if let Some(r) = &first.root {
            let _ = w.set_title(&title_for(Some(r)));
            // 按建窗口的顺序（最久没碰的先）依次记进「最近打开」，最后前台那个排最前 ——
            // 和上次退出时一致。前端起来再报同一个根时不会再动它（见 set_root）
            st.windows.set_root("main", Some(r.clone()));
            st.windows.assign("main", vec![r.clone()]);
        }
    }
    for s in it {
        let paths = s.root.clone().into_iter().collect();
        create(app, s.root, s.frame, paths);
    }
}

/// 点 Dock 图标、而且一个看得见的窗口都没有（#41）。
///
/// 窗口其实还在、只是最小化了：把它们放回来，不另开。真的一个都没有：开回最近关掉的、
/// 项目目录还在的那个（项目和位置），连那个都没有就开一个空窗口。
pub fn reopen(app: &AppHandle) {
    let st = app.state::<AppState>();
    if st.windows.len() > 0 {
        for w in app.webview_windows().values() {
            let _ = w.unminimize();
            let _ = w.show();
        }
        if let Some(w) = st.windows.front().and_then(|l| app.get_webview_window(&l)) {
            let _ = w.set_focus();
        }
        return;
    }
    match st.windows.take_closed(|r| std::path::Path::new(r).is_dir()) {
        Some(SavedWin { root, frame }) => {
            let paths = root.clone().into_iter().collect();
            create(app, root, frame, paths);
        }
        None => {
            create(app, None, None, Vec::new());
        }
    }
}

/// 菜单项按下去，而一个窗口都没有（#41：关掉最后一个窗口应用还在 Dock 上）。
///
/// 只有「不需要现有窗口」的几项有意义，其余的此时都是灰的（最后一个窗口销毁时
/// `lib.rs` 把菜单状态刷成全假）。
pub fn menu_without_window(app: &AppHandle, id: &str) {
    if id == "open-folder" {
        // 原生面板要阻塞等人选，不能在主线程上等（同 commands::pick_folder 的注释）
        let app = app.clone();
        std::thread::spawn(move || {
            use tauri_plugin_dialog::DialogExt;
            if let Some(p) = app.dialog().file().set_title("打开文件夹").blocking_pick_folder() {
                crate::open::deliver(&app, vec![p.to_string()]);
            }
        });
    } else if let Some(path) = id.strip_prefix(crate::menu::RECENT_PREFIX) {
        crate::open::deliver(app, vec![path.to_string()]);
    } else if id == "new-scratch" {
        // 开一个空窗口，记一笔「起来之后新建草稿」，它的前端在启动流程里取走（理由见 Windows::want_scratch）
        if let Some(label) = create(app, None, None, Vec::new()) {
            app.state::<AppState>().windows.want_scratch(&label);
        }
    } else {
        crate::diag!("菜单 {id}：没有窗口可以接");
    }
}

/// 我们自己的「退出」（3.6 第 3 条）。
///
/// predefined 的「退出」走 AppKit 的 `terminate:`，进程直接结束，前端没有任何机会落盘 ——
/// 第 0 步实测 `pagehide` 里写的东西一条都没留下。所以先给每个窗口发 `flush`，各自存完现场
/// （会话快照 + 自动保存）回一句 `quit_ready`，全部回齐或者等满 [`QUIT_WAIT`]，再退。
///
/// Dock 右键「退出」、注销、关机不经过菜单项，仍然直接 `terminate:` —— 那几条路只有定时落盘兜底。
pub fn quit(app: &AppHandle) {
    let st = app.state::<AppState>();
    let Some(wait) = st.windows.begin_quit() else { return };
    crate::diag!("退出：等 {wait:?} 回话");
    for l in &wait {
        let _ = app.emit_to(l.as_str(), "flush", ());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let t = Instant::now();
        while app.state::<AppState>().windows.quit_pending() > 0 && t.elapsed() < QUIT_WAIT {
            std::thread::sleep(Duration::from_millis(20));
        }
        let left = app.state::<AppState>().windows.quit_pending();
        if left > 0 {
            applog::write(applog::Level::Warn, "window", &format!("退出时有 {left} 个窗口 2 秒内没回话，没等它们"));
        }
        crate::diag!("退出：等了 {}ms，没回话的 {left} 个", t.elapsed().as_millis());
        save_now(&app);
        app.exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /*
     * 审查查出来的：三条线程同时写同一个临时文件，落盘的可能是半截。
     * 每条线程写一份「整份都是同一个字母」的内容，读回来的必须是某一份完整的 ——
     * 混了字母或者长度不对，就是写到一半被别人截断 / rename 走了。
     * 验红：去掉 save_to 里那把锁，这条在本机几轮之内就红。
     */
    #[test]
    fn 几个线程同时存盘_落下的总是完整的某一份() {
        let dir = std::env::temp_dir().join(format!("lite-ide-winsave-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("windows.json");
        let bad = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|sc| {
            for c in ['a', 'b', 'c', 'd'] {
                let (path, bad) = (&path, &bad);
                sc.spawn(move || {
                    let text: String = std::iter::repeat_n(c, 256 * 1024).collect();
                    for _ in 0..40 {
                        save_to(path, || Some(text.clone()));
                        if let Ok(got) = std::fs::read_to_string(path) {
                            let first = got.chars().next();
                            if got.len() != text.len() || got.chars().any(|x| Some(x) != first) {
                                bad.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                });
            }
        });
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(bad.into_inner(), 0, "读到了半截或者拼起来的文件");
    }
}
