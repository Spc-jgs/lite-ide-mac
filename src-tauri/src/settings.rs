//! 配置文件（issue #44，docs/SETTINGS.md）：解析、默认值、校验、界面状态。
//!
//! # 只做决定，不碰 Tauri、不碰盘
//!
//! 同 `windows.rs` 的理由：SETTINGS.md 第 6 节那张「读不对的时候」是这件事最容易错的东西 ——
//! 哪种错整份作废、哪种错只丢一个键、运行中改坏了留哪一份 —— 要能在裸单测里一条条测到。
//! 读盘、监听、广播在第 2 步的那一层。
//!
//! # 两份文件两个主人（SETTINGS.md 第 3 节）
//!
//! - `settings.json`：**你写的**，JSONC（能写注释、末尾逗号），扁平点号键。应用只读，只在点「设置…」而它不存在时
//!   建一份模板（[`template`]），之后一个字节都不碰 —— 改写会冲掉你的注释和排版。
//! - `ui-state.json`：**应用写的**（[`UiState`]）：缩略图、文件树紧凑 / 跟随、Git 分组、⌘= 调出来的字号偏移。
//!   按钮一天按几十次，不该去改你手写的那份。
//!
//! # 键只定义一次
//!
//! [`DEFS`] 是唯一的一份：名字、类型、默认值、范围、一句说明。校验、模板、前端的补全（第 2 步的 `settings_schema`）
//! 全从它来 —— 前端不另抄一份键名，抄两份迟早一处漏改。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 一个设置项的类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 字符串。`empty_ok`：空串有意义（「跟编辑器一样」「用 `$SHELL`」），否则空串当写错
    Str { empty_ok: bool },
    /// 整数，超出范围夹回来
    Int { min: i64, max: i64 },
}

/// 一个设置项
#[derive(Debug)]
pub struct Def {
    pub key: &'static str,
    pub kind: Kind,
    /// 默认值，**JSON 字面量**（`"\"JetBrains Mono\""`、`"13"`）：模板原样写进去，补全原样给出去
    pub default: &'static str,
    /// 一句说明：模板里那行注释、补全的提示
    pub doc: &'static str,
}

/// 字号的范围（编辑器、终端、⌘= 夹的都是它）。和原来 App.svelte 里 ⌘= 夹的 9–28 一样
pub const FONT_MIN: i64 = 9;
pub const FONT_MAX: i64 = 28;
pub const FONT_DEFAULT: i64 = 13;

/// 全部设置项（第一批，SETTINGS.md 第 5 节）。**顺序就是模板里的顺序**
pub const DEFS: &[Def] = &[
    Def {
        key: "editor.fontFamily",
        kind: Kind::Str { empty_ok: false },
        default: "\"JetBrains Mono\"",
        doc: "代码字体：编辑器、差异、日志视图。写具体的字体名；系统里没有就退回 JetBrains Mono",
    },
    Def {
        key: "editor.fontSize",
        kind: Kind::Int { min: FONT_MIN, max: FONT_MAX },
        default: "13",
        doc: "代码字号（9–28）。⌘= / ⌘- 在它上面临时加减，⌘0 回到这里写的值",
    },
    Def {
        key: "terminal.fontFamily",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "终端字体。空 = 跟 editor.fontFamily 一样",
    },
    Def {
        key: "terminal.fontSize",
        kind: Kind::Int { min: FONT_MIN, max: FONT_MAX },
        default: "13",
        doc: "终端字号（9–28）",
    },
    Def {
        key: "terminal.shell",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "终端用的 shell，写完整路径（如 \"/opt/homebrew/bin/fish\"）。空 = 用系统的 $SHELL。只对新开的终端生效",
    },
];

fn def(key: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.key == key)
}

/// 生效的设置。字段和 [`DEFS`] 一一对应（`从_defs_取默认值_和_settings_default_一致` 那条测试卡着）
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub editor_font_family: String,
    pub editor_font_size: i64,
    /// 空 = 跟编辑器
    pub terminal_font_family: String,
    pub terminal_font_size: i64,
    /// 空 = `$SHELL`
    pub terminal_shell: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            editor_font_family: "JetBrains Mono".into(),
            editor_font_size: FONT_DEFAULT,
            terminal_font_family: String::new(),
            terminal_font_size: FONT_DEFAULT,
            terminal_shell: String::new(),
        }
    }
}

