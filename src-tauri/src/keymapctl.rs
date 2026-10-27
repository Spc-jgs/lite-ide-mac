//! `keymap.json` 的读盘、监听、存盘、广播（docs/TAKEOVER.md R3）。决定在 `keymap.rs`，这里只接线（同 `settingsctl.rs`）。
//!
//! 放在 `settings.json` 旁边、单独一份：键位是一张「动作 → 键」的表，不是一个个设置项（`settings::DEFS` 那套是一项一个值、
//! 只改那一个值的字符）；VS Code 的 `keybindings.json`、IDEA 的 keymap XML 也都和设置分开放。只存改过的。

use crate::keymap;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// 键位变了：广播给**所有**窗口（同 `settings-changed`），负载是全量的覆盖表
pub const EVENT: &str = "keymap-changed";

pub fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("keymap.json"))
}

static WATCH: Mutex<Option<fsservice::watch::FileWatch>> = Mutex::new(None);

/// 读一遍、换上，读不懂的写进应用日志。没有这份文件 = 没改过任何键
fn load(app: &AppHandle) -> bool {
    let text = path(app).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let (map, problems) = if text.trim().is_empty() { (BTreeMap::new(), vec![]) } else { keymap::parse(&text) };
    for p in &problems {
        applog::write(applog::Level::Warn, "keymap", p);
    }
    let mut cur = keymap::OVERRIDES.lock().unwrap_or_else(|e| e.into_inner());
    if *cur == map {
        return false;
    }
    *cur = map;
    true
}

/// 启动时、**建菜单之前**（`setup` 里）：菜单一建好就该挂着你改过的键，不该先挂默认的、等前端起来再换
pub fn init(app: &AppHandle) {
    load(app);
    let Some(p) = path(app) else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // 手改 keymap.json 也当场生效（VS Code 的 keybindings.json 也是）；起不来监听就要重启才生效，记一笔不挡启动
    let a = app.clone();
    match fsservice::watch::watch_file(&p, move || {
        if load(&a) {
            changed(&a);
        }
    }) {
        Ok(w) => *WATCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(w),
        Err(e) => applog::write(applog::Level::Warn, "keymap", &format!("监听不了 keymap.json，手改了要重启才生效：{e}")),
    }
}

pub fn current() -> BTreeMap<String, String> {
    keymap::OVERRIDES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 菜单换上、告诉每个窗口（编辑器里的键在前端）
fn changed(app: &AppHandle) {
    crate::menu::apply_keys(app, false);
    let _ = app.emit(EVENT, current());
}

/// 设置里「应用 / 确定」：整张覆盖表换掉、写盘。认不了的一条都不收（前端已经判过，这里是最后一道）
pub fn set(app: &AppHandle, map: BTreeMap<String, String>) -> Result<(), String> {
    if let Some((id, a)) = map.iter().find(|(_, a)| !keymap::accel_ok(a)) {
        return Err(format!("`{id}` 的快捷键 {a} 无效"));
    }
    let p = path(app).ok_or("无法获取应用数据目录")?;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // 先换内存、再写盘：写完文件监听会来重读，那时内存里已经是这份、`load` 认出没变就不再广播一遍。写失败换回去
    let text = keymap::render(&map);
    let old = std::mem::replace(&mut *keymap::OVERRIDES.lock().unwrap_or_else(|e| e.into_inner()), map);
    if let Err(e) = fsservice::write_text(&p, &text) {
        *keymap::OVERRIDES.lock().unwrap_or_else(|e| e.into_inner()) = old;
        return Err(format!("无法写入 {}：{e}", p.display()));
    }
    changed(app);
    Ok(())
}

/// 设置里开始 / 停止录键：录的那几秒菜单上一个键都不挂（理由在 `menu::apply_keys`）。停了按生效的挂回去
pub fn recording(app: &AppHandle, on: bool) {
    crate::menu::apply_keys(app, on);
}
