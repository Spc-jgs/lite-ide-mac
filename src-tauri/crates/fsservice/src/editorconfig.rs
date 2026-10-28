//! `.editorconfig`（https://spec.editorconfig.org）：项目约定的缩进、存盘时去行尾空格、补末尾换行。
//! IDEA 内置认它、VS Code 装个扩展认它；很多仓库根上都有一份，不认的话在这些项目里一存盘格式就和同事的对不上。
//!
//! 只做规范里最常用的那几个键（[`Props`]）。`end_of_line` / `charset` 先不接：对一个已有的文件照它们存，
//! 等于整份换行符 / 编码转换 —— 那是一次「改了整个文件」的提交，不该悄悄发生在 ⌘S 里。
//!
//! 找法照规范：从文件所在目录往上，每一层的 `.editorconfig` 都读，遇到写了 `root = true` 的那份就停；
//! **近的盖远的、同一份里靠后的段盖靠前的**。只读、每份最多 64 KB（人写的配置，超了多半不是我们要的那种文件）。

use std::path::Path;

/// 一个文件生效的设置。没写的是 None（交给编辑器自己猜 / 不处理）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Props {
    /// `space` / `tab`
    pub indent_style: Option<String>,
    /// 缩进几格。写了 `indent_size = tab` 的取 `tab_width`
    pub indent_size: Option<u32>,
    pub tab_width: Option<u32>,
    pub trim_trailing_whitespace: Option<bool>,
    pub insert_final_newline: Option<bool>,
}

const MAX_BYTES: u64 = 64 << 10;

/// `file` 生效的设置。`file` 要是绝对路径
pub fn resolve(file: &Path) -> Props {
    let mut configs = Vec::new();
    let mut dir = file.parent();
    while let Some(d) = dir {
        let p = d.join(".editorconfig");
        if let Some(text) = read_small(&p) {
            let (root, sections) = parse(&text);
            configs.push((d.to_path_buf(), sections));
            if root {
                break;
            }
        }
        dir = d.parent();
    }
    let mut out = Props::default();
    // 远的先、近的后：近的盖远的
    for (base, sections) in configs.iter().rev() {
        let Ok(rel) = file.strip_prefix(base) else { continue };
        let rel = rel.to_string_lossy();
        for (glob, kv) in sections {
            if matches(glob, &rel) {
                for (k, v) in kv {
                    apply(&mut out, k, v);
                }
            }
        }
    }
    if out.indent_size.is_none() && out.indent_style.as_deref() == Some("tab") {
        out.indent_size = out.tab_width;
    }
    out
}

fn read_small(p: &Path) -> Option<String> {
    use std::io::Read;
    let f = std::fs::File::open(p).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut s = String::new();
    f.take(MAX_BYTES).read_to_string(&mut s).ok()?;
    Some(s)
}

type Section = (String, Vec<(String, String)>);

/// INI：`[glob]` 起一段，`键 = 值`，`#` / `;` 开头是注释。段之前的 `root = true` 是这份文件的。键和值不分大小写（值里的 glob 除外）
fn parse(text: &str) -> (bool, Vec<Section>) {
    let mut root = false;
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some(g) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            sections.push((g.to_string(), Vec::new()));
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase());
        match sections.last_mut() {
            Some((_, kv)) => kv.push((k, v)),
            None if k == "root" => root = v == "true",
            None => {}
        }
    }
    (root, sections)
}

fn apply(p: &mut Props, k: &str, v: &str) {
    // `unset` = 把上面定的撤掉（规范）
    let unset = v == "unset";
    let num = || v.parse::<u32>().ok().filter(|n| (1..=16).contains(n));
    let flag = || match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    };
    match k {
        "indent_style" if unset => p.indent_style = None,
        "indent_style" if v == "space" || v == "tab" => p.indent_style = Some(v.to_string()),
        "indent_size" if unset => p.indent_size = None,
        "indent_size" if v == "tab" => p.indent_size = p.tab_width,
        "indent_size" => p.indent_size = num().or(p.indent_size),
        "tab_width" if unset => p.tab_width = None,
        "tab_width" => p.tab_width = num().or(p.tab_width),
        "trim_trailing_whitespace" if unset => p.trim_trailing_whitespace = None,
        "trim_trailing_whitespace" => p.trim_trailing_whitespace = flag().or(p.trim_trailing_whitespace),
        "insert_final_newline" if unset => p.insert_final_newline = None,
        "insert_final_newline" => p.insert_final_newline = flag().or(p.insert_final_newline),
        _ => {}
    }
}

