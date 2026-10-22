//! 键位的覆盖（docs/TAKEOVER.md R3）：`keymap.ts` 是默认，你改过的存 `keymap.json`（只存改过的），生效的 = 默认 ⊕ 覆盖。
//!
//! 这里是不碰盘、不碰菜单的那一半（解析、换算、合起来），动手的在 `keymapctl.rs` —— 同 `settings.rs` / `settingsctl.rs` 的拆法。
//!
//! **Rust 只认「菜单那一份」**：哪个键给谁、撞没撞（「同一个键不许挂两个动作」）由前端判，那张全表（连同编辑器自己吃的键）只有
//! `keymap.ts` 有。这里只管：读得懂、换得成菜单认的写法、归 keydown 的那几项不许挂到菜单上（见 [`KEY_OWNED_IN_MENU`]）。

use std::collections::BTreeMap;
use std::sync::Mutex;

/// 在菜单里只有标签、键归 window 级 keydown 的那几项（`keymap.ts` 里 owner = key）。**覆盖里写了它们也不挂到菜单上**：
/// 挂上 AppKit 就先把键吃掉 —— ⌃R / ⌃D 在终端里是 shell 的搜历史 / 输入结束，这正是它们不进菜单的理由。
/// 由 `tests/menu_sync.rs` 卡着和 keymap.ts 一致
pub const KEY_OWNED_IN_MENU: [&str; 4] = ["quick-all", "recent-files", "run-last", "debug-last"];

/// 一个键位能不能认（显示写法：修饰键按 ⌃⌥⇧⌘ 的次序，后面一个键）。空串 = 「不要键位」，也算对
pub fn accel_ok(accel: &str) -> bool {
    accel.is_empty() || split(accel).is_some()
}

/// `⌥⌘L` → `(["Alt", "CmdOrCtrl"], "L")`。修饰键次序不对、没有主键、主键不认识都是 None
fn split(accel: &str) -> Option<(Vec<&'static str>, String)> {
    let mut mods = Vec::new();
    let mut rest = accel;
    for (sym, name) in [("⌃", "Ctrl"), ("⌥", "Alt"), ("⇧", "Shift"), ("⌘", "CmdOrCtrl")] {
        if let Some(r) = rest.strip_prefix(sym) {
            mods.push(name);
            rest = r;
        }
    }
    let key = key_name(rest)?;
    // 不是功能键就要带 ⌃ ⌥ ⌘ 之一（F1、F7 这种是 IDEA 的常用键，可以不带）：裸的字母、⇧ + 字母挂进菜单等于打不了字
    if !(key.starts_with('F') && key.len() > 1) && mods.iter().all(|m| *m == "Shift") {
        return None;
    }
    Some((mods, key))
}

/// 显示写法的主键 → muda 认的名字（读 muda 0.19 `accelerator.rs` 的 `parse_key`：字母数字、F1–F24、方向键、标点、
/// `ENTER` / `BACKSPACE` / `TAB` / `SPACE` / `ESCAPE` / `DELETE`）。**和 keymap.ts 的 `KEY_NAMES` 是同一张表**
fn key_name(k: &str) -> Option<String> {
    let named = match k {
        "←" => "Left",
        "→" => "Right",
        "↑" => "Up",
        "↓" => "Down",
        "↵" => "Enter",
        "⌫" => "Backspace",
        "⌦" => "Delete",
        "Tab" => "Tab",
        "Space" => "Space",
        "Esc" => "Escape",
        "`" => "Backquote",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    let mut cs = k.chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase().to_string()),
        (Some(c), None) if "-=[]\\;',./".contains(c) => Some(c.to_string()),
        (Some('F'), Some(_)) => k[1..].parse::<u8>().ok().filter(|n| (1..=20).contains(n)).map(|_| k.to_string()),
        _ => None,
    }
}

/// `⇧⌘F` → `Shift+CmdOrCtrl+F`（Tauri / muda 的 accelerator 写法）。认不了是 None
pub fn to_tauri_accel(accel: &str) -> Option<String> {
    let (mods, key) = split(accel)?;
    let mut out: Vec<&str> = mods;
    out.push(&key);
    Some(out.join("+"))
}

/// `keymap.json` 的原文 → 覆盖表。形状是 `{ "动作 id": "⌥⌘L" }`，`""` = 不要键位。
/// 读不懂的整份不用（返回一句说明）、认不了的那一条丢掉（也说一句）—— 坏了一条不该连别的一起没了
pub fn parse(text: &str) -> (BTreeMap<String, String>, Vec<String>) {
    let mut out = BTreeMap::new();
    let mut problems = Vec::new();
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return (out, vec![format!("keymap.json 读不懂，整份没用上：{e}")]),
    };
    let Some(obj) = v.as_object() else { return (out, vec!["keymap.json 要是一个对象（{ \"动作\": \"键位\" }），整份没用上".into()]) };
    for (id, a) in obj {
        match a.as_str() {
            Some(a) if accel_ok(a) => {
                out.insert(id.clone(), a.to_string());
            }
            _ => problems.push(format!("keymap.json 里 `{id}` 的键位 {a} 认不了，这一条没用上")),
        }
    }
    (out, problems)
}

