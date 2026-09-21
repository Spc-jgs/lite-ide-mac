//! 过滤框那一串字的语法（2026-09-21）。**和前端 `logview/query.ts` 是同一套**：
//!
//! ```text
//! a b            两个都得有（AND）
//! -c             不能有
//! "x y"          带空格的整串
//! /re/  -/re/    正则（里面的空格不拆）
//! ```
//!
//! 两边各写一份的原因：这边按**字节**比对（关键字要先按文件编码编过，见 `commands.rs`），
//! 那边按字符串画高亮，中间隔着 IPC 和编码，共用不了。所以这个文件的测试和
//! `tests/log-query.test.ts` 用同一组切词用例 —— 改语法两边一起改，用例对不上就是漂了。
//!
//! 这里只切词、分正负、分字面量 / 正则，**不碰编码也不编正则**：编码只有命令层知道，
//! 正则的大小写开关也在那边一起定。

/// 一条条件：字面量子串，或正则源码
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Lit(String),
    Re(String),
}

/// 切好的查询：`include` 全部命中、`exclude` 一个都不命中，这一行才算中
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub include: Vec<Term>,
    pub exclude: Vec<Term>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }
}

/// 切词：空白分隔；`"…"` 和 `/…/` 里的空白不算分隔。引号 / 斜杠没闭合就吃到末尾 ——
/// 边打边搜时大半时间它就是没闭合的。
fn tokenize(q: &str) -> Vec<&str> {
    let b = q.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let start = i;
        // 取反前缀先吃掉，剩下的部分才看是不是引号 / 正则
        if b[i] == b'-' && i + 1 < n && !b[i + 1].is_ascii_whitespace() {
            i += 1;
        }
        let open = b[i];
        if open == b'"' || open == b'/' {
            i = match q[i + 1..].find(open as char) {
                Some(off) => i + 1 + off + 1,
                None => n,
            };
        } else {
            while i < n && !b[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        out.push(&q[start..i]);
    }
    out
}

/// 切成条件。坏正则在这一层看不出来（不编），命令层编不过时报错。
pub fn parse(q: &str) -> Query {
    let mut out = Query::default();
    for raw in tokenize(q) {
        let mut tok = raw;
        // 单独一个 `-` 是字面量（日志里 `-` 分隔符多得是），带东西的才是取反
        let neg = tok.len() > 1 && tok.starts_with('-');
        if neg {
            tok = &tok[1..];
        }
        let term = if tok.len() >= 2 && tok.starts_with('/') && tok.ends_with('/') {
            let src = &tok[1..tok.len() - 1];
            if src.is_empty() {
                continue; // `//` 什么都不筛
            }
            Term::Re(src.to_string())
        } else {
            let lit = if tok.len() >= 2 && tok.starts_with('"') && tok.ends_with('"') {
                &tok[1..tok.len() - 1]
            } else if let Some(rest) = tok.strip_prefix('"') {
                rest // 没闭合的引号：正在打
            } else {
                tok
            };
            if lit.is_empty() {
                continue;
            }
            Term::Lit(lit.to_string())
        };
        if neg {
            out.exclude.push(term);
        } else {
            out.include.push(term);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(q: &Query) -> String {
        let f = |t: &Term| match t {
            Term::Lit(s) => s.clone(),
            Term::Re(s) => format!("/{s}/"),
        };
        q.include
            .iter()
            .map(f)
            .chain(q.exclude.iter().map(|t| format!("-{}", f(t))))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// 这几条和 `tests/log-query.test.ts` 的「切词」一节一一对应
    #[test]
    fn 切词和前端一致() {
        assert_eq!(show(&parse("a b")), "a b");
        assert_eq!(parse("a b").include.len(), 2, "空格是 AND");
        assert_eq!(show(&parse("  a   b  ")), "a b", "多余空白不算");
        assert_eq!(show(&parse("a -b")), "a -b");
        assert_eq!(parse("a -b").exclude.len(), 1, "-b 是排除");
        assert_eq!(show(&parse("-")), "-", "单独一个 - 是字面量");
        assert_eq!(parse("\"x y\" z").include[0], Term::Lit("x y".into()), "引号里的空格不拆");
        assert_eq!(parse("\"x y").include[0], Term::Lit("x y".into()), "没闭合的引号吃到末尾");
        assert_eq!(parse("/a.b/ c").include[0], Term::Re("a.b".into()), "斜杠是正则");
        assert_eq!(show(&parse("-/a|b/")), "-/a|b/", "-/re/ 是排除正则");
        assert_eq!(parse("\"/x/\"").include[0], Term::Lit("/x/".into()), "引号包着的斜杠是字面量");
        assert_eq!(parse("/a b/").include[0], Term::Re("a b".into()), "正则里的空格不拆");
        for e in ["", "   ", "\"\"", "//"] {
            assert!(parse(e).is_empty(), "空的几种写法都是空：{e:?}");
        }
        assert_eq!(parse("/a/").include[0], Term::Re("a".into()), "最短的正则");
        assert_eq!(parse("/").include[0], Term::Lit("/".into()), "单独一个斜杠是字面量");
        // 非 ASCII：切词按字节走，但只在 ASCII 空白 / 引号 / 斜杠上切，多字节字符不会被切开
        assert_eq!(show(&parse("订单 -健康检查")), "订单 -健康检查");
    }
}
