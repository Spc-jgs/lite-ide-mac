//! 换行符：读进来时探测并统一成 `\n`，写回去时换回原样（issue #33 ③）。
//!
//! # 为什么要在这一层做
//!
//! CodeMirror 内部一律用 `\n`：`EditorState` 建文档时把 `\r\n` / `\r` 都拆成行，
//! `doc.toString()` 用 `\n` 拼回来。也就是说一份 CRLF 文件只要经编辑器一存，
//! **换行符就静默换成了 LF** —— 整个文件每一行都算改动，git diff 一片红，
//! 而人只改了一个字。以前就是这样，没人发现是因为 macOS 上 CRLF 文件少。
//!
//! 和编码是同一件事：**读进来是什么就用什么存回去**。编码在 [`super::encoding`]，
//! 换行符在这里，两条都在 IPC 边界上做，前端拿到的永远是 `\n`，不用管。
//!
//! # 混用
//!
//! 一份文件里既有 `\r\n` 又有 `\n`（Windows 上打开过一半的那种），报「混用」，
//! 写回去时**不动**（统一成 `\n` 存）—— 猜一种写回去等于替用户做决定，
//! 而混用的文件本来就是坏的，界面上说出来比悄悄修正诚实。

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
    Mixed,
}

impl Eol {
    /// 过 IPC 的标签。前端原样显示（`混用` 那条前端自己翻）
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
            Eol::Mixed => "mixed",
        }
    }

    pub fn from_label(s: &str) -> Eol {
        match s {
            "CRLF" => Eol::CrLf,
            "CR" => Eol::Cr,
            "mixed" => Eol::Mixed,
            _ => Eol::Lf,
        }
    }
}

/// 看整份文本用的是哪种换行。没有换行的单行文件算 LF —— 写回去时就是它。
pub fn detect(text: &str) -> Eol {
    let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' => {
                if b.get(i + 1) == Some(&b'\n') {
                    crlf += 1;
                    i += 1;
                } else {
                    cr += 1;
                }
            }
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    let kinds = (lf > 0) as u8 + (crlf > 0) as u8 + (cr > 0) as u8;
    if kinds > 1 {
        Eol::Mixed
    } else if crlf > 0 {
        Eol::CrLf
    } else if cr > 0 {
        Eol::Cr
    } else {
        Eol::Lf
    }
}

/// 统一成 `\n`。已经是 LF 的不分配。
pub fn normalize(text: String) -> String {
    if !text.contains('\r') {
        return text;
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `\n` 换回 `eol`。LF 和混用都原样返回（混用的理由见文件头）。
pub fn denormalize(text: &str, eol: Eol) -> Cow<'_, str> {
    match eol {
        Eol::Lf | Eol::Mixed => Cow::Borrowed(text),
        Eol::CrLf => Cow::Owned(text.replace('\n', "\r\n")),
        Eol::Cr => Cow::Owned(text.replace('\n', "\r")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 探测四种() {
        assert_eq!(detect("a\nb\n"), Eol::Lf);
        assert_eq!(detect("a\r\nb\r\n"), Eol::CrLf);
        assert_eq!(detect("a\rb\r"), Eol::Cr);
        assert_eq!(detect("a\nb\r\n"), Eol::Mixed);
        assert_eq!(detect("单行"), Eol::Lf, "没有换行按 LF");
        assert_eq!(detect(""), Eol::Lf);
    }

    #[test]
    fn crlf_一来一回不丢不多() {
        let disk = "第一行\r\n第二行\r\n";
        let eol = detect(disk);
        let inner = normalize(disk.to_string());
        assert_eq!(inner, "第一行\n第二行\n", "编辑器里只能有 \\n");
        assert_eq!(denormalize(&inner, eol), disk, "写回去要和盘上一模一样");
        // 编辑器里加一行，写回去那一行也得是 CRLF
        assert_eq!(denormalize("a\nb\n", Eol::CrLf), "a\r\nb\r\n");
    }

    #[test]
    fn 混用的写回去不动() {
        let disk = "a\nb\r\n";
        assert_eq!(detect(disk), Eol::Mixed);
        assert_eq!(denormalize("a\nb\n", Eol::Mixed), "a\nb\n", "混用不猜，统一成 LF 存");
    }
}
