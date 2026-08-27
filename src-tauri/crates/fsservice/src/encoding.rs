//! 文本编码探测、解码与回写。
//!
//! # 为什么要有这一层
//!
//! 早先的策略是「只支持 UTF-8，其余一律拒绝」。理由是正当的 —— lossy 解码之后
//! 保存会把原文件写坏，比打不开糟糕得多。但对中文项目来说，代价太大了：
//! 老 Java 工程、Windows 上生成的日志、一堆 `.properties`，GBK 遍地都是。
//!
//! 现在的策略是**探测 → 记住 → 原样写回**：用什么编码读进来的，就用什么编码存回去。
//! 保存不再是「转成 UTF-8」这种擅自的决定。
//!
//! # 探测顺序
//!
//! 1. **BOM** —— 有就信它，这是文件自己声明的，没有比这更可靠的证据
//! 2. **UTF-8 合法性** —— 能通过校验就是 UTF-8（纯 ASCII 也走这条）。
//!    这不是猜：非 UTF-8 的字节序列碰巧通过 UTF-8 校验的概率极低
//! 3. **统计探测** —— 到这一步才开始猜，交给 chardetng（Firefox 用的那个）
//!
//! 顺序不能反。先跑统计探测的话，一份纯 ASCII 的文件可能被判成 windows-1252，
//! 存回去就带上了错误的编码标签。

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};

/// 一次解码的结果
#[derive(Debug, Clone)]
pub struct Decoded {
    pub content: String,
    /// WHATWG 编码标签，如 `UTF-8` / `GBK` / `Shift_JIS`
    pub encoding: &'static str,
    /// 文件开头带 BOM
    pub bom: bool,
    /// 解码过程中出现了替换字符 —— 原文里有这个编码解释不了的字节。
    ///
    /// **这个标记必须一路传到界面上**：带着它保存等于把那些字节永久换成 `U+FFFD`。
    /// 用户有权在覆盖之前知道这件事。
    pub lossy: bool,
}

/// 探测并解码。
pub fn decode(bytes: &[u8]) -> Decoded {
    // 1. BOM 是文件自己的声明，优先级最高
    if let Some((enc, len)) = sniff_bom(bytes) {
        let (cow, _, lossy) = enc.decode(&bytes[len..]);
        return Decoded {
            content: cow.into_owned(),
            encoding: enc.name(),
            bom: true,
            lossy,
        };
    }

    // 2. 合法 UTF-8（含纯 ASCII）
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Decoded {
            content: s.to_owned(),
            encoding: UTF_8.name(),
            bom: false,
            lossy: false,
        };
    }

    // 3. 到这里才开始猜
    let mut det = chardetng::EncodingDetector::new();
    det.feed(bytes, true);
    // allow_utf8 = false：UTF-8 在上一步已经用「校验」判过了，
    // 这里再让它猜 UTF-8 只会把一份坏掉的 GBK 文件误判回去
    let enc = det.guess(None, false);
    let (cow, _, lossy) = enc.decode(bytes);
    Decoded {
        content: cow.into_owned(),
        encoding: enc.name(),
        bom: false,
        lossy,
    }
}

/// 按指定标签解码。标签不认识时退回自动探测。
pub fn decode_as(bytes: &[u8], label: &str) -> Decoded {
    let Some(enc) = Encoding::for_label(label.as_bytes()) else {
        return decode(bytes);
    };
    // 用户点名了编码，但文件开头的 BOM 仍然要跳过，否则正文里会多一个 U+FEFF
    let body = match sniff_bom(bytes) {
        Some((bom_enc, len)) if bom_enc == enc => &bytes[len..],
        _ => bytes,
    };
    let (cow, _, lossy) = enc.decode(body);
    Decoded {
        content: cow.into_owned(),
        encoding: enc.name(),
        bom: sniff_bom(bytes).is_some(),
        lossy,
    }
}

/// 按指定编码编码回字节，`bom` 为真时加上 BOM。
///
/// UTF-16 要单独处理：Encoding Standard **只定义了 UTF-16 的解码**，
/// encoding_rs 的 `encode()` 对 UTF-16 会退回 UTF-8。手写这十几行，
/// 免得一份 UTF-16 的文件打开能看、一保存就悄悄变成 UTF-8。
pub fn encode(content: &str, label: &str, bom: bool) -> Vec<u8> {
    let enc = Encoding::for_label(label.as_bytes()).unwrap_or(UTF_8);

    if enc == UTF_16LE || enc == UTF_16BE {
        let le = enc == UTF_16LE;
        let mut out = Vec::with_capacity(content.len() * 2 + 2);
        if bom {
            out.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
        }
        for u in content.encode_utf16() {
            let b = if le { u.to_le_bytes() } else { u.to_be_bytes() };
            out.extend_from_slice(&b);
        }
        return out;
    }

    let (cow, _, _) = enc.encode(content);
    if bom && enc == UTF_8 {
        let mut out = Vec::with_capacity(cow.len() + 3);
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        out.extend_from_slice(&cow);
        return out;
    }
    cow.into_owned()
}

/// 认 BOM，返回 (编码, BOM 字节数)
fn sniff_bom(b: &[u8]) -> Option<(&'static Encoding, usize)> {
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some((UTF_8, 3));
    }
    // UTF-16 的两个 BOM 是互为反序的两个字节，判断顺序无所谓
    if b.starts_with(&[0xFF, 0xFE]) {
        return Some((UTF_16LE, 2));
    }
    if b.starts_with(&[0xFE, 0xFF]) {
        return Some((UTF_16BE, 2));
    }
    None
}

