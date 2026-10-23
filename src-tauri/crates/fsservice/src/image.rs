//! 图片预览（docs/TAKEOVER.md G26）：认出「这是一张图」，把它的字节交出去。
//!
//! 原来 PNG 走 `logengine::probe`，里面有 NUL 字节 → 判成「二进制文件」→ 日志模式，满屏乱码。
//! IDEA / VS Code 都是直接画出来：看一眼图标、截图、设计稿是很常见的「打开一个文件」。

use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// 图片读多大为止。图标、截图几十 KB 到几 MB；超过这个数的多半是素材原图，预览它会把整份字节过一趟 IPC、
/// 再在页面里存一份 data URL（约 1.33 倍）—— 一张 64MB 的图就是两百多兆内存。超了就说，不硬画
pub const MAX_IMAGE_BYTES: u64 = 32 << 20;

/// 按扩展名 + 文件头认图，认出来给 MIME 类型。
///
/// **两样都要对上**：只看扩展名的话，一个改了名的文本文件（`notes.png`）会被当成图、画出一个破图标，
/// 而它本来可以用编辑器打开；只看文件头的话，没有扩展名的二进制碰巧以 `BM` 开头也成了图。
/// 对不上就返回 `None`，交回原来那条路（编辑器 / 日志模式），不比原来差。
///
/// SVG 是文本，没有固定的文件头：看前 1KB 里有没有 `<svg`（前面可能是 XML 声明、注释、DOCTYPE）。
/// 它在页面里是当 `<img>` 画的 —— `<img>` 里的 SVG 不跑脚本、不加载外部资源（浏览器的规矩），
/// 所以别人仓库里一份带 `<script>` 的 SVG 打开看一眼是安全的
pub fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mut head = [0u8; 1024];
    let n = fs::File::open(path).and_then(|mut f| read_up_to(&mut f, &mut head)).ok()?;
    sniff(&ext, &head[..n])
}

fn read_up_to(f: &mut fs::File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// 纯的那一半：扩展名和文件头 → MIME
pub(crate) fn sniff(ext: &str, head: &[u8]) -> Option<&'static str> {
    let (mime, ok) = match ext {
        "png" => ("image/png", head.starts_with(b"\x89PNG\r\n\x1a\n")),
        "jpg" | "jpeg" => ("image/jpeg", head.starts_with(b"\xff\xd8\xff")),
        "gif" => ("image/gif", head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a")),
        "webp" => ("image/webp", head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP"),
        "bmp" => ("image/bmp", head.starts_with(b"BM")),
        // ICO 头：保留 0、类型 1（图标）；类型 2 是光标（.cur），不在这里
        "ico" => ("image/x-icon", head.starts_with(b"\x00\x00\x01\x00")),
        "svg" => ("image/svg+xml", has_svg_tag(head)),
        _ => return None,
    };
    ok.then_some(mime)
}

fn has_svg_tag(head: &[u8]) -> bool {
    head.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg"))
}

/// 读整张图的字节，带上限（同 `read_text_detect`：边读边卡，多读一个字节分清「正好等于」和「后面还有」）
pub fn read_image(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    read_image_capped(path.as_ref(), MAX_IMAGE_BYTES)
}

pub(crate) fn read_image_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(io::Error::other(format!(
            "图片超过 {} MB，不预览了 —— 在访达里用「预览」打开",
            cap >> 20
        )));
    }
    Ok(bytes)
}
