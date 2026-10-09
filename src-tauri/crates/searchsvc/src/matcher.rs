//! 「这段文字里哪几处算命中」—— **全应用只有这一个答案**（issue #42，设计见 docs/REPLACE.md 第 3 节）。
//!
//! ⇧⌘F 原来有两套语义：装了 rg 是正则 + smart-case（`a.b` 命中 `axb`、`foo` 命中 `Foo`），
//! 没装是字面量 + 区分大小写。对查找只是结果有出入，对替换就是「预览给你看的」和「真正改掉的」不是同一批。
//! 现在查找、替换预览、替换执行都问这里；rg 只用来加速 ⇧⌘F 找候选行，**每一行算不算命中还是这里说了算**。
//!
//! 三个开关显式给，没有 smart-case：它让 `foo` 和 `Foo` 的语义不同，而界面上看不出来。

use regex::{Regex, RegexBuilder};

/// 用户要找什么。三个开关和界面上的三个按钮一一对应，默认都关（IDEA 的默认）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub pattern: String,
    /// 区分大小写
    pub case: bool,
    /// 整词：命中的前一个、后一个字符都不是词字符
    pub word: bool,
    /// 正则；关着时整串按字面量找
    pub regex: bool,
}

impl Query {
    /// 只有词、开关全关（测试和老调用点用）
    pub fn literal(pattern: &str) -> Self {
        Query { pattern: pattern.to_string(), ..Default::default() }
    }
}

pub struct Matcher {
    re: Regex,
    word: bool,
    regex: bool,
    /// 「词字符」的定义直接借 `regex` 的 `\w`（Unicode：字母、数字、连接号、组合记号）——
    /// 自己写 `is_alphanumeric() || '_'` 会和正则里的 `\w` 在组合记号上对不上
    wordch: Regex,
}

impl Matcher {
    /// 词为空、或者正则写错了，返回给人看的一句话
    pub fn new(q: &Query) -> Result<Self, String> {
        if q.pattern.is_empty() {
            return Err("搜索词是空的".into());
        }
        let src = if q.regex { q.pattern.clone() } else { regex::escape(&q.pattern) };
        let re = RegexBuilder::new(&src)
            .case_insensitive(!q.case)
            // `^` `$` 按行首行尾算 —— 和 rg（按行搜）一致，也是人在编辑器里的直觉
            .multi_line(true)
            .build()
            .map_err(|e| format!("正则写错了：{}", first_line(&e.to_string())))?;
        Ok(Matcher { re, word: q.word, regex: q.regex, wordch: Regex::new(r"^\w").expect("固定的正则") })
    }

    /// 全部命中的字节区间，按出现顺序、互不重叠。**空命中不算**：`a*` 这种正则在每个位置都能配上一个空串，
    /// 拿去替换就是在每个字符之间插东西
    pub fn find_all(&self, text: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut at = 0;
        while at <= text.len() {
            let Some(m) = self.re.find_at(text, at) else { break };
            if m.start() == m.end() || (self.word && !self.bounded(text, m.start(), m.end())) {
                // 不合格就从下一个字符接着找。不能跳到 m.end()：整词模式下，同一段里靠后的位置可能合格
                // （`foo_foo foo` 找整词 `foo`，第一处不算，要接着往后看）
                match next_char(text, m.start()) {
                    Some(n) => at = n,
                    None => break,
                }
                continue;
            }
            out.push((m.start(), m.end()));
            at = m.end();
        }
        out
    }

    pub fn is_match(&self, text: &str) -> bool {
        if !self.word {
            // 不是整词时只要配上一个非空的就行；空命中的正则（`a*`）也会被 find_all 过滤，这里同样过一遍
            return self.re.find_iter(text).any(|m| m.start() != m.end());
        }
        !self.find_all(text).is_empty()
    }