/// 界面上给用户挑的编码清单。
///
/// 不列 encoding_rs 支持的全部四十来种 —— 那是给浏览器兼容用的长尾，
/// 摆在菜单里只会让人找不到自己要的那个。这里只放实际会遇到的。
pub const COMMON: &[(&str, &str)] = &[
    ("UTF-8", "UTF-8"),
    ("GB18030", "GB18030（简体中文，GBK 的超集）"),
    ("GBK", "GBK（简体中文）"),
    ("Big5", "Big5（繁体中文）"),
    ("Shift_JIS", "Shift_JIS（日文）"),
    ("EUC-KR", "EUC-KR（韩文）"),
    ("UTF-16LE", "UTF-16 小端"),
    ("UTF-16BE", "UTF-16 大端"),
    ("windows-1252", "windows-1252（西欧）"),
    ("KOI8-R", "KOI8-R（俄文）"),
];

/// 标签规范化成 encoding_rs 的正式名字；不认识返回 None
pub fn canonical(label: &str) -> Option<&'static str> {
    Encoding::for_label(label.as_bytes()).map(|e| e.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 纯ascii判成utf8而不是去猜() {
        let d = decode(b"hello world\nplain ascii\n");
        assert_eq!(d.encoding, "UTF-8");
        assert!(!d.bom && !d.lossy);
    }

    #[test]
    fn 合法utf8中文() {
        let d = decode("你好，世界\n".as_bytes());
        assert_eq!(d.encoding, "UTF-8");
        assert_eq!(d.content, "你好，世界\n");
        assert!(!d.lossy);
    }

    #[test]
    fn utf8的bom要被吃掉而不是留在正文里() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("内容".as_bytes());
        let d = decode(&b);
        assert_eq!(d.encoding, "UTF-8");
        assert!(d.bom);
        assert_eq!(d.content, "内容", "BOM 漏进正文了");
    }

    #[test]
    fn gbk往返() {
        let text = "订单处理失败，重试中\n第二行\n";
        let bytes = encode(text, "GBK", false);
        // GBK 的中文是双字节，不该等于 UTF-8 的三字节长度
        assert!(bytes.len() < text.len(), "看起来没真的编成 GBK");
        let d = decode(&bytes);
        assert!(
            d.encoding == "GBK" || d.encoding == "gb18030",
            "探测结果是 {}",
            d.encoding
        );
        assert_eq!(d.content, text);
        assert!(!d.lossy);
    }

    #[test]
    fn big5往返() {
        let text = "訂單處理失敗，重試中\n";
        let bytes = encode(text, "Big5", false);
        let d = decode_as(&bytes, "Big5");
        assert_eq!(d.content, text);
        assert!(!d.lossy);
    }

    #[test]
    fn utf16带bom能读也能写回去() {
        let text = "混合 mixed 内容\n";
        for label in ["UTF-16LE", "UTF-16BE"] {
            let bytes = encode(text, label, true);
            let d = decode(&bytes);
            assert_eq!(d.encoding, label, "{label} 的 BOM 没认出来");
            assert!(d.bom);
            assert_eq!(d.content, text, "{label} 往返内容不一致");
        }
    }

    /// 这条是冲着「打开能看、一存就悄悄变 UTF-8」那个 bug 去的
    #[test]
    fn utf16不能被悄悄编成utf8() {
        let bytes = encode("abc", "UTF-16LE", false);
        // UTF-16LE 下 "abc" 是 6 字节；若退回 UTF-8 就是 3 字节
        assert_eq!(bytes.len(), 6, "UTF-16 被退回 UTF-8 编码了：{bytes:?}");
        assert_eq!(bytes, vec![b'a', 0, b'b', 0, b'c', 0]);
    }

    #[test]
    fn 解不出来的字节要报lossy() {
        // 0xFF 在 UTF-8 里非法，在 GBK 里也是非法首字节
        let d = decode_as(&[b'a', 0xFF, b'b'], "GBK");
        assert!(d.lossy, "应该报告有解不出的字节");
        assert!(d.content.contains('\u{FFFD}'));
    }

    #[test]
    fn 指定编码优先于自动探测() {
        // 这串字节按 GBK 和按 windows-1252 都能解出东西，结果不同
        let bytes = encode("中文", "GBK", false);
        let g = decode_as(&bytes, "GBK");
        let w = decode_as(&bytes, "windows-1252");
        assert_eq!(g.content, "中文");
        assert_ne!(w.content, "中文", "指定的编码没被采纳");
    }

    #[test]
    fn 不认识的标签退回自动探测() {
        let d = decode_as("你好".as_bytes(), "not-a-real-encoding");
        assert_eq!(d.encoding, "UTF-8");
        assert_eq!(d.content, "你好");
    }

    #[test]
    fn 清单里的标签必须都认识() {
        for (label, desc) in COMMON {
            assert!(
                canonical(label).is_some(),
                "清单里的 {label}（{desc}）encoding_rs 不认识"
            );
        }
    }

    #[test]
    fn 空文件不崩() {
        let d = decode(b"");
        assert_eq!(d.content, "");
        assert_eq!(d.encoding, "UTF-8");
        assert!(!d.lossy);
    }
}
