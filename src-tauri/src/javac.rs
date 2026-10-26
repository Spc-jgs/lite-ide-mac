//! 构建（⌘F9，docs/IDEA.md 第 22 节）的编译错误：从 Maven 的输出里认出 javac 报的每一条，出错误树（按文件分组、点了跳过去）。
//!
//! **认结构，不认字**：javac 的话跟着系统语言走 —— 这台机器上是「找不到符号 / 符号: 变量 nosuch / 位置: 类 …」（2026-10-20 真 mvn 撞出来的），
//! 认 `cannot find symbol` 的话一条都认不出。maven-compiler-plugin 的格式是固定的：
//!
//! ```text
//! [ERROR] /abs/Foo.java:[13,131] 找不到符号        ← 一条的开头：级别、绝对路径、[行,列]、消息
//!   符号:   变量 nosuch                            ← 缩进两格的是这一条的补充
//!   位置: 类 com.demo.Foo
//! …
//! [ERROR] Failed to execute goal …: Compilation failure
//! [ERROR] /abs/Foo.java:[13,131] 找不到符号        ← **同一条又出现一遍**，补充行也带 `[ERROR] ` 前缀 —— 按 (路径, 行, 列, 消息) 去重
//! [ERROR]   符号:   变量 nosuch
//! ```
//!
//! 「使用或覆盖了已过时的 API」那种是 `[INFO]` 里的注，没有行号，不算一条（IDEA 也只在开了 -Xlint 时才列警告）。

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// 最多列多少条（几千条错误时多半是一个依赖没下下来，列再多也没人看；总数照实报）
pub const PROBLEMS_MAX: usize = 1_000;
/// 一行最多读多长（没有换行的超长输出不让它攒满内存）
const LINE_MAX: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub path: String,
    pub line: u32,
    pub col: u32,
    /// true = 错误，false = 警告
    pub error: bool,
    /// 第一行消息 + 补充行（`符号: …`、`位置: …`），用换行连着
    pub message: String,
}

/// `[ERROR] /abs/Foo.java:[13,131] 消息` → (是不是错误, 路径, 行, 列, 消息)
fn head(line: &str) -> Option<(bool, &str, u32, u32, &str)> {
    let (error, rest) = if let Some(r) = line.strip_prefix("[ERROR] ") {
        (true, r)
    } else if let Some(r) = line.strip_prefix("[WARNING] ") {
        (false, r)
    } else {
        return None;
    };
    // 路径里可能有 `:`、空格，所以从 `.java:[` 往回切，不按冒号拆
    let at = rest.find(".java:[")? + ".java".len();
    let (path, tail) = rest.split_at(at);
    if !path.starts_with('/') {
        return None;
    }
    let tail = tail.strip_prefix(":[")?;
    let (pos, msg) = tail.split_once("] ").or_else(|| tail.strip_suffix(']').map(|p| (p, "")))?;
    let (l, c) = pos.split_once(',')?;
    Some((error, path, l.trim().parse().ok()?, c.trim().parse().ok()?, msg.trim()))
}

/// 一条后面的补充行：缩进两格（第一遍）或者 `[ERROR]   `（Failed to execute goal 后面那一遍）
fn more(line: &str, error: bool) -> Option<&str> {
    let pre = if error { "[ERROR]   " } else { "[WARNING]   " };
    line.strip_prefix(pre).or_else(|| line.strip_prefix("  ")).map(str::trim).filter(|s| !s.is_empty())
}

/// 级别那个词跟着系统语言走（javac 的本地化）：认得的几种语言列在这儿，别的当错误 —— 把警告当成错误列出来，比漏掉一条错误好
const WARN_WORDS: [&str; 3] = ["警告", "warning", "경고"];

/// Gradle（和直接跑 javac）的格式：`/abs/Foo.java:4: 错误: 找不到符号` → (缩进, 是不是错误, 路径, 行, 消息)。
/// 没有列：列从下面那行 `^` 的位置算（[`parse`]）。同一段在失败摘要里又出现一遍、每行多缩进两格
fn head_javac(line: &str) -> Option<(usize, bool, &str, u32, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    if !rest.starts_with('/') {
        return None;
    }
    let at = rest.find(".java:")? + ".java".len();
    let (path, tail) = rest.split_at(at);
    let (num, tail) = tail.strip_prefix(':')?.split_once(": ")?;
    let line_no: u32 = num.parse().ok()?;
    let (kind, msg) = tail.split_once(": ").unwrap_or((tail, ""));
    let error = !WARN_WORDS.iter().any(|w| kind.trim().eq_ignore_ascii_case(w));
    Some((indent, error, path, line_no, msg.trim()))
}

