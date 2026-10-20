//! 把一段 JSON 重新缩进，**不经过解析成对象那一步**。
//!
//! 为什么不用 serde_json 的 pretty：它的 `Value` 默认按键名排序（要保序得开 `preserve_order`，多一个依赖），
//! 而且会改写数字（`1.0` 写回成 `1.0`、`1e3` 写回成 `1000.0`，大整数超出 i64/u64 变浮点、丢精度）——
//! 看接口返回值的人要看到的是服务器**原样**回的东西，只是排好看。这里只认字符串（带转义）、括号、逗号、冒号，别的原样抄。
//! 不是合法 JSON（括号对不上、半截）就返回 None，调用方原样显示。

/// 两个空格缩进（IDEA 的响应视图也是两个）
const INDENT: &[u8] = b"  ";

pub fn reindent(src: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(src.len() + src.len() / 4);
    let mut depth: usize = 0;
    let mut stack: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut seen = false;
    let newline = |out: &mut Vec<u8>, depth: usize| {
        out.push(b'\n');
        for _ in 0..depth {
            out.extend_from_slice(INDENT);
        }
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
                out.extend_from_slice(&src[start..=i]);
                seen = true;
            }
            b'{' | b'[' => {
                stack.push(if c == b'{' { b'}' } else { b']' });
                out.push(c);
                // 空的 {} / [] 不换行
                let mut j = i + 1;
                while j < src.len() && matches!(src[j], b' ' | b'\t' | b'\r' | b'\n') {
                    j += 1;
                }
                if j < src.len() && src[j] == *stack.last()? {
                    out.push(src[j]);
                    stack.pop();
                    i = j;
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
                seen = true;
            }
            b'}' | b']' => {
                if stack.pop()? != c {
                    return None;
                }
                depth = depth.checked_sub(1)?;
                newline(&mut out, depth);
                out.push(c);
            }
            b',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            b':' => out.extend_from_slice(b": "),
            _ => {
                out.push(c);
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
}