impl Settings {
    /// 把一个**已经校验过**的值放进对应的字段
    fn put(&mut self, key: &str, v: &Value) {
        let s = || v.as_str().unwrap_or_default().to_string();
        let n = || v.as_i64().unwrap_or(FONT_DEFAULT);
        match key {
            "editor.fontFamily" => self.editor_font_family = s(),
            "editor.fontSize" => self.editor_font_size = n(),
            "terminal.fontFamily" => self.terminal_font_family = s(),
            "terminal.fontSize" => self.terminal_font_size = n(),
            "terminal.shell" => self.terminal_shell = s(),
            _ => {}
        }
    }
}

/// 文件里的一个位置。`line` 从 1 数；`col` 从 1 数，**按 UTF-16 数** —— 前端 CM6 的位置是 JS 字符串下标，
/// 拿到就能用。`serde_json` 报的列是按字节数的（实测：「中文」后面那个字符报第 16 列，其实是第 12 个字），不能直接给出去
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// 读的时候发现的问题。**每一种都要告诉你**（SETTINGS.md 第 6 节）—— 静默用默认值会让人以为自己的设置生效了
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// JSON 本身坏了：整份用不了
    Syntax { pos: Pos, msg: String },
    /// 最外层不是 `{ … }`：整份用不了
    NotObject,
    /// 不认识的键 —— 拼错时（`editor.fontsize`）这是唯一的线索
    Unknown { key: String, pos: Pos },
    /// 类型不对：只有这个键用默认值
    WrongType { key: &'static str, pos: Pos, want: &'static str },
    /// 超出范围：夹回来
    Clamped { key: &'static str, pos: Pos, from: i64, to: i64 },
    /// 同一个键写了两次：后写的算数（和 JSON 解析器的行为一致），提醒一句
    Duplicate { key: String, pos: Pos },
}

impl Problem {
    /// 整份文件用不了的那两种
    pub fn is_fatal(&self) -> bool {
        matches!(self, Problem::Syntax { .. } | Problem::NotObject)
    }

    pub fn pos(&self) -> Option<Pos> {
        match self {
            Problem::Syntax { pos, .. }
            | Problem::Unknown { pos, .. }
            | Problem::WrongType { pos, .. }
            | Problem::Clamped { pos, .. }
            | Problem::Duplicate { pos, .. } => Some(*pos),
            Problem::NotObject => None,
        }
    }

    /// 给人看的一句话（状态栏、编辑器里那一行的诊断）
    pub fn text(&self) -> String {
        match self {
            Problem::Syntax { msg, .. } => format!("设置文件写错了：{msg}"),
            Problem::NotObject => "设置文件最外层要是一个 { … }".into(),
            Problem::Unknown { key, .. } => format!("不认识的设置：{key}"),
            Problem::WrongType { key, want, .. } => format!("{key} 要是{want}，这次用的默认值"),
            Problem::Clamped { key, from, to, .. } => format!("{key} 是 {from}，超出范围，按 {to} 算"),
            Problem::Duplicate { key, .. } => format!("{key} 写了不止一次，按最后一个算"),
        }
    }
}

/// 读出来的结果：生效的设置 + 一路上的问题
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub settings: Settings,
    pub problems: Vec<Problem>,
}

/// 读 `settings.json`。**不抛**（启动路径上的规矩），规则见 SETTINGS.md 第 6 节：
///
/// - 文件不存在（`text` 是 None）→ 全默认，不报任何问题
/// - 整份坏了（语法错、最外层不是对象）→ 有 `prev`（运行中热加载）就**留着上一份好的**，没有（启动时）就全默认；
///   报这一条问题。改到一半按了 ⌘S，字体不该先跳回默认再跳回来
/// - 某个键有问题 → 只有这个键用默认值 / 夹回来，别的照常
pub fn load(text: Option<&str>, prev: Option<&Settings>) -> Parsed {
    let Some(text) = text else {
        return Parsed { settings: Settings::default(), problems: Vec::new() };
    };
    match parse(text) {
        Ok(p) => p,
        Err(problem) => Parsed { settings: prev.cloned().unwrap_or_default(), problems: vec![problem] },
    }
}

