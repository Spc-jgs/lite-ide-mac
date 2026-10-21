//! 不靠语言服务器认 Java 源码里几样东西用的轻量分词（MyBatis 跳转、Spring 接口和配置，docs/IDEA.md 第 24、25 节）。
//!
//! 不是 Java 的语法分析：只分出标识符、单个符号和注解，记着每个记号在第几行、第几层花括号、第几层圆括号里 ——
//! 认「接口体第一层的方法声明」「类上和方法上的注解」够了。正则做不到这件事：它不知道自己在第几层括号里，
//! `default` 方法体里的匿名类、`@Param("id")` 里的括号都会被认错（MyBatis 那条测试各有一例）。
//!
//! 字符串、字符、注释整个跳过（文本块 `"""…"""` 也是一对一对地吃掉，不出错）；
//! 注解（`@名字` + 可选的一对括号）出一个记号：名字在 `text`，括号里的原文在 `args`（读 `@GetMapping("/x")` 的路径用）。

pub struct Tok {
    pub text: String,
    /// 标识符（关键字也算）
    pub ident: bool,
    /// 注解：括号里的原文（没有括号是空串）。注解记号的 `ident` 是 false —— 它不是类型也不是名字
    pub args: Option<String>,
    /// 从 1 数
    pub line: u32,
    /// 在第几层花括号里（顶层是 0，类 / 接口体是 1）
    pub depth: u32,
    /// 在几层圆括号里（参数表、注解的参数）
    pub paren: u32,
}

pub fn tokens(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let (mut i, mut line, mut depth, mut paren) = (0usize, 1u32, 0u32, 0u32);
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                if c[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
        } else if ch == '"' || ch == '\'' {
            i += 1;
            while i < c.len() && c[i] != ch {
                if c[i] == '\\' {
                    i += 1;
                } else if c[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1;
        } else if ch == '@' && c.get(i + 1).is_some_and(|x| x.is_alphabetic()) {
            // 注解：名字（可能带点：`@org.springframework.web.bind.annotation.GetMapping`，取最后一段）+ 可选的一对括号
            let at_line = line;
            i += 1;
            let s = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '.') {
                i += 1;
            }
            let full: String = c[s..i].iter().collect();
            let name = full.rsplit('.').next().unwrap_or(&full).to_string();
            let mut j = i;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            let mut args = String::new();
            if c.get(j) == Some(&'(') {
                let open = j;
                let mut d = 0;
                while j < c.len() {
                    match c[j] {
                        '(' => d += 1,
                        ')' => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        '\n' => line += 1,
                        q @ ('"' | '\'') => {
                            j += 1;
                            while j < c.len() && c[j] != q {
                                if c[j] == '\\' {
                                    j += 1;
                                }
                                j += 1;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                args = c[open + 1..j.min(c.len())].iter().collect();
                i = j + 1;
            }
            out.push(Tok { text: name, ident: false, args: Some(args), line: at_line, depth, paren });
        } else if ch.is_alphanumeric() || ch == '_' || ch == '$' {
            let s = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '$') {
                i += 1;
            }
            out.push(Tok { text: c[s..i].iter().collect(), ident: true, args: None, line, depth, paren });
        } else {
            // `{` `(` 自己记在外面那一层，后面的才算进去
            out.push(Tok { text: ch.to_string(), ident: false, args: None, line, depth, paren });
            match ch {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                '(' => paren += 1,
                ')' => paren = paren.saturating_sub(1),
                _ => {}
            }
            i += 1;
        }
    }
    out
}

/// `prev` 后面紧跟 `(` 的标识符是不是一个方法声明的名字：前面是一个类型（标识符、`>` 泛型、`]` 数组），不是 `new` / `return` 这种
pub fn after_type(prev: &Tok) -> bool {
    (prev.ident && !["new", "return", "throw", "else", "case"].contains(&prev.text.as_str())) || prev.text == ">" || prev.text == "]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 注解出一个记号_带括号里的原文_全名取最后一段_括号里的字符串不打乱层次() {
        let t = tokens("@org.x.GetMapping(value = \"/a)(\", produces = \"j\")\nint f();\n@Deprecated void g();");
        let anns: Vec<(String, String, u32)> = t.iter().filter_map(|k| k.args.as_ref().map(|a| (k.text.clone(), a.clone(), k.line))).collect();
        assert_eq!(anns, vec![("GetMapping".into(), "value = \"/a)(\", produces = \"j\"".into(), 1), ("Deprecated".into(), String::new(), 3)]);
        let f = t.iter().find(|k| k.text == "f").unwrap();
        assert_eq!((f.line, f.paren), (2, 0), "注解参数里的括号不算进外面");
    }
}
