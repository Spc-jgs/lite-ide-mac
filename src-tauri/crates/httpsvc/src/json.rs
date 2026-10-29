//! 把一段 JSON 重新缩进，**不经过解析成对象那一步**。
//!
//! 为什么不用 serde_json 的 pretty：它的 `Value` 默认按键名排序（要保序得开 `preserve_order`，多一个依赖），
//! 而且会改写数字（`1.0` 写回成 `1.0`、`1e3` 写回成 `1000.0`，大整数超出 i64/u64 变浮点、丢精度）——
//! 看接口返回值的人要看到的是服务器**原样**回的东西，只是排好看。这里只认字符串（带转义）、括号、逗号、冒号，别的原样抄。
//! 不是合法 JSON（括号对不上、半截）就返回 None，调用方原样显示。

/// 两个空格缩进（IDEA 的响应视图也是两个）
const INDENT: &[u8] = b"  ";

pub fn reindent(src: &[u8]) -> Option<Vec<u8>> {
    reindent_capped(src, super::PRETTY_MAX)
}

// 输入上限管不住缩进：很短的深层数组也能生成巨量空格。每次追加前判输出预算，超限交回原文。
fn append(out: &mut Vec<u8>, bytes: &[u8], cap: usize) -> Option<()> {
    if bytes.len() > cap.saturating_sub(out.len()) {
        return None;
    }
    out.extend_from_slice(bytes);
    Some(())
}

fn reindent_capped(src: &[u8], cap: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(src.len().min(cap));
    let mut depth: usize = 0;
    let mut stack: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut seen = false;
    let newline = |out: &mut Vec<u8>, depth: usize| -> Option<()> {
        let len = out
            .len()
            .checked_add(1)?
            .checked_add(depth.checked_mul(INDENT.len())?)?;
        if len > cap {
            return None;
        }
        out.push(b'\n');
        out.resize(len, b' ');
        Some(())
    };
    while i < src.len() {
        let c = src[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => {}
            b'"' => {
                let start = i;
                i += 1;
                while i < src.len() && src[i] != b'"' {
                    if src[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i >= src.len() {
                    return None;
                }
                append(&mut out, &src[start..=i], cap)?;
                seen = true;
            }
            b'{' | b'[' => {
                stack.push(if c == b'{' { b'}' } else { b']' });
                append(&mut out, &[c], cap)?;
                // 空的 {} / [] 不换行
                let mut j = i + 1;
                while j < src.len() && matches!(src[j], b' ' | b'\t' | b'\r' | b'\n') {
                    j += 1;
                }
                if j < src.len() && src[j] == *stack.last()? {
                    append(&mut out, &[src[j]], cap)?;
                    stack.pop();
                    i = j;
                } else {
                    depth += 1;
                    newline(&mut out, depth)?;
                }
                seen = true;
            }
            b'}' | b']' => {
                if stack.pop()? != c {
                    return None;
                }
                depth = depth.checked_sub(1)?;
                newline(&mut out, depth)?;
                append(&mut out, &[c], cap)?;
            }
            b',' => {
                append(&mut out, &[c], cap)?;
                newline(&mut out, depth)?;
            }
            b':' => append(&mut out, b": ", cap)?,
            _ => {
                append(&mut out, &[c], cap)?;
                seen = true;
            }
        }
        i += 1;
    }
    (stack.is_empty() && seen).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Option<String> {
        reindent(x.as_bytes()).map(|v| String::from_utf8(v).unwrap())
    }

    #[test]
    fn 键的次序和数字原样_字符串里的括号逗号引号不算() {
        let got = s(r#"{"zeta":1.0,"alpha":12345678901234567890,"s":"a,b:{c}\"]","e":[],"o":{}}"#).unwrap();
        assert_eq!(got, "{\n  \"zeta\": 1.0,\n  \"alpha\": 12345678901234567890,\n  \"s\": \"a,b:{c}\\\"]\",\n  \"e\": [],\n  \"o\": {}\n}");
    }

    #[test]
    fn 嵌套_数组里的对象() {
        assert_eq!(s(r#"[{"a":[1,2]},null]"#).unwrap(), "[\n  {\n    \"a\": [\n      1,\n      2\n    ]\n  },\n  null\n]");
    }

    #[test]
    fn 不是_json_给_none() {
        assert_eq!(s("{\"a\":1"), None, "半截");
        assert_eq!(s("[1}"), None, "括号对不上");
        assert_eq!(s("\"没收尾"), None);
        assert_eq!(s(""), None, "空的不算");
        assert_eq!(s("   "), None);
    }

    #[test]
    fn 嵌套输入不能靠缩进无限放大() {
        let src = format!("{}0{}", "[".repeat(3000), "]".repeat(3000));
        assert!(
            reindent(src.as_bytes()).is_none(),
            "6001 字节的输入会扩成约 18 MB，必须在写输出时停"
        );
    }

    #[test]
    fn 输出恰好到线能排_超过一字节也回原文() {
        for src in [
            r#"{"x":"中文\\\"\n","n":1e3}"#,
            "[1,2]",
            "[]",
            "{}",
            "null",
            "\"long string\"",
        ] {
            let pretty = reindent(src.as_bytes()).unwrap();
            assert_eq!(
                reindent_capped(src.as_bytes(), pretty.len()),
                Some(pretty.clone())
            );
            assert!(
                reindent_capped(src.as_bytes(), pretty.len() - 1).is_none(),
                "{src}"
            );
            assert!(reindent_capped(src.as_bytes(), 0).is_none());
        }
    }
}