/// 解析一份文本。`Err` 只在整份用不了时（[`Problem::is_fatal`]）
pub fn parse(text: &str) -> Result<Parsed, Problem> {
    let clean = strip(text)?;
    // 空文件（或者只有注释）当 `{}`：刚建出来、还没写东西，不算错
    if clean.trim().is_empty() {
        return Ok(Parsed { settings: Settings::default(), problems: Vec::new() });
    }
    let value: Value = serde_json::from_str(&clean).map_err(|e| Problem::Syntax {
        pos: pos_of_line_byte(text, e.line(), e.column()),
        // 最常见的是没写完（少个 `}`、引号没收尾），给句人话；别的保留 serde_json 的原文（去掉它自己带的行列，我们另算）
        msg: if e.classify() == serde_json::error::Category::Eof {
            "没写完：可能少了 } 或者引号没收尾".into()
        } else {
            e.to_string().split(" at line ").next().unwrap_or_default().to_string()
        },
    })?;
    let Value::Object(map) = value else { return Err(Problem::NotObject) };

    let mut problems = Vec::new();
    // 键的位置要自己找：serde_json 的 Map 不记位置。在剥过注释的文本上找 —— 被注释掉的键已经是空格了，不会被认成真的
    let keys = top_level_keys(&clean);
    let mut seen: Vec<&str> = Vec::new();
    for (k, off) in &keys {
        let pos = pos_of_offset(text, *off);
        if def(k).is_none() {
            problems.push(Problem::Unknown { key: k.clone(), pos });
        } else if seen.contains(&k.as_str()) {
            problems.push(Problem::Duplicate { key: k.clone(), pos });
        }
        seen.push(k);
    }
    // 生效的是最后一次出现的那个（serde_json 的 Map 后写的覆盖先写的），问题也标在那一行
    let last_pos = |key: &str| keys.iter().rev().find(|(k, _)| k == key).map(|(_, off)| pos_of_offset(text, *off));

    let mut settings = Settings::default();
    for d in DEFS {
        let Some(v) = map.get(d.key) else { continue };
        let pos = last_pos(d.key).unwrap_or(Pos { line: 1, col: 1 });
        match check(d, v) {
            Check::Ok => settings.put(d.key, v),
            Check::Clamp(from, to) => {
                problems.push(Problem::Clamped { key: d.key, pos, from, to });
                settings.put(d.key, &Value::from(to));
            }
            Check::Wrong(want) => problems.push(Problem::WrongType { key: d.key, pos, want }),
        }
    }
    Ok(Parsed { settings, problems })
}