/// 覆盖表 → `keymap.json` 的原文。整份是我们写的（不像 settings.json 有人的注释要留），按 id 排、两格缩进
pub fn render(map: &BTreeMap<String, String>) -> String {
    let mut s = serde_json::to_string_pretty(map).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

/// 菜单项在默认之外有没有被改过，改成了什么（Tauri 的写法）。`None` = 没改，用默认；`Some(None)` = 改成不要键位
pub fn menu_override(over: &BTreeMap<String, String>, id: &str) -> Option<Option<String>> {
    if KEY_OWNED_IN_MENU.contains(&id) {
        return None;
    }
    over.get(id).map(|a| to_tauri_accel(a))
}

/// 进程里生效的那份覆盖（启动时读、设置里改了换掉）。菜单建的时候和改了之后都从这儿取
pub static OVERRIDES: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// 菜单项的默认键位（Tauri 写法），建菜单时 `menu::item` 记下来：改回默认、或者覆盖被删掉时要换回去
pub static DEFAULTS: Mutex<BTreeMap<String, Option<String>>> = Mutex::new(BTreeMap::new());

/// 菜单项此刻真挂着的键位（Tauri 写法）。muda 的菜单项没有「读键位」的口子，挂的时候自己记一笔 —— 测试通道读它（`testbridge` 的 `accel`）
pub static APPLIED: Mutex<BTreeMap<String, Option<String>>> = Mutex::new(BTreeMap::new());

/// 建菜单的一项时问：这一项挂什么键。顺手把默认、挂上的记下来
pub fn menu_accel(id: &str, default: Option<&str>) -> Option<String> {
    DEFAULTS.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), default.map(str::to_string));
    let over = OVERRIDES.lock().unwrap_or_else(|e| e.into_inner());
    let a = match menu_override(&over, id) {
        Some(a) => a,
        None => default.map(str::to_string),
    };
    APPLIED.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), a.clone());
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 换算_和菜单原来那份一致_加上回车退格这些() {
        assert_eq!(to_tauri_accel("⌘S").as_deref(), Some("CmdOrCtrl+S"));
        assert_eq!(to_tauri_accel("⇧⌘F").as_deref(), Some("Shift+CmdOrCtrl+F"));
        assert_eq!(to_tauri_accel("⌃⇧`").as_deref(), Some("Ctrl+Shift+Backquote"));
        assert_eq!(to_tauri_accel("⌥⌘←").as_deref(), Some("Alt+CmdOrCtrl+Left"));
        assert_eq!(to_tauri_accel("⌃↵").as_deref(), Some("Ctrl+Enter"));
        assert_eq!(to_tauri_accel("⌘⌫").as_deref(), Some("CmdOrCtrl+Backspace"));
        assert_eq!(to_tauri_accel("⇧F8").as_deref(), Some("Shift+F8"));
        assert_eq!(to_tauri_accel("F9").as_deref(), Some("F9"));
        assert_eq!(to_tauri_accel("⌘/").as_deref(), Some("CmdOrCtrl+/"));
    }

    #[test]
    fn 认不了的_次序错_没主键_裸字母() {
        assert_eq!(to_tauri_accel("⌘⇧F"), None, "修饰键次序要按 ⌃⌥⇧⌘");
        assert_eq!(to_tauri_accel("⌘"), None);
        assert_eq!(to_tauri_accel("L"), None, "裸字母挂进菜单就打不了字");
        assert_eq!(to_tauri_accel("⇧L"), None, "⇧ + 字母就是一个大写字母");
        assert_eq!(to_tauri_accel("↵"), None);
        assert_eq!(to_tauri_accel("F99"), None);
        assert_eq!(to_tauri_accel("⌘é"), None);
        assert!(accel_ok(""), "空 = 不要键位");
    }

    #[test]
    fn 解析_坏一条只丢那一条_整份坏了说一句() {
        let (m, p) = parse(r#"{ "save": "⌥⌘S", "git-fetch": "", "bad": "⌘⇧X", "num": 3 }"#);
        assert_eq!(m.get("save").map(String::as_str), Some("⌥⌘S"));
        assert_eq!(m.get("git-fetch").map(String::as_str), Some(""));
        assert_eq!(m.len(), 2);
        assert_eq!(p.len(), 2, "{p:?}");
        let (m, p) = parse("{ 坏的");
        assert!(m.is_empty() && p.len() == 1);
        let (m, _) = parse(&render(&BTreeMap::from([("save".to_string(), "⌥⌘S".to_string())])));
        assert_eq!(m.get("save").map(String::as_str), Some("⌥⌘S"), "写出去的读得回来");
    }

    #[test]
    fn 菜单_改了用改的_改成空就不挂_归keydown的那几项不理() {
        let over = BTreeMap::from([
            ("save".to_string(), "⌥⌘S".to_string()),
            ("git-push".to_string(), String::new()),
            ("run-last".to_string(), "⌘R".to_string()),
        ]);
        assert_eq!(menu_override(&over, "save"), Some(Some("Alt+CmdOrCtrl+S".into())));
        assert_eq!(menu_override(&over, "git-push"), Some(None));
        assert_eq!(menu_override(&over, "close-tab"), None);
        assert_eq!(menu_override(&over, "run-last"), None, "⌃R 挂进菜单，终端里就搜不了历史");
    }
}
