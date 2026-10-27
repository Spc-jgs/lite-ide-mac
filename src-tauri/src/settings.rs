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
    /// true / false
    Bool,
    /// 字符串列表（手动添加的 JDK 目录）
    StrList,
}

/// 一个设置项
#[derive(Debug)]
pub struct Def {
    pub key: &'static str,
    /// 设置页上那一项的名字
    pub label: &'static str,
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
        label: "代码字体",
        kind: Kind::Str { empty_ok: false },
        default: "\"JetBrains Mono\"",
        doc: "代码字体：编辑器、差异、日志视图。写具体的字体名；系统里没有就退回 JetBrains Mono",
    },
    Def {
        key: "editor.fontSize",
        label: "代码字号",
        kind: Kind::Int { min: FONT_MIN, max: FONT_MAX },
        default: "13",
        doc: "代码字号（9–28）。⌘= / ⌘- 在它上面临时加减，⌘0 回到这里写的值",
    },
    // ── Java 智能的三样「编辑器里看得见的」（docs/TAKEOVER.md G11）：照 IDEA 默认开着，嫌吵的关掉。键归 editor.：以后别的语言服务器也用 ──
    Def {
        key: "editor.highlightUsages",
        label: "光标停在名字上时，标出它在这个文件里的用法",
        kind: Kind::Bool,
        default: "true",
        doc: "光标停在一个名字上，这个文件里它出现的地方铺一层淡底、写的那几处换暖色（IDEA 的 Highlight usages）。按意思找的，同名的别的变量不标。目前只有 Java",
    },
    Def {
        key: "editor.inlayHints",
        label: "参数名提示",
        kind: Kind::Bool,
        default: "true",
        doc: "调用时传字面量的参数前面一小块灰字写参数名（apply(cents: 100)），不进正文。目前只有 Java",
    },
    // 粘性行（docs/TAKEOVER.md G19）：照 IDEA 默认开着。靠编辑器自己的语法树，不分语言
    Def {
        key: "editor.stickyLines",
        label: "滚动时把所在的类、方法钉在顶上",
        kind: Kind::Bool,
        default: "true",
        doc: "滚到一个长方法中间时，它所在的类、方法、if / for / try 那一行钉在编辑器顶上，点一下跳过去（IDEA 的 Sticky Lines）。最多 4 行",
    },
    Def {
        key: "terminal.fontFamily",
        label: "终端字体",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "终端字体。空 = 跟 editor.fontFamily 一样",
    },
    Def {
        key: "terminal.fontSize",
        label: "终端字号",
        kind: Kind::Int { min: FONT_MIN, max: FONT_MAX },
        default: "13",
        doc: "终端字号（9–28）",
    },
    Def {
        key: "terminal.shell",
        label: "终端用的 shell",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "终端用的 shell，写完整路径（如 \"/opt/homebrew/bin/fish\"）。空 = 用系统的 $SHELL。只对新开的终端生效",
    },
    Def {
        key: "java.jdtlsHome",
        label: "jdtls 的位置",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "Java 智能用的 jdtls（解开的发行包，里面有 plugins/ 和 config_mac_arm/）。空 = 内置的那份。下次起 jdtls 时生效",
    },
    // ── 设置页（2026-10-21，SETTINGS.md 第 13 节）新加的 ──
    Def {
        key: "java.jdtlsMemory",
        label: "jdtls 最多用的内存（MB）",
        kind: Kind::Int { min: JDTLS_MEMORY_MIN, max: JDTLS_MEMORY_MAX },
        default: "1024",
        doc: "jdtls 最多用多少内存（MB，512–16384）。大项目导入慢、或者报内存不够时调大。下次起 jdtls 时生效",
    },
    Def {
        key: "java.jdks",
        label: "手动添加的 JDK",
        kind: Kind::StrList,
        default: "[]",
        doc: "手动添加的 JDK（JAVA_HOME 目录，如 \"/opt/jdk-21\"）。系统、~/Library/Java、Homebrew、sdkman、asdf、jenv、mise 装的会自己找到，找不到的加在这里",
    },
    Def {
        key: "java.hotswapOnSave",
        label: "调试时存盘就自动构建、热替换",
        kind: Kind::Bool,
        default: "true",
        doc: "调试着的时候存了 .java 文件，停手不到一秒就自动构建（⌘F9）、把改过的类换进正在调试的程序，不用重启。IDEA 是存完问一句，这里不问、做完说一句换了哪几个类",
    },
    Def {
        key: "maven.useWrapper",
        label: "项目里有 mvnw 就用它",
        kind: Kind::Bool,
        default: "true",
        doc: "项目里有 mvnw（Maven Wrapper）就用它 —— 项目要的 Maven 版本写在 .mvn/wrapper/ 里，Spring Initializr 生成的项目都有",
    },
    Def {
        key: "maven.home",
        label: "Maven 安装目录",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "Maven 的安装目录（里面有 bin/mvn）。空 = PATH 里的 mvn。项目有 mvnw 而且 maven.useWrapper 开着时不看这一项",
    },
    Def {
        key: "maven.settingsFile",
        label: "settings.xml",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "Maven 的 settings.xml（mvn -s）。空 = Maven 的默认（~/.m2/settings.xml）",
    },
    Def {
        key: "maven.localRepository",
        label: "本地仓库",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "Maven 本地仓库（-Dmaven.repo.local）。空 = settings.xml 里写的，没写就是 ~/.m2/repository",
    },
    Def {
        key: "maven.extraArgs",
        label: "每条 mvn 都带的参数",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "每条 mvn 都带上的参数，照 shell 的写法（如 \"-T 4 -U\"）",
    },
    Def {
        key: "maven.opts",
        label: "MAVEN_OPTS（Maven 自己的 JVM 参数）",
        kind: Kind::Str { empty_ok: true },
        default: "\"\"",
        doc: "MAVEN_OPTS：跑 Maven 本身的 JVM 的参数（如 \"-Xmx2g\"）。不是你程序的 JVM 参数 —— 那个在运行配置里",
    },
    Def {
        key: "update.check",
        label: "自动检查更新",
        kind: Kind::Bool,
        default: "true",
        doc: "启动后和之后每 12 小时去 GitHub Release 看一眼有没有新版；有就在后台下好、验过签名，状态栏出一个「重启更新」，点了才换。不发任何关于你的信息（只是下载一个公开文件）。关掉后菜单「检查更新…」照样能手动查",
    },
];