    /// `text` 里 `[start, end)` 这一处替换成什么（#42）。字面量模式下替换串原样用 —— 写 `$1` 就是 `$1`；
    /// 正则模式下 `$1` / `${name}` / `$$` 照 `regex` 的语法展开（分组不存在展开成空串，和 rg `-r` 一样）
    pub fn replacement(&self, text: &str, at: (usize, usize), repl: &str) -> String {
        if !self.regex {
            return repl.to_string();
        }
        // 从这一处的起点重新配一次拿分组。整词模式下 find_all 可能是跳过了前面不合格的候选才找到它的，
        // 但从它的起点开始配，最左最先的那个就是它
        match self.re.captures_at(text, at.0) {
            Some(c) if c.get(0).is_some_and(|m| (m.start(), m.end()) == at) => {
                let mut out = String::new();
                c.expand(repl, &mut out);
                out
            }
            _ => repl.to_string(),
        }
    }

    /*
     * **整词按 rg `-w` 的意思算，不是 `\b…\b`**（docs/REPLACE.md 第 8 节实测）。
     *
     * `\b` 说的是「这里一边是词字符一边不是」。词以非词字符开头时（`-x`、`.foo`）它就反过来了：
     * `\b-x\b` 在 `a -x b` 里配不上（空格和 `-` 都不是词字符，中间没有边界），在 `foo-x` 里反而配上了。
     * rg 的 `-w` 是「命中的前后都不是词字符」，那才是人说「整词」的意思。`regex` 没有前后断言，所以找到候选再查两边。
     */
    fn bounded(&self, text: &str, start: usize, end: usize) -> bool {
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        !before.is_some_and(|c| self.is_word_char(c)) && !after.is_some_and(|c| self.is_word_char(c))
    }

    fn is_word_char(&self, c: char) -> bool {
        let mut buf = [0u8; 4];
        self.wordch.is_match(c.encode_utf8(&mut buf))
    }
}

fn next_char(text: &str, i: usize) -> Option<usize> {
    text[i..].chars().next().map(|c| i + c.len_utf8())
}

/// `regex` 的报错是好几行带 ASCII 箭头的，界面上一行放不下 —— 取最后一句（「error: unclosed group」那句）
fn first_line(e: &str) -> String {
    e.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(e).trim().to_string()
}