/// 段头的 glob 对不对得上（`rel` 是相对那份 `.editorconfig` 所在目录的路径，`/` 分隔）。
/// 规范：不带 `/` 的 glob 对任意一层的文件名（等于前面补 `**/`）；带 `/` 的从那个目录算起（开头的 `/` 去掉）
pub fn matches(glob: &str, rel: &str) -> bool {
    let g = if glob.contains('/') { glob.trim_start_matches('/').to_string() } else { format!("**/{glob}") };
    let Some(alts) = expand_braces(&g) else { return false };
    alts.iter().any(|a| glob_match(a.as_bytes(), rel.as_bytes()))
}

/// `{a,b}` 展开成几个；`{1..3}` 展开成 1、2、3（上限 256 个，防一个 `{1..999999}` 把内存撑大）。嵌套的照样展开。不成对的花括号当字面量
fn expand_braces(g: &str) -> Option<Vec<String>> {
    let b = g.as_bytes();
    // 找第一个没被 `\` 转义的 `{`（只有一项的 `{x}` 下面会改写成转义的，不跳过就又找回它自己、无限递归）
    let unescaped = |from: usize, want: u8| {
        let mut i = from;
        while i < b.len() {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == want {
                return Some(i);
            }
            i += 1;
        }
        None
    };
    let Some(open) = unescaped(0, b'{') else { return Some(vec![g.to_string()]) };
    let mut depth = 0;
    let mut close = None;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    // 不成对：这个 `{` 当字面量，后面的照常展开
    let Some(close) = close else { return Some(expand_braces(&format!("{}\\{{{}", &g[..open], &g[open + 1..]))?) };
    let (pre, body, post) = (&g[..open], &g[open + 1..close], &g[close + 1..]);
    let mut items: Vec<String> = Vec::new();
    if let Some((a, z)) = body.split_once("..").filter(|(a, z)| a.parse::<i64>().is_ok() && z.parse::<i64>().is_ok()) {
        let (a, z): (i64, i64) = (a.parse().ok()?, z.parse().ok()?);
        if (z - a).abs() > 256 {
            return None;
        }
        let range: Vec<i64> = if a <= z { (a..=z).collect() } else { (z..=a).rev().collect() };
        items.extend(range.iter().map(|n| n.to_string()));
    } else if !body.contains(',') {
        // `{x}` 只有一项：规范说当字面量
        return Some(expand_braces(&format!("{pre}\\{{{body}\\}}{post}"))?.into_iter().collect());
    } else {
        let mut depth = 0;
        let mut cur = String::new();
        for c in body.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    items.push(std::mem::take(&mut cur));
                    continue;
                }
                _ => {}
            }
            cur.push(c);
        }
        items.push(cur);
    }
    let mut out = Vec::new();
    for it in items {
        for x in expand_braces(&format!("{pre}{it}{post}"))? {
            out.push(x);
            if out.len() > 256 {
                return None;
            }
        }
    }
    Some(out)
}