/// jdtls 内存的范围（MB）
pub const JDTLS_MEMORY_MIN: i64 = 512;
pub const JDTLS_MEMORY_MAX: i64 = 16384;

fn def(key: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.key == key)
}

/// 生效的设置。字段和 [`DEFS`] 一一对应（`从_defs_取默认值_和_settings_default_一致` 那条测试卡着）
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub editor_font_family: String,
    pub editor_font_size: i64,
    pub editor_highlight_usages: bool,
    pub editor_inlay_hints: bool,
    pub editor_sticky_lines: bool,
    /// 空 = 跟编辑器
    pub terminal_font_family: String,
    pub terminal_font_size: i64,
    /// 空 = `$SHELL`
    pub terminal_shell: String,
    /// 空 = `<应用数据>/jdtls`（docs/IDEA.md 第 2 步）
    pub java_jdtls_home: String,
    /// MB
    pub java_jdtls_memory: i64,
    pub java_jdks: Vec<String>,
    pub java_hotswap_on_save: bool,
    pub maven_use_wrapper: bool,
    pub maven_home: String,
    pub maven_settings_file: String,
    pub maven_local_repository: String,
    pub maven_extra_args: String,
    pub maven_opts: String,
    pub update_check: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            editor_font_family: "JetBrains Mono".into(),
            editor_font_size: FONT_DEFAULT,
            editor_highlight_usages: true,
            editor_inlay_hints: true,
            editor_sticky_lines: true,
            terminal_font_family: String::new(),
            terminal_font_size: FONT_DEFAULT,
            terminal_shell: String::new(),
            java_jdtls_home: String::new(),
            java_jdtls_memory: 1024,
            java_jdks: Vec::new(),
            java_hotswap_on_save: true,
            maven_use_wrapper: true,
            maven_home: String::new(),
            maven_settings_file: String::new(),
            maven_local_repository: String::new(),
            maven_extra_args: String::new(),
            maven_opts: String::new(),
            update_check: true,
        }
    }
}

impl Settings {
    /// 把一个**已经校验过**的值放进对应的字段
    fn put(&mut self, key: &str, v: &Value) {
        let s = || v.as_str().unwrap_or_default().to_string();
        let n = || v.as_i64().unwrap_or(FONT_DEFAULT);
        let b = || v.as_bool().unwrap_or_default();
        let list = || v.as_array().into_iter().flatten().filter_map(Value::as_str).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
        match key {
            "editor.fontFamily" => self.editor_font_family = s(),
            "editor.fontSize" => self.editor_font_size = n(),
            "editor.highlightUsages" => self.editor_highlight_usages = b(),
            "editor.inlayHints" => self.editor_inlay_hints = b(),
            "editor.stickyLines" => self.editor_sticky_lines = b(),
            "terminal.fontFamily" => self.terminal_font_family = s(),
            "terminal.fontSize" => self.terminal_font_size = n(),
            "terminal.shell" => self.terminal_shell = s(),
            "java.jdtlsHome" => self.java_jdtls_home = s(),
            "java.jdtlsMemory" => self.java_jdtls_memory = v.as_i64().unwrap_or(1024),
            "java.jdks" => self.java_jdks = list(),
            "java.hotswapOnSave" => self.java_hotswap_on_save = b(),
            "maven.useWrapper" => self.maven_use_wrapper = b(),
            "update.check" => self.update_check = b(),
            "maven.home" => self.maven_home = s(),
            "maven.settingsFile" => self.maven_settings_file = s(),
            "maven.localRepository" => self.maven_local_repository = s(),
            "maven.extraArgs" => self.maven_extra_args = s(),
            "maven.opts" => self.maven_opts = s(),
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
    /// `terminal.shell` 指的文件不存在或者不能执行：用 `$SHELL`
    BadShell { path: String, pos: Pos },
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
            | Problem::Duplicate { pos, .. }
            | Problem::BadShell { pos, .. } => Some(*pos),
            Problem::NotObject => None,
        }
    }