/// 字节区间 → 这一行里的 UTF-16 区间（前端的字符串下标是 UTF-16）。超出 `limit` 个字符的截掉
pub fn utf16_spans(text: &str, spans: &[(usize, usize)], limit_chars: usize) -> Vec<[u32; 2]> {
    let cut = text.char_indices().nth(limit_chars).map_or(text.len(), |(i, _)| i);
    spans
        .iter()
        .filter(|(s, _)| *s < cut)
        .map(|&(s, e)| {
            let e = e.min(cut);
            let a = text[..s].encode_utf16().count() as u32;
            [a, a + text[s..e].encode_utf16().count() as u32]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(p: &str, case: bool, word: bool, regex: bool) -> Matcher {
        Matcher::new(&Query { pattern: p.into(), case, word, regex }).unwrap()
    }
    fn hits(m: &Matcher, t: &str) -> Vec<&'static str> {
        // 为了断言好读：把命中的那几段原文拿出来（泄漏到 'static 只在测试里）
        m.find_all(t).into_iter().map(|(s, e)| &*Box::leak(t[s..e].to_string().into_boxed_str())).collect()
    }

    #[test]
    fn 字面量模式下正则字符就是字符() {
        let m = q("a.b", false, false, false);
        assert!(m.is_match("x a.b y"));
        assert!(!m.is_match("axb"), "原来装了 rg 时 a.b 会命中 axb");
        assert!(q("foo(bar)", false, false, false).is_match("foo(bar)"));
        assert!(q("$x", false, false, false).is_match("price $x"));
    }

    #[test]
    fn 正则模式() {
        assert!(q("a.b", false, false, true).is_match("axb"));
        assert_eq!(hits(&q(r"\d+", false, false, true), "a1 b22 c333"), vec!["1", "22", "333"]);
    }

    #[test]
    fn 大小写是显式的开关_没有smart_case() {
        let off = q("Foo", false, false, false);
        assert_eq!(hits(&off, "foo Foo FOO"), vec!["foo", "Foo", "FOO"], "关着：大写的词也不分大小写（smart-case 会让它区分）");
        let on = q("foo", true, false, false);
        assert_eq!(hits(&on, "foo Foo FOO"), vec!["foo"]);
        // Unicode 大小写：和 rg -i 一致（第 0 步实测）
        assert!(q("straße", false, false, false).is_match("STRAßE"));
        assert!(!q("straße", false, false, false).is_match("STRASSE"), "ß 不折成 SS，rg 也不");
    }

    #[test]
    fn 整词按rg的意思_不是反斜杠b() {
        let w = q("-x", false, true, false);
        assert!(w.is_match("a -x b"), "前后是空格：算整词");
        assert!(!w.is_match("foo-x"), "前面是 o：不算（\\b-x\\b 正好反过来）");
        let dot = q(".foo", false, true, false);
        assert!(dot.is_match(".foo bar"));
        assert!(!dot.is_match("x.foo"));
        let order = q("order", false, true, false);
        assert!(order.is_match("order id"));
        assert!(!order.is_match("reorder"));
        assert!(!order.is_match("order_id"), "_ 是词字符");
        // 同一行里第一处不合格、后面的合格：要接着往后找，不能整行判死
        assert_eq!(hits(&q("foo", false, true, false), "foo_foo foo"), vec!["foo"]);
    }

    #[test]
    fn 中文字是词字符_整词在一串中文里配不到() {
        let w = q("订单", false, true, false);
        assert!(!w.is_match("查询订单状态"));
        assert!(w.is_match("订单"));
        assert!(w.is_match(" 订单 "));
    }

    #[test]
    fn 空命中不算() {
        let m = q("a*", false, false, true);
        assert_eq!(hits(&m, "xaay"), vec!["aa"], "每个位置的空串都不算命中");
        assert!(!m.is_match("xyz"));
    }

    #[test]
    fn 行首行尾按行算() {
        let m = q("^b", false, false, true);
        assert!(m.is_match("a\nb"), "多行文本里第二行的行首");
    }

    #[test]
    fn 正则写错了给一句人话() {
        let e = Matcher::new(&Query { pattern: "(ab".into(), regex: true, ..Default::default() }).err().unwrap();
        assert!(e.starts_with("正则写错了"), "{e}");
        assert!(!e.contains('\n'), "一行放得下：{e}");
        assert!(Matcher::new(&Query::literal("")).is_err());
    }

    #[test]
    fn 替换串_正则才展开分组() {
        let t = "getName getAge";
        let re = q(r"get(\w+)", false, false, true);
        let at = re.find_all(t);
        assert_eq!(re.replacement(t, at[0], "fetch$1"), "fetchName");
        assert_eq!(re.replacement(t, at[1], "${1}Of"), "AgeOf");
        assert_eq!(re.replacement(t, at[0], "$$1"), "$1", "$$ 是字面的 $");
        let lit = q("getName", false, false, false);
        assert_eq!(lit.replacement(t, lit.find_all(t)[0], "x$1"), "x$1", "字面量模式下 $1 原样");
    }

    #[test]
    fn utf16区间() {
        // 「订单」在 UTF-8 里是 6 个字节，在 UTF-16 里是 2 个单位；😀 是 4 字节、2 个单位
        let t = "😀 订单 x";
        let m = q("订单", false, false, false);
        assert_eq!(utf16_spans(t, &m.find_all(t), 400), vec![[3, 5]]);
        assert_eq!(utf16_spans(t, &m.find_all(t), 2), Vec::<[u32; 2]>::new(), "截掉的那一截里的命中不带");
    }
}