/// `*` 不跨 `/`、`**` 跨、`?` 一个字符（不是 `/`）、`[abc]` / `[!abc]` / `[a-z]`、`\` 转义。回溯式匹配，glob 和路径都短
fn glob_match(g: &[u8], s: &[u8]) -> bool {
    match g.first() {
        None => s.is_empty(),
        Some(b'*') if g.get(1) == Some(&b'*') => {
            let rest = &g[2..];
            // `**/` 也能匹配零层目录
            if let Some(r) = rest.strip_prefix(b"/") {
                if glob_match(r, s) {
                    return true;
                }
            }
            (0..=s.len()).any(|i| glob_match(rest, &s[i..]))
        }
        Some(b'*') => {
            let rest = &g[1..];
            for i in 0..=s.len() {
                if glob_match(rest, &s[i..]) {
                    return true;
                }
                if i < s.len() && s[i] == b'/' {
                    break;
                }
            }
            false
        }
        Some(b'?') => s.first().is_some_and(|&c| c != b'/') && glob_match(&g[1..], &s[1..]),
        Some(b'[') => {
            let Some(end) = g.iter().skip(1).position(|&c| c == b']').map(|i| i + 1) else {
                return s.first() == Some(&b'[') && glob_match(&g[1..], &s[1..]);
            };
            let Some(&c) = s.first() else { return false };
            let (neg, set) = match g.get(1) {
                Some(b'!') => (true, &g[2..end]),
                _ => (false, &g[1..end]),
            };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == b'-' {
                    hit |= set[i] <= c && c <= set[i + 2];
                    i += 3;
                } else {
                    hit |= set[i] == c;
                    i += 1;
                }
            }
            hit != neg && c != b'/' && glob_match(&g[end + 1..], &s[1..])
        }
        Some(b'\\') if g.len() > 1 => s.first() == Some(&g[1]) && glob_match(&g[2..], &s[1..]),
        Some(&c) => s.first() == Some(&c) && glob_match(&g[1..], &s[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_照规范() {
        // 不带 / 的对任意一层的文件名
        assert!(matches("*", "a.java") && matches("*", "src/main/A.java"));
        assert!(matches("*.java", "src/main/A.java") && !matches("*.java", "A.javax"));
        assert!(matches("*.{js,ts}", "web/app.ts") && matches("*.{js,ts}", "app.js") && !matches("*.{js,ts}", "app.tsx"));
        assert!(matches("{package.json,*.yml}", "package.json") && matches("{package.json,*.yml}", ".github/ci.yml"));
        assert!(matches("Makefile", "sub/Makefile") && !matches("Makefile", "Makefile.am"));
        // 带 / 的从 .editorconfig 所在目录算
        assert!(matches("/src/*.rs", "src/a.rs") && !matches("/src/*.rs", "x/src/a.rs") && !matches("/src/*.rs", "src/sub/a.rs"));
        assert!(matches("src/**.rs", "src/sub/deep/a.rs"));
        assert!(matches("lib/**/x.c", "lib/x.c") && matches("lib/**/x.c", "lib/a/b/x.c"), "**/ 也能是零层");
        assert!(matches("file{1..3}.txt", "file2.txt") && !matches("file{1..3}.txt", "file4.txt"));
        assert!(matches("[abc].md", "b.md") && !matches("[!abc].md", "b.md") && matches("[a-c]x", "cx"));
        assert!(matches("?.c", "a.c") && !matches("?.c", "ab.c"));
        assert!(matches("{single}.txt", "{single}.txt"), "只有一项的花括号当字面量");
        assert!(!matches("{1..99999}", "5"), "范围太大不展开、不匹配，也不把内存撑大");
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ec-{tag}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 近的盖远的_靠后的段盖靠前的_root_停住_unset_撤掉() {
        let outer = tmp("root");
        let proj = outer.join("proj");
        std::fs::create_dir_all(proj.join("web/src")).unwrap();
        // 外面那份不该被读到：proj 里那份写了 root = true
        std::fs::write(outer.join(".editorconfig"), "[*]\nindent_size = 8\ninsert_final_newline = false\ntab_width = 3\n").unwrap();
        std::fs::write(
            proj.join(".editorconfig"),
            "# 项目约定\nroot = true\n\n[*]\nindent_style = space\nindent_size = 4\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n\n[*.md]\ntrim_trailing_whitespace = false\n\n[Makefile]\nindent_style = tab\ntab_width = 8\n",
        )
        .unwrap();
        std::fs::write(proj.join("web/.editorconfig"), "[*.{ts,js}]\nindent_size = 2\n[*.min.js]\ninsert_final_newline = unset\n").unwrap();

        let java = resolve(&proj.join("src/A.java"));
        // 外面那份写了、项目里那份没写的键：停住了就读不到（只看两边都写了的键，近的本来就盖得掉，测不出停没停 —— 第一版就是这么空过的）
        assert_eq!(java.tab_width, None, "root = true：外面那份一个键都不读");
        assert_eq!((java.indent_style.as_deref(), java.indent_size, java.trim_trailing_whitespace, java.insert_final_newline), (Some("space"), Some(4), Some(true), Some(true)), "root = true 那份往上不再读：外面的 8 / false 不算");
        assert_eq!(resolve(&proj.join("README.md")).trim_trailing_whitespace, Some(false), "靠后的段盖靠前的");
        let ts = resolve(&proj.join("web/src/app.ts"));
        assert_eq!((ts.indent_size, ts.trim_trailing_whitespace), (Some(2), Some(true)), "近的盖远的，没写的继承");
        assert_eq!(resolve(&proj.join("web/x.min.js")).insert_final_newline, None, "unset 把上面定的撤掉");
        let mk = resolve(&proj.join("Makefile"));
        assert_eq!((mk.indent_style.as_deref(), mk.indent_size, mk.tab_width), (Some("tab"), Some(4), Some(8)), "indent_size 从 [*] 继承 4（规范如此）；用 Tab 缩进时显示宽度看 tab_width");
        let only = tmp("tabonly");
        std::fs::write(only.join(".editorconfig"), "root = true\n[*]\nindent_style = tab\ntab_width = 3\n").unwrap();
        assert_eq!(resolve(&only.join("x.go")).indent_size, Some(3), "Tab 缩进、哪里都没写 indent_size：取 tab_width");
        let _ = std::fs::remove_dir_all(&only);
        assert_eq!(resolve(&outer.join("other/x.txt")).indent_size, Some(8), "proj 外面的文件读外面那份");
        assert_eq!(resolve(&tmp("none").join("x.txt")), Props::default(), "一份都没有：全是 None");
        let _ = std::fs::remove_dir_all(&outer);
    }
}
