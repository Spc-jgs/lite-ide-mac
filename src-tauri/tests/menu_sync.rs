//! 菜单栏和 `keymap.ts` 必须对得上。
//!
//! 键位的唯一出处是前端的 `src/lib/state/keymap.ts`（菜单栏、快捷键速查、
//! 随处搜索的 hint 三处都从它取）。但菜单要在窗口出现之前就建好，
//! 那时前端还没起来 —— 所以 `menu.rs` 里是**一份拷贝**。
//!
//! 两份拷贝靠自觉迟早会漂移，而漂移**不报错**：
//!
//! - 文案漂了 → 菜单上写着「保存」，速查表里写着「保存当前文件」
//! - 键位漂了 → 菜单上写着 ⌘S，按下去没反应
//! - `owner` 漂了 → ⌘F 混进菜单，CM6 的查找被静默抢走
//!
//! 判据和 `dto_sync.rs` 一样：**只读源码，比两边的集合**。
//! 造价是这一个文件，拦住的是一整类看不见的 bug。

use std::collections::BTreeMap;

const MENU_RS: &str = include_str!("../src/menu.rs");
const KEYMAP_TS: &str = include_str!("../../src/lib/state/keymap.ts");

#[derive(Debug, PartialEq)]
struct Entry {
    label: String,
    accel: Option<String>,
}

/// 从 `keymap.ts` 里抠出 `{ id, label, accel?, gesture?, owner }`。
///
/// 手写扫描而不是引 JSON/JS 解析器：这张表是**人手维护的字面量**，
/// 形状固定（每条一个对象，字段名是裸标识符），值得为它省掉一个依赖。
fn parse_keymap() -> BTreeMap<String, (Entry, String)> {
    let mut out = BTreeMap::new();
    // 把整张表拍平成一行一条：条目可能跨行（带注释、格式化换行）
    let flat: String = KEYMAP_TS
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("*") && !t.starts_with("/*")
        })
        .collect::<Vec<_>>()
        .join(" ");

    // 逐个 `{ ... }` 扫过去，只认同时带 id 和 owner 的那些
    let mut rest = flat.as_str();
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let body = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];

        let (Some(id), Some(owner)) = (field(body, "id"), field(body, "owner")) else {
            continue;
        };
        let Some(label) = field(body, "label") else { continue };
        out.insert(id, (Entry { label, accel: field(body, "accel") }, owner));
    }
    out
}

/// `name: "值"` → `Some("值")`。只认双引号，表里就是这么写的。
fn field(body: &str, name: &str) -> Option<String> {
    let pat = format!("{name}:");
    let i = body.find(&pat)?;
    let after = &body[i + pat.len()..];
    let q = after.find('"')?;
    // 冒号和引号之间只允许空白 —— 否则 `menuAccel:` 会被 `accel:` 误匹配
    if !after[..q].trim().is_empty() {
        return None;
    }
    let end = after[q + 1..].find('"')?;
    Some(after[q + 1..q + 1 + end].to_string())
}

/// 从 `menu.rs` 里抠出 `item(app, "id", "标签", Some("Accel"))` / `None`。
fn parse_menu() -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    for raw in MENU_RS.split("item(app, \"").skip(1) {
        let Some(id_end) = raw.find('"') else { continue };
        let id = raw[..id_end].to_string();
        let after = &raw[id_end + 1..];
        // 标签
        let Some(lq) = after.find('"') else { continue };
        let Some(le) = after[lq + 1..].find('"') else { continue };
        let label = after[lq + 1..lq + 1 + le].to_string();
        // accel：紧接着要么是 Some("...") 要么是 None
        let tail = &after[lq + 1 + le + 1..];
        let accel = tail.find("Some(\"").and_then(|s| {
            // 只看这次调用之内（到第一个 `)?` 为止），别串到下一条去
            let stop = tail.find(")?").unwrap_or(tail.len());
            if s > stop {
                return None;
            }
            let a = &tail[s + 6..];
            a.find('"').map(|e| a[..e].to_string())
        });
        out.insert(id, Entry { label, accel });
    }
    out
}