/// 正在认的那一条后面还要吃几行
enum Cur {
    /// Maven 的：补充行。下标是 None 时是重复的那一遍（吃掉不加）
    Maven(Option<usize>, bool),
    /// javac 的：先是那一行源码、再是 `^` 那行（算出列才知道是不是重复的），之后是比它多缩进两格的补充行
    Javac { indent: usize, error: bool, path: String, line: u32, msg: String, stage: u8, idx: Option<usize> },
}

/// 从一段输出里认出全部（Maven 的 `[ERROR] …:[行,列]`，和 Gradle / javac 的 `…:行: 错误: …`）。回 (列出来的, 一共几条)
pub fn parse(r: impl Read) -> (Vec<Problem>, usize) {
    let mut out: Vec<Problem> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut total = 0usize;
    let mut cur: Option<Cur> = None;
    let mut r = BufReader::new(r);
    let mut buf = Vec::new();
    // 认一条：去重（路径, 行, 列, 消息）、记总数、没超上限就列出来。回它的下标（重复的、超了的是 None）
    let mut add = |out: &mut Vec<Problem>, p: Problem| -> Option<usize> {
        if !seen.insert((p.path.clone(), p.line, p.col, p.message.clone())) {
            return None;
        }
        total += 1;
        if out.len() >= PROBLEMS_MAX {
            return None;
        }
        out.push(p);
        Some(out.len() - 1)
    };
    loop {
        buf.clear();
        match r.by_ref().take(LINE_MAX as u64).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let s = String::from_utf8_lossy(&buf);
        let line = s.trim_end_matches(['\n', '\r']);
        // javac 那条还没定列（在等源码行 / `^` 行）
        if let Some(Cur::Javac { indent, error, path, line: l, msg, stage, idx }) = &mut cur {
            if *stage == 0 {
                *stage = 1;
                continue;
            }
            if *stage == 1 {
                let caret = line.find('^').filter(|&i| line[..i].chars().all(|c| c == ' ') && line[i + 1..].trim().is_empty());
                let col = caret.map_or(1, |i| (i.saturating_sub(*indent) + 1) as u32);
                *idx = add(&mut out, Problem { path: path.clone(), line: *l, col, error: *error, message: msg.clone() });
                *stage = 2;
                if caret.is_some() {
                    continue;
                }
            } else {
                // 补充行（`符号: …`、`位置: …`）：正好比开头那行多缩进两格、后面直接是字。「1 个错误」那行缩进和开头一样，到它就完
                let pre = " ".repeat(*indent + 2);
                if let Some(m) = line.strip_prefix(pre.as_str()).filter(|m| !m.starts_with(' ') && !m.trim().is_empty()) {
                    if let Some(i) = idx {
                        out[*i].message.push('\n');
                        out[*i].message.push_str(m.trim());
                    }
                    continue;
                }
            }
            cur = None;
        }
        if let Some((error, path, l, c, msg)) = head(line) {
            let idx = add(&mut out, Problem { path: path.to_string(), line: l, col: c, error, message: msg.to_string() });
            cur = Some(Cur::Maven(idx, error));
            continue;
        }
        if let Some((indent, error, path, l, msg)) = head_javac(line) {
            cur = Some(Cur::Javac { indent, error, path: path.to_string(), line: l, msg: msg.to_string(), stage: 0, idx: None });
            continue;
        }
        if let Some(Cur::Maven(i, error)) = cur {
            match more(line, error) {
                Some(m) => {
                    if let Some(i) = i {
                        out[i].message.push('\n');
                        out[i].message.push_str(m);
                    }
                }
                None => cur = None,
            }
        }
    }
    // 输出在 javac 那条的源码行 / `^` 行之前就断了：照样算一条（列给 1）
    if let Some(Cur::Javac { error, path, line, msg, stage, .. }) = cur {
        if stage < 2 {
            add(&mut out, Problem { path, line, col: 1, error, message: msg });
        }
    }
    (out, total)
}