enum Check {
    Ok,
    Clamp(i64, i64),
    Wrong(&'static str),
}

fn check(d: &Def, v: &Value) -> Check {
    match d.kind {
        Kind::Str { empty_ok } => match v.as_str() {
            Some(s) if empty_ok || !s.trim().is_empty() => Check::Ok,
            Some(_) => Check::Wrong("非空的字符串"),
            None => Check::Wrong("字符串"),
        },
        // `14.0`、`"14"` 都算写错：要的是整数。`as_i64` 对 `14.0` 也给 None
        Kind::Int { min, max } => match v.as_i64() {
            Some(n) if n < min || n > max => Check::Clamp(n, n.clamp(min, max)),
            Some(_) => Check::Ok,
            None => Check::Wrong("整数"),
        },
    }
}

/// 把注释和末尾逗号换成**等长的空格**，交给 `serde_json`。
///
/// 换成空格而不是删掉：剥完之后每个字节的位置不变，`serde_json` 报的行列、[`top_level_keys`] 找到的偏移，
/// 拿回原文上就对得上。注释里的多字节字符整段换掉（一个汉字三个空格），所以结果仍是合法的 UTF-8。
///
/// 要认的边界：字符串里的 `//`（`"http://…"` 不是注释）、字符串里的 `\"`、`/* */` 跨行（换行留着，行号才不乱）。
/// 没收尾的 `/*` 算语法错 —— 不然它后面的设置全被当成注释吞掉，而你看不出为什么不生效。
fn strip(text: &str) -> Result<String, Problem> {
    let mut b = text.as_bytes().to_vec();
    let n = b.len();
    let mut i = 0;
    let mut in_str = false;
    while i < n {
        let c = b[i];
        if in_str {
            match c {
                b'\\' => i += 1, // 跳过被转义的那个字节（`\"` 不结束字符串）
                b'"' => in_str = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match (c, b.get(i + 1)) {
            (b'"', _) => {
                in_str = true;
                i += 1;
            }
            (b'/', Some(b'/')) => {
                while i < n && b[i] != b'\n' {
                    b[i] = b' ';
                    i += 1;
                }
            }
            (b'/', Some(b'*')) => {
                let start = i;
                b[i] = b' ';
                b[i + 1] = b' ';
                i += 2;
                loop {
                    if i + 1 >= n {
                        return Err(Problem::Syntax { pos: pos_of_offset(text, start), msg: "块注释 /* 没有收尾的 */".into() });
                    }
                    if b[i] == b'*' && b[i + 1] == b'/' {
                        b[i] = b' ';
                        b[i + 1] = b' ';
                        i += 2;
                        break;
                    }
                    if b[i] != b'\n' {
                        b[i] = b' ';
                    }
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    // 第二遍：末尾逗号。注释已经是空格了，`,  // 说明\n}` 这种也认得出
    let mut in_str = false;
    let mut i = 0;
    while i < n {
        let c = b[i];
        if in_str {
            match c {
                b'\\' => i += 1,
                b'"' => in_str = false,
                _ => {}
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b',' {
            let next = b[i + 1..].iter().find(|x| !x.is_ascii_whitespace());
            if matches!(next, Some(b'}') | Some(b']')) {
                b[i] = b' ';
            }
        }
        i += 1;
    }
    // 只把 ASCII 字节换成了空格、整段换掉了多字节字符，结果一定是合法的 UTF-8
    Ok(String::from_utf8(b).unwrap_or_default())
}

/// 最外层对象的每个键和它在文本里的字节偏移（按出现顺序，重复的也都在）。
/// 在剥过注释的文本上跑：字符串、嵌套的 `{ [` 都要跳过，只认深度 1 上「字符串后面跟着 `:`」的那种
fn top_level_keys(clean: &str) -> Vec<(String, usize)> {
    let b = clean.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.saturating_sub(1),
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let end = (i + 1).min(b.len());
                let after = b[end..].iter().find(|x| !x.is_ascii_whitespace());
                if depth == 1 && after == Some(&b':') {
                    // 用 JSON 自己的规则把转义解开（`"ab"` 就是 `ab`），解不开就原样
                    let raw = &clean[start..end];
                    let key = serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.trim_matches('"').to_string());
                    out.push((key, start));
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// 字节偏移 → 行列（列按 UTF-16 数，见 [`Pos`]）
fn pos_of_offset(text: &str, off: usize) -> Pos {
    let off = off.min(text.len());
    let before = &text[..floor_char(text, off)];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    Pos { line, col: before[line_start..].encode_utf16().count() + 1 }
}

/// `serde_json` 给的（行，按字节数的列）→ [`Pos`]
fn pos_of_line_byte(text: &str, line: usize, byte_col: usize) -> Pos {
    let start: usize = text.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum();
    pos_of_offset(text, start + byte_col.saturating_sub(1))
}

/// 往前退到字符边界（偏移落在多字节字符中间时）
fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 「设置…」时 `settings.json` 不存在就建这一份：全部设置项都列出来、**全都注释着** ——
/// 没写的就是默认值，所以模板本身等于「什么都没改」。去掉行首的 `// ` 就生效。
pub fn template() -> String {
    let mut s = String::from(
        "{\n  // lite-ide 的设置。只写你要改的，没写的就是默认值；存盘立刻生效，不用重启。\n  // 下面列的是全部设置项和它们的默认值，去掉行首的 // 就生效。\n",
    );
    for d in DEFS {
        s.push_str(&format!("\n  // {}\n  // \"{}\": {},\n", d.doc, d.key, d.default));
    }
    s.push_str("}\n");
    s
}

// ─────────────────────────── ui-state.json ───────────────────────────

/// `ui-state.json` 的格式版本。字段含义变了就 +1，读到不认识的整份当没有（同 `windows.rs` 的 `SAVED_VERSION`）
pub const UI_VERSION: u32 = 1;

/// 按钮和快捷键改的界面状态（SETTINGS.md 3.2）。**应用自己写**，你不用管它。
///
/// 键名和 `settings.json` 一样用点号写法，将来要把某一项挪进 `settings.json` 时不用改名
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub v: u32,
    #[serde(rename = "editor.minimap")]
    pub minimap: bool,
    #[serde(rename = "tree.compact")]
    pub tree_compact: bool,
    #[serde(rename = "tree.follow")]
    pub tree_follow: bool,
    #[serde(rename = "git.grouped")]
    pub git_grouped: bool,
    /// ⌘= / ⌘- 在 `editor.fontSize` 上加减的量（SETTINGS.md 3.3）。⌘0 清零
    #[serde(rename = "editor.fontSizeOffset")]
    pub font_offset: i64,
}

impl Default for UiState {
    /// 和原来 localStorage 里各处的默认值一致（App / FileTree / GitPane 的 `readPref(…, 默认)`）
    fn default() -> Self {
        UiState { v: UI_VERSION, minimap: true, tree_compact: true, tree_follow: true, git_grouped: false, font_offset: 0 }
    }
}

/// 读 `ui-state.json`。坏了、版本不对一律当没有，**不报** —— 它是应用自己写的，你没写过它，报给你也没用
/// （同 `windows::parse_saved`）。缺的字段用默认值（`serde(default)`），所以以后加字段不用升版本
pub fn parse_ui(text: &str) -> UiState {
    match serde_json::from_str::<UiState>(text) {
        Ok(u) if u.v == UI_VERSION => u,
        _ => UiState::default(),
    }
}

pub fn serialize_ui(u: &UiState) -> String {
    serde_json::to_string_pretty(u).unwrap_or_default()
}

impl UiState {
    /// 改一个开关（`set_ui_state` 命令）。返回变了没有；键不认识或者值不是布尔，`Err` 说哪里不对。
    /// 字号偏移不走这里，走 [`UiState::step_font`] —— 它要夹范围，而且要以 Rust 这边的当前值为准（两个窗口同时按 ⌘=）
    pub fn set(&mut self, key: &str, v: bool) -> Result<bool, String> {
        let slot = match key {
            "editor.minimap" => &mut self.minimap,
            "tree.compact" => &mut self.tree_compact,
            "tree.follow" => &mut self.tree_follow,
            "git.grouped" => &mut self.git_grouped,
            _ => return Err(format!("不认识的界面状态：{key}")),
        };
        let changed = *slot != v;
        *slot = v;
        Ok(changed)
    }

    /// ⌘= / ⌘- / ⌘0。`delta` 是 None 就清零（回到 `settings.json` 里写的字号）。
    /// 实际字号夹在 9–28：在 28 上再按 ⌘=，偏移不再涨 —— 不然要按好几下 ⌘- 才看得到变化。返回变了没有
    pub fn step_font(&mut self, base: i64, delta: Option<i64>) -> bool {
        let next = match delta {
            None => 0,
            Some(d) => (font_size(base, self.font_offset) + d).clamp(FONT_MIN, FONT_MAX) - base,
        };
        let changed = next != self.font_offset;
        self.font_offset = next;
        changed
    }

    /// 升级后第一次启动：localStorage 里那 5 个旧偏好（前端读出来交给 Rust，同 `adopt_recent`）。
    /// 旧的 `editorFont` 是绝对字号，换算成相对默认字号的偏移 —— 迁移不建 `settings.json`，基础字号就是默认的 13
    pub fn from_legacy(minimap: Option<bool>, compact: Option<bool>, follow: Option<bool>, grouped: Option<bool>, font: Option<i64>) -> UiState {
        let d = UiState::default();
        UiState {
            v: UI_VERSION,
            minimap: minimap.unwrap_or(d.minimap),
            tree_compact: compact.unwrap_or(d.tree_compact),
            tree_follow: follow.unwrap_or(d.tree_follow),
            git_grouped: grouped.unwrap_or(d.git_grouped),
            font_offset: font.map_or(0, |f| f.clamp(FONT_MIN, FONT_MAX) - FONT_DEFAULT),
        }
    }
}

/// 实际的编辑器字号：基础 + 偏移，夹在范围里
pub fn font_size(base: i64, offset: i64) -> i64 {
    (base + offset).clamp(FONT_MIN, FONT_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Parsed {
        parse(text).unwrap_or_else(|p| panic!("整份解析失败：{p:?}\n{text}"))
    }

    // ── 键的定义只有一份，别的都跟着它 ──

    #[test]
    fn 从_defs_取默认值_和_settings_default_一致() {
        // DEFS 里的默认值字面量和 Settings::default() 是两处写的，靠这条卡住不分叉
        let mut s = Settings::default();
        for d in DEFS {
            let v: Value = serde_json::from_str(d.default).unwrap_or_else(|_| panic!("{} 的默认值不是合法 JSON", d.key));
            assert!(matches!(check(d, &v), Check::Ok), "{} 的默认值过不了自己的校验", d.key);
            s.put(d.key, &v);
        }
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn 模板什么都没改_每一行去掉注释都是合法的() {
        let t = template();
        assert_eq!(ok(&t), Parsed { settings: Settings::default(), problems: vec![] }, "模板本身 = 全默认、没有问题");
        for d in DEFS {
            assert!(t.contains(&format!("// \"{}\": {},", d.key, d.default)), "模板漏了 {}", d.key);
        }
        // 把所有设置行的 `// ` 去掉：照样全默认、没有问题 —— 模板里写的默认值和真正的默认值一致
        let live: String = t.lines().map(|l| l.replacen("// \"", "\"", 1) + "\n").collect();
        assert_eq!(ok(&live), Parsed { settings: Settings::default(), problems: vec![] }, "\n{live}");
    }

    // ── 第 6 节：读不对的时候 ──

    #[test]
    fn 文件不存在_全默认_不报() {
        assert_eq!(load(None, None), Parsed { settings: Settings::default(), problems: vec![] });
    }

    #[test]
    fn 空文件和只有注释_当作什么都没改() {
        for t in ["", "  \n", "// 只有一行注释\n", "/* 块 */"] {
            assert_eq!(ok(t).problems, vec![], "{t:?}");
        }
    }

    #[test]
    fn 启动时整份坏了_全默认_报出第几行() {
        let p = load(Some("{\n  \"editor.fontSize\": 14\n  \"terminal.fontSize\": 15\n}"), None);
        assert_eq!(p.settings, Settings::default());
        assert!(matches!(p.problems.as_slice(), [Problem::Syntax { pos: Pos { line: 3, .. }, .. }]), "{:?}", p.problems);
    }

    #[test]
    fn 运行中改坏了_留着上一份好的_不退回默认() {
        let good = ok("{\"editor.fontFamily\": \"Fira Code\"}").settings;
        let p = load(Some("{\"editor.fontFamily\": \"Fira Co"), Some(&good));
        assert_eq!(p.settings, good, "改到一半按了 ⌘S：字体不该先跳回默认");
        assert!(p.problems[0].is_fatal());
        let p = load(Some("[1, 2]"), Some(&good));
        assert_eq!((p.settings, p.problems), (good, vec![Problem::NotObject]), "最外层不是对象也一样");
    }

    #[test]
    fn 一个键类型不对_只有它用默认值() {
        let p = ok("{\n  \"editor.fontFamily\": \"Fira Code\",\n  \"editor.fontSize\": \"14\",\n  \"terminal.fontSize\": 14.5\n}");
        assert_eq!(p.settings.editor_font_family, "Fira Code", "别的键照常");
        assert_eq!(p.settings.editor_font_size, FONT_DEFAULT);
        assert_eq!(p.settings.terminal_font_size, FONT_DEFAULT, "14.5 不是整数");
        assert_eq!(
            p.problems,
            vec![
                Problem::WrongType { key: "editor.fontSize", pos: Pos { line: 3, col: 3 }, want: "整数" },
                Problem::WrongType { key: "terminal.fontSize", pos: Pos { line: 4, col: 3 }, want: "整数" },
            ]
        );
        let p = ok("{\"editor.fontFamily\": \"  \"}");
        assert_eq!(p.settings.editor_font_family, "JetBrains Mono", "字体不能是空的");
        assert!(matches!(p.problems.as_slice(), [Problem::WrongType { want: "非空的字符串", .. }]));
        assert_eq!(ok("{\"terminal.shell\": \"\"}").problems, vec![], "shell 空 = 用 $SHELL，是合法的");
    }

    #[test]
    fn 不认识的键_拼错时要说出来() {
        let p = ok("{\n  \"editor.fontsize\": 20\n}");
        assert_eq!(p.settings, Settings::default());
        assert_eq!(p.problems, vec![Problem::Unknown { key: "editor.fontsize".into(), pos: Pos { line: 2, col: 3 } }]);
    }

    #[test]
    fn 超出范围_夹回来并且说一声() {
        let p = ok("{\"editor.fontSize\": 200, \"terminal.fontSize\": 2}");
        assert_eq!((p.settings.editor_font_size, p.settings.terminal_font_size), (FONT_MAX, FONT_MIN));
        assert!(matches!(p.problems[0], Problem::Clamped { key: "editor.fontSize", from: 200, to: 28, .. }));
        assert!(matches!(p.problems[1], Problem::Clamped { key: "terminal.fontSize", from: 2, to: 9, .. }));
    }

    #[test]
    fn 同一个键写两次_后写的算数_标在后一处() {
        let p = ok("{\n  \"editor.fontSize\": 14,\n  \"editor.fontSize\": 16\n}");
        assert_eq!(p.settings.editor_font_size, 16);
        assert_eq!(p.problems, vec![Problem::Duplicate { key: "editor.fontSize".into(), pos: Pos { line: 3, col: 3 } }]);
    }

    // ── JSONC：注释、末尾逗号，以及不能误伤的地方 ──

    #[test]
    fn 注释和末尾逗号() {
        let p = ok(r#"{
  // 行注释 "editor.fontSize": 99,
  /* 块注释
     "editor.fontSize": 98, 跨行 */
  "editor.fontSize": 14, // 行尾注释
  "terminal.fontSize": 15,   /* 末尾逗号后面还跟着注释 */
}"#);
        assert_eq!((p.settings.editor_font_size, p.settings.terminal_font_size), (14, 15));
        assert_eq!(p.problems, vec![], "被注释掉的键不该被认成重复或者生效");
    }

    #[test]
    fn 字符串里的斜杠和转义引号不是注释() {
        let p = ok(r#"{"terminal.shell": "/usr/local/bin//fish", "editor.fontFamily": "a \"//b\" /* c */,}"}"#);
        assert_eq!(p.settings.terminal_shell, "/usr/local/bin//fish");
        assert_eq!(p.settings.editor_font_family, r#"a "//b" /* c */,}"#);
        assert_eq!(p.problems, vec![]);
    }

    #[test]
    fn 多行块注释后面的错_行号照样对() {
        // 剥注释时换行要留着：换成空格的话 serde_json 数出来的行号会往前错
        let e = parse("{\n  /* 一\n     二\n     三 */\n  \"editor.fontSize\": 14\n  \"terminal.fontSize\": 15\n}").unwrap_err();
        assert!(matches!(e, Problem::Syntax { pos: Pos { line: 6, .. }, .. }), "{e:?}");
        let e = parse("{\n  \"editor.fontSize\": 14,\n").unwrap_err();
        assert!(matches!(&e, Problem::Syntax { msg, .. } if msg.contains("没写完")), "{e:?}");
    }

    #[test]
    fn 没收尾的块注释_算语法错_不能把后面的设置悄悄吞掉() {
        let e = parse("{\n  /* 忘了收尾\n  \"editor.fontSize\": 14\n}").unwrap_err();
        assert!(matches!(e, Problem::Syntax { pos: Pos { line: 2, col: 3 }, .. }), "{e:?}");
    }

    #[test]
    fn 嵌套的对象里的键不算顶层键() {
        // 现在没有对象类型的设置，嵌套的整个当一个不认识的键；里面的 "editor.fontSize" 不能被当成顶层的。
        // 顶层也写一个同名的才看得出来：认错了会多报一条「写了两次」、超范围的位置也会标到嵌套那一处
        // （第一版只放了嵌套的那个，验红时把「只认深度 1」改掉它照样绿）
        let p = ok("{\n  \"editor.fontSize\": 99,\n  \"x\": {\"editor.fontSize\": 20}\n}");
        assert_eq!(p.settings.editor_font_size, FONT_MAX);
        assert_eq!(
            p.problems,
            vec![
                Problem::Unknown { key: "x".into(), pos: Pos { line: 3, col: 3 } },
                Problem::Clamped { key: "editor.fontSize", pos: Pos { line: 2, col: 3 }, from: 99, to: FONT_MAX },
            ]
        );
    }

    // ── 位置：中文和 emoji ──

    #[test]
    fn 报错的列按_utf16_数_中文和_emoji_都对() {
        // 「中文」在 UTF-8 里 6 个字节、UTF-16 里 2 个；😀 在 UTF-8 里 4 个、UTF-16 里 2 个
        let e = parse("{\"editor.fontFamily\": \"中文😀\" x}").unwrap_err();
        let Problem::Syntax { pos, .. } = e else { panic!("{e:?}") };
        let want = "{\"editor.fontFamily\": \"中文😀\" ".encode_utf16().count() + 1;
        assert_eq!(pos, Pos { line: 1, col: want }, "serde_json 按字节报列，要换算");
        let p = ok("{\n  /* 注释里有中文 */ \"editor.fontsize\": 1\n}");
        let want = "  /* 注释里有中文 */ ".encode_utf16().count() + 1;
        assert_eq!(p.problems[0].pos(), Some(Pos { line: 2, col: want }), "同一行前面有中文注释");
    }

    // ── ui-state.json ──

    #[test]
    fn 界面状态_坏了当没有_缺字段用默认_版本不对当没有() {
        assert_eq!(parse_ui("{"), UiState::default());
        assert_eq!(parse_ui(r#"{"v": 1, "editor.minimap": false}"#), UiState { minimap: false, ..UiState::default() });
        assert_eq!(parse_ui(r#"{"v": 99, "editor.minimap": false}"#), UiState::default());
        let u = UiState { git_grouped: true, font_offset: 3, ..UiState::default() };
        assert_eq!(parse_ui(&serialize_ui(&u)), u, "写出去的要读得回来");
    }

    #[test]
    fn 界面状态_改开关() {
        let mut u = UiState::default();
        assert_eq!(u.set("editor.minimap", false), Ok(true));
        assert_eq!(u.set("editor.minimap", false), Ok(false), "没变");
        assert!(u.set("editor.fontSizeOffset", true).is_err(), "字号偏移不走这里");
        assert!(u.set("nope", true).is_err());
    }

    #[test]
    fn 字号_基础加偏移_夹在范围里_零回到配置里写的() {
        let mut u = UiState::default();
        assert!(u.step_font(14, Some(1)));
        assert_eq!(font_size(14, u.font_offset), 15);
        for _ in 0..30 {
            u.step_font(14, Some(1));
        }
        assert_eq!(font_size(14, u.font_offset), FONT_MAX);
        assert_eq!(u.font_offset, FONT_MAX - 14, "到顶之后偏移不再涨，按一下 ⌘- 就能看到变小");
        assert!(u.step_font(14, Some(-1)));
        assert_eq!(font_size(14, u.font_offset), FONT_MAX - 1);
        assert!(u.step_font(14, None));
        assert_eq!(font_size(14, u.font_offset), 14, "⌘0 回到 settings.json 里写的，不是写死的 13");
        assert!(!u.step_font(14, None), "已经是零：没变");
    }

    #[test]
    fn 旧偏好迁过来_字号换算成偏移() {
        let u = UiState::from_legacy(Some(false), None, Some(false), Some(true), Some(16));
        assert_eq!(u, UiState { minimap: false, tree_follow: false, git_grouped: true, font_offset: 3, ..UiState::default() });
        assert_eq!(font_size(FONT_DEFAULT, u.font_offset), 16, "迁完字号和原来一样");
        assert_eq!(UiState::from_legacy(None, None, None, None, None), UiState::default(), "什么都没存过");
        assert_eq!(UiState::from_legacy(None, None, None, None, Some(99)).font_offset, FONT_MAX - FONT_DEFAULT, "手改过的怪值也夹住");
    }
}