/// `⇧⌘F` → `Shift+CmdOrCtrl+F`。**必须和 keymap.ts 的 toTauriAccel 同一套规则。**
fn to_tauri_accel(accel: &str) -> Option<String> {
    let mut mods = Vec::new();
    let mut rest = accel;
    for (sym, name) in [("⌃", "Ctrl"), ("⌥", "Alt"), ("⇧", "Shift"), ("⌘", "CmdOrCtrl")] {
        if let Some(r) = rest.strip_prefix(sym) {
            mods.push(name);
            rest = r;
        }
    }
    if rest.is_empty() {
        return None;
    }
    let key = if rest == "`" { "Backquote".to_string() } else { rest.to_uppercase() };
    mods.push(&key);
    Some(mods.join("+"))
}

#[test]
fn 菜单项的文案和键位必须和_keymap_ts_一致() {
    let keymap = parse_keymap();
    let menu = parse_menu();

    assert!(keymap.len() > 15, "keymap.ts 只解析出 {} 条，解析器怕是坏了", keymap.len());
    assert!(menu.len() > 10, "menu.rs 只解析出 {} 条，解析器怕是坏了", menu.len());

    for (id, item) in &menu {
        let Some((want, owner)) = keymap.get(id) else {
            panic!("菜单里有 `{id}`，而 keymap.ts 里没有 —— 键位表是唯一出处，先去那儿登记");
        };
        assert_ne!(
            owner, "cm6",
            "`{id}` 在 keymap.ts 里归 CM6，却出现在菜单里。\n\
             菜单项带 accelerator 会先把键吃掉 —— 这等于把编辑器的查找静默抢没了"
        );

        // 标签：菜单允许把手势写进括号里（「随处搜索（连按两下 ⇧）」），
        // 除此之外必须一字不差
        let ok = item.label == want.label
            || item.label.starts_with(&format!("{}（", want.label));
        assert!(ok, "`{id}` 文案对不上：菜单「{}」/ keymap「{}」", item.label, want.label);

        match owner.as_str() {
            "menu" => {
                let expect = want.accel.as_deref().and_then(to_tauri_accel);
                assert_eq!(
                    item.accel, expect,
                    "`{id}` 键位对不上：菜单 {:?} / keymap {:?} 推出的 {:?}",
                    item.accel, want.accel, expect
                );
            }
            "key" => assert_eq!(
                item.accel, None,
                "`{id}` 在 keymap.ts 里归 keydown，菜单项就不能挂 accelerator ——\n\
                 挂了 AppKit 会先把键吃掉，keydown 分支永远轮不到"
            ),
            other => panic!("`{id}` 的 owner 是 `{other}`，没见过"),
        }
    }

    // 反向：归菜单的每一条都得真的在菜单里，否则那个键位谁都没接
    for (id, (_, owner)) in &keymap {
        if owner == "menu" {
            assert!(menu.contains_key(id), "`{id}` 在 keymap.ts 里归菜单，菜单里却没有它");
        }
    }
}

#[test]
fn 解析器本身不是空转() {
    // 两个解析器只要有一个悄悄返回空集，上面那条测试就会全部通过而什么也没测
    let keymap = parse_keymap();
    assert_eq!(keymap.get("save").map(|(e, _)| e.accel.as_deref()), Some(Some("⌘S")));
    assert_eq!(keymap.get("cm-find").map(|(_, o)| o.as_str()), Some("cm6"));
    assert_eq!(keymap.get("quick-file").map(|(_, o)| o.as_str()), Some("key"));

    let menu = parse_menu();
    assert_eq!(menu.get("save").map(|e| e.accel.as_deref()), Some(Some("CmdOrCtrl+S")));
    assert_eq!(menu.get("quick-file").map(|e| e.accel.as_deref()), Some(None), "⌘P 不该挂 accelerator");
    assert!(!menu.contains_key("cm-find"), "⌘F 不许出现在菜单里");
}

#[test]
fn 两侧的_accel_换算规则一致() {
    assert_eq!(to_tauri_accel("⌘S").as_deref(), Some("CmdOrCtrl+S"));
    assert_eq!(to_tauri_accel("⇧⌘F").as_deref(), Some("Shift+CmdOrCtrl+F"));
    assert_eq!(to_tauri_accel("⌃⇧`").as_deref(), Some("Ctrl+Shift+Backquote"));
    assert_eq!(to_tauri_accel("⌘/").as_deref(), Some("CmdOrCtrl+/"));
    assert_eq!(to_tauri_accel("⌘"), None, "只有修饰键时不该产出 muda 解析不了的串");
}