/// 读一次运行的输出文件
pub fn parse_file(log: &Path) -> std::io::Result<(Vec<Problem>, usize)> {
    Ok(parse(std::fs::File::open(log)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-20 真 mvn（maven-compiler-plugin 3.14.1，中文系统）的输出，原样
    const REAL: &str = "[INFO] --- maven-compiler-plugin:3.14.1:compile (default-compile) @ order-service ---
[INFO] Compiling 1 source file with javac [debug parameters release 17] to target/classes
[INFO] /p/shop-common/src/main/java/com/demo/common/Old.java: /p/shop-common/src/main/java/com/demo/common/Old.java使用或覆盖了已过时的 API。
[INFO] -------------------------------------------------------------
[ERROR] COMPILATION ERROR :
[INFO] -------------------------------------------------------------
[ERROR] /p/order-service/src/main/java/com/demo/order/OrderApplication.java:[13,131] 找不到符号
  符号:   变量 nosuch
  位置: 类 com.demo.order.OrderApplication
[INFO] 1 error
[INFO] -------------------------------------------------------------
[INFO] BUILD FAILURE
[ERROR] Failed to execute goal org.apache.maven.plugins:maven-compiler-plugin:3.14.1:compile (default-compile) on project order-service: Compilation failure
[ERROR] /p/order-service/src/main/java/com/demo/order/OrderApplication.java:[13,131] 找不到符号
[ERROR]   符号:   变量 nosuch
[ERROR]   位置: 类 com.demo.order.OrderApplication
[ERROR] -> [Help 1]
";

    #[test]
    fn 真输出_中文的照认_出现两遍的只算一条_补充行接在后面() {
        let (p, total) = parse(REAL.as_bytes());
        assert_eq!(total, 1, "{p:?}");
        assert_eq!(p[0].path, "/p/order-service/src/main/java/com/demo/order/OrderApplication.java");
        assert_eq!((p[0].line, p[0].col, p[0].error), (13, 131, true));
        assert_eq!(p[0].message, "找不到符号\n符号:   变量 nosuch\n位置: 类 com.demo.order.OrderApplication", "补充行只接一遍（第二遍的不重复加）");
    }

    /// 2026-10-26 真 Gradle 9.8.1（中文系统）的输出，原样：一条在任务输出里、又在失败摘要里多缩进两格出现一遍
    const GRADLE: &str = "
> Task :shop-common:compileJava FAILED
/p/shop-common/src/main/java/com/demo/common/Money.java:4: 错误: 找不到符号
    public static long cents(long yuan) { return yuan * nosuch; }
                                                        ^
  符号:   变量 nosuch
  位置: 类 Money
/p/shop-common/src/main/java/com/demo/common/Money.java:6: 警告: [deprecation] Money中的old()已过时
    void use() { old(); }
                 ^
1 个错误
1 个警告

FAILURE: Build failed with an exception.

* What went wrong:
Execution failed for task ':shop-common:compileJava' (registered by plugin class 'org.gradle.api.plugins.JavaBasePlugin').
> Compilation failed; see the compiler output below.
  /p/shop-common/src/main/java/com/demo/common/Money.java:4: 错误: 找不到符号
      public static long cents(long yuan) { return yuan * nosuch; }
                                                          ^
    符号:   变量 nosuch
    位置: 类 Money
  1 个错误
";

    #[test]
    fn gradle_的格式_列从尖号算_两遍只算一条_警告分得出() {
        let (p, total) = parse(GRADLE.as_bytes());
        assert_eq!(total, 2, "{p:?}");
        assert_eq!((p[0].path.as_str(), p[0].line, p[0].col, p[0].error), ("/p/shop-common/src/main/java/com/demo/common/Money.java", 4, 57, true));
        assert_eq!(p[0].message, "找不到符号\n符号:   变量 nosuch\n位置: 类 Money", "补充行只接一遍；源码行和尖号行不算补充");
        assert_eq!((p[1].line, p[1].col, p[1].error), (6, 18, false));
        assert_eq!(p[1].message, "[deprecation] Money中的old()已过时");
    }

    #[test]
    fn 警告也认_路径里有空格和冒号的照切_没行号的注不算_超过上限照实报总数() {
        let s = "[WARNING] /p/my app/a:b/Foo.java:[3,8] [deprecation] Date 中的 getYear() 已过时\n[INFO] /p/Old.java: 有关详细信息, 请使用 -Xlint:deprecation 重新编译。\n[ERROR] Foo.java:[1,1] 相对路径的不认\n";
        let (p, total) = parse(s.as_bytes());
        assert_eq!(total, 1, "{p:?}");
        assert_eq!((p[0].path.as_str(), p[0].line, p[0].error), ("/p/my app/a:b/Foo.java", 3, false));
        let many: String = (1..=PROBLEMS_MAX + 5).map(|i| format!("[ERROR] /p/A.java:[{i},1] x\n")).collect();
        let (p, total) = parse(many.as_bytes());
        assert_eq!((p.len(), total), (PROBLEMS_MAX, PROBLEMS_MAX + 5));
    }
}