    /// 给人看的一句话（状态栏、编辑器里那一行的诊断）
    pub fn text(&self) -> String {
        match self {
            Problem::Syntax { msg, .. } => format!("设置文件格式错误：{msg}"),
            Problem::NotObject => "设置文件的顶层必须是对象 { … }".into(),
            Problem::Unknown { key, .. } => format!("未知的设置项：{key}"),
            Problem::WrongType { key, want, .. } => format!("{key} 应为{want}，已使用默认值"),
            Problem::Clamped { key, from, to, .. } => format!("{key} 的值 {from} 超出范围，已按 {to} 处理"),
            Problem::Duplicate { key, .. } => format!("{key} 重复出现，以最后一个为准"),
            Problem::BadShell { path, .. } => format!("terminal.shell 指定的 {path} 不存在或不可执行，终端已改用 $SHELL"),
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
            "没写完，可能少了 } 或者引号没收尾".into()
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
        Kind::Bool => match v {
            Value::Bool(_) => Check::Ok,
            _ => Check::Wrong(" true 或 false"),
        },
        Kind::StrList => match v.as_array() {
            Some(a) if a.iter().all(Value::is_string) => Check::Ok,
            _ => Check::Wrong("字符串的列表（[\"…\", \"…\"]）"),
        },
    }
}

/// 把注释和末尾逗号换成**等长的空格**，交给 `serde_json`。`.lite-ide/tasks.json` 也用它（`taskdefs.rs`，#48）：一种 JSONC 一个解析器。
///
/// 换成空格而不是删掉：剥完之后每个字节的位置不变，`serde_json` 报的行列、[`top_level_keys`] 找到的偏移，
/// 拿回原文上就对得上。注释里的多字节字符整段换掉（一个汉字三个空格），所以结果仍是合法的 UTF-8。
///
/// 要认的边界：字符串里的 `//`（`"http://…"` 不是注释）、字符串里的 `\"`、`/* */` 跨行（换行留着，行号才不乱）。
/// 没收尾的 `/*` 算语法错 —— 不然它后面的设置全被当成注释吞掉，而你看不出为什么不生效。
pub(crate) fn strip(text: &str) -> Result<String, Problem> {
    let mut b = strip_comments(text)?.into_bytes();
    let n = b.len();
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

/// [`strip`] 的第一步：只把注释换成等长的空格，**末尾逗号留着**。设置页定点改写要用它（[`edit`]）：
/// 改写要知道「这个成员后面原来有没有逗号」，而 `strip` 连末尾逗号也抹成了空格 —— 拿它判，`16,` 会被当成没逗号、再补一个，
/// 写出 `16,,`（真 .app 验收撞出来的；单元测试的样例最后一项碰巧都没写逗号）
fn strip_comments(text: &str) -> Result<String, Problem> {
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
pub(crate) fn pos_of_line_byte(text: &str, line: usize, byte_col: usize) -> Pos {
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

/// `terminal.shell` 写了一个用不了的路径：退回 `$SHELL`（空串），报一条问题，标在那个键上
fn check_shell(p: &mut Parsed, text: &str, ok: impl Fn(&str) -> bool) {
    let path = p.settings.terminal_shell.trim().to_string();
    if path.is_empty() || ok(&path) {
        return;
    }
    let pos = strip(text)
        .ok()
        .and_then(|clean| top_level_keys(&clean).into_iter().rev().find(|(k, _)| k == "terminal.shell"))
        .map_or(Pos { line: 1, col: 1 }, |(_, off)| pos_of_offset(text, off));
    p.settings.terminal_shell = String::new();
    p.problems.push(Problem::BadShell { path, pos });
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

// ─────────────────────────── 设置页写回 settings.json（SETTINGS.md 13.5）───────────────────────────

/// 设置页交来的一个值能不能写：认识这个键、类型对、在范围里。和读文件时用的是同一个 [`check`] —— 写进去的东西读回来一定认
pub fn validate(key: &str, v: &Value) -> Result<(), String> {
    let d = def(key).ok_or_else(|| format!("未知的设置项：{key}"))?;
    match check(d, v) {
        Check::Ok => Ok(()),
        Check::Clamp(from, _) => Err(format!("{key} 是 {from}，超出范围")),
        Check::Wrong(want) => Err(format!("{key} 要是{want}")),
    }
}

/// 原文里写了哪些键（设置页上「改过」的标记、「恢复默认」只对它们有意义）。读不懂就当一个都没写
pub fn written_keys(text: &str) -> Vec<String> {
    strip(text).map(|c| top_level_keys(&c).into_iter().map(|(k, _)| k).collect()).unwrap_or_default()
}

/// 一份设置按 [`DEFS`] 的键转成 JSON 值（设置页要显示每一项现在是什么）
pub fn values(s: &Settings) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    for d in DEFS {
        let v = match d.key {
            "editor.fontFamily" => Value::from(s.editor_font_family.clone()),
            "editor.fontSize" => Value::from(s.editor_font_size),
            "editor.highlightUsages" => Value::from(s.editor_highlight_usages),
            "editor.inlayHints" => Value::from(s.editor_inlay_hints),
            "editor.stickyLines" => Value::from(s.editor_sticky_lines),
            "terminal.fontFamily" => Value::from(s.terminal_font_family.clone()),
            "terminal.fontSize" => Value::from(s.terminal_font_size),
            "terminal.shell" => Value::from(s.terminal_shell.clone()),
            "java.jdtlsHome" => Value::from(s.java_jdtls_home.clone()),
            "java.jdtlsMemory" => Value::from(s.java_jdtls_memory),
            "java.jdks" => Value::from(s.java_jdks.clone()),
            "java.hotswapOnSave" => Value::from(s.java_hotswap_on_save),
            "maven.useWrapper" => Value::from(s.maven_use_wrapper),
            "maven.home" => Value::from(s.maven_home.clone()),
            "maven.settingsFile" => Value::from(s.maven_settings_file.clone()),
            "maven.localRepository" => Value::from(s.maven_local_repository.clone()),
            "maven.extraArgs" => Value::from(s.maven_extra_args.clone()),
            "maven.opts" => Value::from(s.maven_opts.clone()),
            "update.check" => Value::from(s.update_check),
            _ => continue,
        };
        m.insert(d.key.to_string(), v);
    }
    m
}

/// 把一批改动写进 `settings.json` 的原文：`Some(值)` 设成这个值，`None` 恢复默认（删掉这一项）。
/// **只动那一个值的字符**：你写的注释、空行、键的次序都留着（VS Code 的设置编辑器同一个做法）。
///
/// - 已经写了：换值；写了不止一次的，换生效的那个（最后一个）。恢复默认：整行删掉（同一行没别的成员时），连同后面的逗号
/// - 没写、模板里有它那行注释（`// "key": 默认,`）：插在那行注释后面，和它的说明挨着
/// - 都没有：插在最后一个 `}` 前面
/// - 新插的一行末尾带逗号（JSONC 认）；插在一个没带逗号的成员后面时先给它补上逗号
///
/// 原文整份读不懂（语法错）就不写，回那条问题：在一份读不懂的文本上动刀，只会把它改得更坏。
/// 写完再解析一遍，读不懂就不交出去（防自己写错）
pub fn edit(text: &str, changes: &[(&str, Option<Value>)]) -> Result<String, Problem> {
    let mut out = text.to_string();
    if out.trim().is_empty() {
        out = "{\n}\n".into();
    }
    parse(&out)?;
    for (key, val) in changes {
        out = edit_one(&out, key, val.as_ref())?;
    }
    parse(&out)?;
    Ok(out)
}

fn edit_one(text: &str, key: &str, val: Option<&Value>) -> Result<String, Problem> {
    // 只剥注释、末尾逗号留着（[`strip_comments`]）。真离不开它的是插入（`finish_insert` 要看前一项后面有没有逗号）；
    // 换值、删除用哪种都对 —— 被剥掉的逗号是空格，删的时候跟着空格一起扫走（验过：这里换回 `strip` 测试照样绿）。和插入用同一份，少一种要想的情况
    let clean = strip_comments(text)?;
    let b = clean.as_bytes();
    let found = top_level_keys(&clean).into_iter().rev().find(|(k, _)| k == key).map(|(_, off)| off);
    let lit = val.map(|v| serde_json::to_string(v).unwrap_or_else(|_| "null".into()));
    match (found, lit) {
        (Some(off), Some(lit)) => {
            let (vs, ve) = value_span(b, off).ok_or(Problem::NotObject)?;
            Ok(format!("{}{}{}", &text[..vs], lit, &text[ve..]))
        }
        (Some(off), None) => {
            let (_, ve) = value_span(b, off).ok_or(Problem::NotObject)?;
            // 往后吃掉空格和逗号
            let mut end = ve;
            while end < b.len() && (b[end] == b' ' || b[end] == b'\t') {
                end += 1;
            }
            if end < b.len() && b[end] == b',' {
                end += 1;
            }
            // 这一行在这个成员前后都只有空白（剥过注释的文本上看：行尾的注释也算空白，跟着它一起走）→ 整行删掉
            let line_start = clean[..off].rfind('\n').map_or(0, |i| i + 1);
            let line_end = clean[end..].find('\n').map_or(clean.len(), |i| end + i);
            if clean[line_start..off].trim().is_empty() && clean[end..line_end].trim().is_empty() {
                let cut_end = (line_end + 1).min(text.len());
                Ok(format!("{}{}", &text[..line_start], &text[cut_end..]))
            } else {
                Ok(format!("{}{}", &text[..off], &text[end..]))
            }
        }
        (None, None) => Ok(text.to_string()),
        (None, Some(lit)) => {
            // 模板里它那行注释：`// "key":` 开头的一行（只认原文里整行就是注释的，真设置后面的行尾注释不算）
            let marker = format!("// \"{key}\":");
            let mut at_line = None;
            let mut pos = 0;
            for line in text.split_inclusive('\n') {
                if line.trim_start().starts_with(&marker) {
                    at_line = Some((pos, line));
                }
                pos += line.len();
            }
            let (insert_at, indent) = match at_line {
                Some((start, line)) => {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    (start + line.len(), indent.to_string())
                }
                None => {
                    // 最外层的收尾 `}`：剥过注释的文本里最后一个非空白字符
                    let close = clean.trim_end().len().checked_sub(1).filter(|&i| b[i] == b'}').ok_or(Problem::NotObject)?;
                    let line_start = clean[..close].rfind('\n').map_or(0, |i| i + 1);
                    if clean[line_start..close].trim().is_empty() {
                        (line_start, "  ".to_string())
                    } else {
                        // `{ "a": 1 }` 写在一行里：在 `}` 前面另起一行
                        let head = format!("{}\n", &text[..close]);
                        return finish_insert(&head, head.len(), "  ", key, &lit, &text[close..]);
                    }
                }
            };
            finish_insert(text, insert_at, &indent, key, &lit, "")
        }
    }
}

/// 在 `text[..at]` 后面插一行 `"key": 值,`，再接 `text[at..]`（`tail` 非空时接它）。插入点前面最后一个真成员没带逗号的，先补上
fn finish_insert(text: &str, at: usize, indent: &str, key: &str, lit: &str, tail: &str) -> Result<String, Problem> {
    let clean = strip_comments(text)?;
    let before = &clean.as_bytes()[..at.min(clean.len())];
    let mut head = text[..at].to_string();
    if let Some(i) = before.iter().rposition(|c| !c.is_ascii_whitespace()) {
        if before[i] != b'{' && before[i] != b',' {
            head.insert(i + 1, ',');
        }
    }
    let rest = if tail.is_empty() { &text[at..] } else { tail };
    let key_lit = serde_json::to_string(key).unwrap_or_default();
    Ok(format!("{head}{indent}{key_lit}: {lit},\n{rest}"))
}

/// 剥过注释的文本里、从键的引号开始，找到它的值的 [起, 止)。值可能是字符串、数、true / false / null、数组、对象
fn value_span(b: &[u8], key_off: usize) -> Option<(usize, usize)> {
    // 跳过键这个字符串
    let mut i = key_off + 1;
    while i < b.len() && b[i] != b'"' {
        if b[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i += 1;
    while i < b.len() && b[i] != b':' {
        i += 1;
    }
    i += 1;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    let mut depth = 0usize;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            match c {
                b'\\' => i += 1,
                b'"' => {
                    in_str = false;
                    if depth == 0 {
                        return Some((start, i + 1));
                    }
                }
                _ => {}
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((start, i + 1));
                    }
                }
                b',' | b'}' | b'\n' if depth == 0 => {
                    let end = start + clean_trim_end(&b[start..i]);
                    return Some((start, end));
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn clean_trim_end(s: &[u8]) -> usize {
    s.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(0, |i| i + 1)
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

// ─────────────────────────── 合起来：前端要的样子 ───────────────────────────

/// 前端拿到就能用的值：两份文件合起来、空值已经换成实际用的（终端字体空 = 跟编辑器；字号 = 基础 + 偏移）。
/// 这些合并规则放在这儿而不是前端：多个窗口各自算一遍，迟早有一个算得不一样
#[derive(Clone, Debug, PartialEq)]
pub struct Effective {
    pub editor_font_family: String,
    /// 实际字号（基础 + ⌘= 的偏移）
    pub editor_font_size: i64,
    /// `settings.json` 里写的基础字号（⌘0 回到它）
    pub editor_font_base: i64,
    pub editor_highlight_usages: bool,
    pub editor_inlay_hints: bool,
    pub editor_sticky_lines: bool,
    pub java_hotswap_on_save: bool,
    pub terminal_font_family: String,
    pub terminal_font_size: i64,
    /// 空 = `$SHELL`
    pub terminal_shell: String,
    /// 空 = `<应用数据>/jdtls`。只在 Rust 侧用（起 jdtls），不进 SettingsDto
    pub java_jdtls_home: String,
    pub minimap: bool,
    pub tree_compact: bool,
    pub tree_follow: bool,
    pub git_grouped: bool,
}

pub fn effective(s: &Settings, u: &UiState) -> Effective {
    Effective {
        editor_font_family: s.editor_font_family.clone(),
        editor_font_size: font_size(s.editor_font_size, u.font_offset),
        editor_font_base: s.editor_font_size,
        editor_highlight_usages: s.editor_highlight_usages,
        editor_inlay_hints: s.editor_inlay_hints,
        editor_sticky_lines: s.editor_sticky_lines,
        java_hotswap_on_save: s.java_hotswap_on_save,
        terminal_font_family: if s.terminal_font_family.trim().is_empty() {
            s.editor_font_family.clone()
        } else {
            s.terminal_font_family.clone()
        },
        terminal_font_size: s.terminal_font_size,
        terminal_shell: s.terminal_shell.clone(),
        java_jdtls_home: s.java_jdtls_home.clone(),
        minimap: u.minimap,
        tree_compact: u.tree_compact,
        tree_follow: u.tree_follow,
        git_grouped: u.git_grouped,
    }
}

/// 进程里唯一的一份设置（`AppState.settings`）。**跨窗口共享的东西只有 Rust 一个主人**（MULTIWINDOW.md 3.5）：
/// 原来 5 个偏好在 localStorage 里各窗口各读各的，A 里关了缩略图 B 不知道。
#[derive(Default)]
pub struct Store {
    inner: std::sync::Mutex<StoreInner>,
}

#[derive(Default)]
struct StoreInner {
    parsed: Parsed,
    /// 读过 `settings.json` 没有：没读过 = 启动时，整份坏了用默认值；读过 = 运行中，整份坏了留上一份好的
    loaded: bool,
    ui: UiState,
    /// 启动时 `ui-state.json` 不存在 —— 升级后第一次启动，等前端交来旧的 localStorage 偏好（[`Store::adopt_legacy`]）
    ui_fresh: bool,
}

impl Default for Parsed {
    fn default() -> Self {
        Parsed { settings: Settings::default(), problems: Vec::new() }
    }
}

impl Store {
    fn lock(&self) -> std::sync::MutexGuard<'_, StoreInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 读到了 `settings.json` 的这份原文（启动时、文件监听报「碰过」时）。返回生效的设置或者问题列表变了没有 ——
    /// 变了调用方才广播。
    ///
    /// **比的是解析的结果，不是「有没有事件」**：文件监听只说「碰过」不说「变了」（`fsservice::watch_file` 的注释），
    /// `touch` 一下、vim 存一个没改的文件都会来，结果一样就不广播。（第一版另外先比了一道原文，验红时删掉它测试照样绿 ——
    /// 解析是纯函数，原文一样结果必然一样，那道比较挡下的这里都会挡下，是多余的，删了。）
    /// 运行中整份坏了留着上一份好的（[`load`]），启动时没有上一份就是默认值
    ///
    /// `shell_ok`：`terminal.shell` 写的那个路径能不能用（要碰盘，调用方判，这里保持纯）。不能用就退回 `$SHELL`
    /// 并报一条 [`Problem::BadShell`] —— SETTINGS.md 第 6 节：静默退回会让人以为自己的 shell 生效了
    pub fn load_settings(&self, text: Option<String>, shell_ok: impl Fn(&str) -> bool) -> bool {
        let mut g = self.lock();
        let prev = g.loaded.then(|| g.parsed.settings.clone());
        let mut next = load(text.as_deref(), prev.as_ref());
        check_shell(&mut next, text.as_deref().unwrap_or_default(), shell_ok);
        g.loaded = true;
        let changed = next != g.parsed;
        g.parsed = next;
        changed
    }

    /// 启动时读到的 `ui-state.json`（None = 不存在）
    pub fn load_ui(&self, text: Option<&str>) {
        let mut g = self.lock();
        g.ui = text.map(parse_ui).unwrap_or_default();
        g.ui_fresh = text.is_none();
    }

    /// `settings.json` 里生效的那份（设置页要拿它的原样值，Maven / JDK 那些要下发给各自的使用方）
    pub fn current(&self) -> Settings {
        self.lock().parsed.settings.clone()
    }

    /// 当前生效的值和问题
    pub fn view(&self) -> (Effective, Vec<Problem>) {
        let g = self.lock();
        (effective(&g.parsed.settings, &g.ui), g.parsed.problems.clone())
    }

    /// 改界面状态，返回变了没有（变了调用方存盘、广播）。`f` 拿到的是界面状态和当前的基础字号（⌘= 要用）
    pub fn update_ui(&self, f: impl FnOnce(&mut UiState, i64) -> Result<bool, String>) -> Result<bool, String> {
        let mut g = self.lock();
        let base = g.parsed.settings.editor_font_size;
        let changed = f(&mut g.ui, base)?;
        if changed {
            // 有过一次真正的改动就不再是「升级后第一次」：之后交来的旧偏好不能盖掉它
            g.ui_fresh = false;
        }
        Ok(changed)
    }

    /// 升级后第一次启动，前端交来 localStorage 里的旧偏好。**只在 `ui-state.json` 原来不存在、而且这次启动还没收过时收**
    /// —— 多窗口时每个窗口都会交一次，后面几次不能把第一次的、或者用户刚改的盖掉（同 `Windows::adopt_recent`）
    pub fn adopt_legacy(&self, legacy: UiState) -> bool {
        let mut g = self.lock();
        if !g.ui_fresh {
            return false;
        }
        g.ui_fresh = false;
        let changed = g.ui != legacy;
        g.ui = legacy;
        changed
    }

    pub fn ui_text(&self) -> String {
        serialize_ui(&self.lock().ui)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Parsed {
        parse(text).unwrap_or_else(|p| panic!("整份解析失败：{p:?}\n{text}"))
    }

    // ── 设置页写回（SETTINGS.md 13.5）──

    fn set(text: &str, key: &str, v: Value) -> String {
        edit(text, &[(key, Some(v))]).unwrap_or_else(|p| panic!("{p:?}"))
    }

    #[test]
    fn 写回_已经写了的只换值_注释和次序都在() {
        let t = "{\n  // 我平时用 14\n  \"editor.fontSize\": 14, // 投屏时改大\n  \"terminal.shell\": \"/bin/zsh\",\n}\n";
        let got = set(t, "editor.fontSize", Value::from(16));
        assert_eq!(got, "{\n  // 我平时用 14\n  \"editor.fontSize\": 16, // 投屏时改大\n  \"terminal.shell\": \"/bin/zsh\",\n}\n");
        let got = set(t, "terminal.shell", Value::from("/opt/homebrew/bin/fish"));
        assert!(got.contains("\"terminal.shell\": \"/opt/homebrew/bin/fish\","), "{got}");
        assert_eq!(ok(&got).settings.terminal_shell, "/opt/homebrew/bin/fish");
    }

    #[test]
    fn 写回_模板里注释着的_插在那行注释后面() {
        let got = set(&template(), "maven.home", Value::from("/opt/maven"));
        let lines: Vec<&str> = got.lines().collect();
        let i = lines.iter().position(|l| l.trim_start().starts_with("// \"maven.home\":")).unwrap();
        assert_eq!(lines[i + 1], "  \"maven.home\": \"/opt/maven\",", "紧挨着它那行注释");
        assert_eq!(ok(&got).settings.maven_home, "/opt/maven");
        assert_eq!(ok(&got).problems, vec![]);
    }

    #[test]
    fn 写回_都没有就插在最后_前一个没逗号的补上() {
        let got = set("{\n  \"editor.fontSize\": 14\n}\n", "maven.opts", Value::from("-Xmx2g"));
        assert_eq!(got, "{\n  \"editor.fontSize\": 14,\n  \"maven.opts\": \"-Xmx2g\",\n}\n");
        let got = set("{ \"editor.fontSize\": 14 }", "maven.useWrapper", Value::from(false));
        let p = ok(&got);
        assert_eq!((p.settings.editor_font_size, p.settings.maven_use_wrapper), (14, false), "写在一行里的也认：{got}");
        let got = set("", "java.jdks", serde_json::json!(["/opt/jdk-21"]));
        assert_eq!(ok(&got).settings.java_jdks, vec!["/opt/jdk-21".to_string()], "空文件：{got}");
    }

    #[test]
    fn 写回_带中文注释和末尾逗号的文件_插在最后也对() {
        // 真 .app 验收撞出来的（settings.sh）：这份文件写回时报「第 3 行 key must be a string」
        let t = "{\n  // 验收\n  \"editor.fontSize\": 16,\n}\n";
        let got = edit(t, &[("maven.extraArgs", Some(Value::from("-T 2")))]).unwrap_or_else(|p| panic!("{p:?}"));
        assert_eq!(got, "{\n  // 验收\n  \"editor.fontSize\": 16,\n  \"maven.extraArgs\": \"-T 2\",\n}\n");
    }

    #[test]
    fn 写回_同一行里删掉带末尾逗号的最后一项_不留两个逗号() {
        let got = edit("{ \"editor.fontSize\": 14, \"terminal.fontSize\": 12, }", &[("terminal.fontSize", None)]).unwrap_or_else(|p| panic!("{p:?}"));
        assert_eq!(ok(&got).settings.editor_font_size, 14, "{got}");
        assert!(!got.contains(",,") && !got.contains(", ,"), "{got}");
    }

    #[test]
    fn 写回_恢复默认整行删掉_行尾注释跟着走_同一行还有别的只删它自己() {
        let t = "{\n  \"editor.fontSize\": 14, // 投屏\n  \"terminal.shell\": \"/bin/zsh\"\n}\n";
        let got = edit(t, &[("editor.fontSize", None)]).unwrap();
        assert_eq!(got, "{\n  \"terminal.shell\": \"/bin/zsh\"\n}\n");
        let got = edit("{ \"editor.fontSize\": 14, \"terminal.fontSize\": 12 }", &[("editor.fontSize", None)]).unwrap();
        assert_eq!(ok(&got).settings.terminal_font_size, 12, "{got}");
        assert_eq!(ok(&got).settings.editor_font_size, FONT_DEFAULT);
        assert_eq!(edit(t, &[("maven.home", None)]).unwrap(), t, "本来就没写：原样");
    }

    #[test]
    fn 写回_值是列表和对象和含逗号的字符串_整个换掉() {
        let t = "{\n  \"java.jdks\": [\"/a\", \"/b\"],\n  \"maven.extraArgs\": \"-T 4, -U\",\n}\n";
        let got = set(t, "java.jdks", serde_json::json!(["/c"]));
        assert!(got.contains("\"java.jdks\": [\"/c\"],"), "{got}");
        let got = set(t, "maven.extraArgs", Value::from("-o"));
        assert!(got.contains("\"maven.extraArgs\": \"-o\","), "字符串里的逗号不是值的结尾：{got}");
    }

    #[test]
    fn 写回_写了两次的换生效的那个_读不懂的不动刀() {
        let got = set("{\n  \"editor.fontSize\": 12,\n  \"editor.fontSize\": 14,\n}", "editor.fontSize", Value::from(18));
        assert_eq!(ok(&got).settings.editor_font_size, 18);
        assert!(got.contains("\"editor.fontSize\": 12,"), "前面那个不生效的不碰：{got}");
        assert!(matches!(edit("{ \"editor.fontSize\": 14", &[("editor.fontSize", Some(Value::from(1)))]), Err(Problem::Syntax { .. })));
    }

    #[test]
    fn 每个键都能转成值_转回去读得一样() {
        // values() 是手写的一张对照表：漏了哪个键，设置页上那一项就是空的。这条卡住：每个 DEFS 里的键都在，而且写进文件再读回来一样
        let mut s = Settings::default();
        s.maven_home = "/opt/maven".into();
        s.java_jdks = vec!["/opt/jdk".into()];
        s.java_jdtls_memory = 2048;
        let v = values(&s);
        assert_eq!(v.len(), DEFS.len(), "漏了：{:?}", DEFS.iter().filter(|d| !v.contains_key(d.key)).map(|d| d.key).collect::<Vec<_>>());
        let text = serde_json::to_string(&v).unwrap();
        assert_eq!(ok(&text).settings, s);
        assert!(validate("maven.useWrapper", &Value::from("no")).is_err());
        assert!(validate("java.jdtlsMemory", &Value::from(100)).is_err(), "设置页交来的越界值不写（读文件时是夹回来，写的时候直接拒）");
        assert!(validate("x.y", &Value::from(1)).is_err());
        assert_eq!(written_keys("{\n // \"maven.home\": \"\",\n \"editor.fontSize\": 14 }"), vec!["editor.fontSize".to_string()], "注释掉的不算写了");
    }

    #[test]
    fn 新键的类型_布尔和列表写错了只丢这一项() {
        let p = ok("{ \"maven.useWrapper\": \"no\", \"java.jdks\": \"/opt/jdk\", \"java.jdtlsMemory\": 99999 }");
        assert!(p.settings.maven_use_wrapper, "写错了用默认（true）");
        assert!(p.settings.java_jdks.is_empty());
        assert_eq!(p.settings.java_jdtls_memory, JDTLS_MEMORY_MAX, "超出范围夹回来");
        assert_eq!(p.problems.len(), 3, "{:?}", p.problems);
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

    // ── 合起来、Store ──

    #[test]
    fn 合起来_终端字体空就跟编辑器_字号是基础加偏移() {
        let s = Settings { editor_font_family: "Fira Code".into(), editor_font_size: 14, ..Settings::default() };
        let u = UiState { font_offset: 2, minimap: false, ..UiState::default() };
        let e = effective(&s, &u);
        assert_eq!((e.terminal_font_family.as_str(), e.editor_font_size, e.editor_font_base), ("Fira Code", 16, 14));
        assert!(!e.minimap);
        let s = Settings { terminal_font_family: "Menlo".into(), ..s };
        assert_eq!(effective(&s, &u).terminal_font_family, "Menlo", "写了就用写的");
    }

    #[test]
    fn store_原文没变不算变_运行中坏了留上一份_删掉回到默认() {
        let st = Store::default();
        assert!(!st.load_settings(None, |_| true), "启动时文件不存在：和默认一样，不算变");
        assert!(st.load_settings(Some("{\"editor.fontSize\": 15}".into()), |_| true));
        assert!(!st.load_settings(Some("{\"editor.fontSize\": 15}".into()), |_| true), "touch 一下、存一个没改的：原文一样，不广播");
        assert!(st.load_settings(Some("{\"editor.fontSize\": 1".into()), |_| true), "坏了：问题列表变了，要广播（状态栏要说）");
        let (e, problems) = st.view();
        assert_eq!(e.editor_font_size, 15, "运行中改坏了：留着上一份好的");
        assert!(problems[0].is_fatal());
        assert!(st.load_settings(Some("{\"editor.fontSize\": 1}".into()), |_| true), "修好了");
        assert_eq!(st.view().0.editor_font_size, FONT_MIN);
        assert!(st.load_settings(None, |_| true));
        assert_eq!(st.view().0.editor_font_size, FONT_DEFAULT, "文件删了：回到默认");
    }

    #[test]
    fn store_启动时就坏了_没有上一份_用默认() {
        let st = Store::default();
        assert!(st.load_settings(Some("{".into()), |_| true), "问题列表从空变成一条：算变");
        assert_eq!(st.view().0, effective(&Settings::default(), &UiState::default()));
    }

    #[test]
    fn store_旧偏好只在_ui_state_原来不存在时收一次() {
        let st = Store::default();
        st.load_ui(None);
        let legacy = UiState { minimap: false, ..UiState::default() };
        assert!(st.adopt_legacy(legacy.clone()), "升级后第一次：收");
        assert!(!st.adopt_legacy(UiState::default()), "第二个窗口也交一次：不收，不能把第一次的盖回去");
        assert!(!st.view().0.minimap);

        let st = Store::default();
        st.load_ui(Some(&serialize_ui(&UiState::default())));
        assert!(!st.adopt_legacy(legacy.clone()), "ui-state.json 本来就有：迁移早做过了");

        let st = Store::default();
        st.load_ui(None);
        st.update_ui(|u, _| u.set("git.grouped", true)).unwrap();
        assert!(!st.adopt_legacy(legacy), "用户已经在新版里改过了：旧的不能盖掉它");
        assert!(st.view().0.git_grouped);
    }

    #[test]
    fn store_字号按当前的基础字号走() {
        let st = Store::default();
        st.load_settings(Some("{\"editor.fontSize\": 20}".into()), |_| true);
        st.update_ui(|u, base| Ok(u.step_font(base, Some(3)))).unwrap();
        assert_eq!(st.view().0.editor_font_size, 23);
        st.update_ui(|u, base| Ok(u.step_font(base, None))).unwrap();
        assert_eq!(st.view().0.editor_font_size, 20, "⌘0 回到 settings.json 里的 20");
    }

    #[test]
    fn shell_用不了_退回_shell_环境变量_报出来_标在那个键上() {
        let st = Store::default();
        let text = "{\n  \"terminal.shell\": \"/no/such/fish\"\n}";
        st.load_settings(Some(text.into()), |p| p == "/bin/zsh");
        let (e, problems) = st.view();
        assert_eq!(e.terminal_shell, "", "用不了：退回 $SHELL");
        assert_eq!(problems, vec![Problem::BadShell { path: "/no/such/fish".into(), pos: Pos { line: 2, col: 3 } }]);
        st.load_settings(Some("{\"terminal.shell\": \"/bin/zsh\"}".into()), |p| p == "/bin/zsh");
        assert_eq!(st.view(), (Effective { terminal_shell: "/bin/zsh".into(), ..effective(&Settings::default(), &UiState::default()) }, vec![]), "能用：照用，不报");
    }
}
