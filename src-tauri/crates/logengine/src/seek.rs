//! 跳到时间（2026-09-21）：「看 14:32 附近发生了什么」。
//!
//! 九百万行的文件靠滚是不可能的，而日志按时间单调递增 —— 二分。每一步只读一行
//! （mmap 上按需换页），几十次探测就到；不建索引、不预扫。
//!
//! # 时间戳怎么认
//!
//! 只认行首附近（前 64 字节）的几种常见写法，和前端 `parse.ts` 的格式探测对齐：
//!
//! - `2026-08-24 14:03:21.442` / `2026-08-24T14:03:21Z`（Java / Python / ISO / Rust 的 `[...]`）
//! - `[24/Aug/2026:14:03:21` nginx
//! - `Aug 24 14:03:21` syslog（没有年，记 0）
//! - JSON 里 `"time"` / `"ts"` / `"timestamp"` / `"@timestamp"` 后面的 ISO 串
//! - 行首裸的 `14:03:21`（没有日期，记 0）
//!
//! 没时间戳的行（堆栈帧、横幅）二分时往后找最近一条有的，最多 512 行。
//!
//! # 输入怎么解
//!
//! `14:32` / `14:32:05` / `14:32:05.442`，前面可带 `2026-08-24 `。不带日期时用**光标附近那行**
//! 的日期 —— 人说「14:32」指的是正在看的这一天，不是文件的第一天。秒 / 毫秒缺省 0。
//!
//! 返回第一条 **≥** 目标时间的行；全文件都比它早就落到最后一行。乱序的日志（多线程
//! 交错写）二分只能给个大概，那本来也是「附近」。

use crate::index::LineIndex;

/// 打包成一个可比较的整数：((((y*13+mo)*32+d)*24+h)*60+m)*60+s)*1000+ms
fn pack(y: u32, mo: u32, d: u32, h: u32, m: u32, s: u32, ms: u32) -> u64 {
    (((((y as u64 * 13 + mo as u64) * 32 + d as u64) * 24 + h as u64) * 60 + m as u64) * 60 + s as u64) * 1000 + ms as u64
}

fn digits(b: &[u8], at: usize, n: usize) -> Option<u32> {
    let s = b.get(at..at + n)?;
    let mut v = 0u32;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (c - b'0') as u32;
    }
    Some(v)
}

/// `HH:MM:SS(.fff)?` 从 `at` 起；返回 (h, m, s, ms, 吃掉的长度)
fn hms(b: &[u8], at: usize) -> Option<(u32, u32, u32, u32, usize)> {
    let h = digits(b, at, 2)?;
    if b.get(at + 2) != Some(&b':') {
        return None;
    }
    let m = digits(b, at + 3, 2)?;
    let mut len = 5;
    let mut s = 0;
    let mut ms = 0;
    if b.get(at + 5) == Some(&b':') {
        s = digits(b, at + 6, 2)?;
        len = 8;
        if matches!(b.get(at + 8), Some(b'.') | Some(b',')) {
            // 小数位任意长，只取前三位当毫秒
            let mut i = at + 9;
            let mut k = 0;
            while let Some(&c) = b.get(i) {
                if !c.is_ascii_digit() {
                    break;
                }
                if k < 3 {
                    ms = ms * 10 + (c - b'0') as u32;
                }
                k += 1;
                i += 1;
            }
            if k > 0 {
                ms *= 10u32.pow(3u32.saturating_sub(k.min(3) as u32));
                len = i - at;
            }
        }
    }
    if h > 23 || m > 59 || s > 60 {
        return None;
    }
    Some((h, m, s, ms, len))
}

/// `YYYY-MM-DD[ T]HH:MM:SS…` 从 `at` 起
fn iso(b: &[u8], at: usize) -> Option<u64> {
    let y = digits(b, at, 4)?;
    if b.get(at + 4) != Some(&b'-') || b.get(at + 7) != Some(&b'-') {
        return None;
    }
    let mo = digits(b, at + 5, 2)?;
    let d = digits(b, at + 8, 2)?;
    if !matches!(b.get(at + 10), Some(b' ') | Some(b'T')) {
        return None;
    }
    let (h, m, s, ms, _) = hms(b, at + 11)?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    Some(pack(y, mo, d, h, m, s, ms))
}

fn month(b: &[u8]) -> Option<u32> {
    const M: [&[u8]; 12] = [b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec"];
    M.iter().position(|m| b.get(..3) == Some(*m)).map(|i| i as u32 + 1)
}

/// 一行的时间戳键；认不出就 None
pub fn ts_key(line: &[u8]) -> Option<u64> {
    let b = &line[..line.len().min(64)];
    // 行首：ISO、`[ISO`（Rust）、syslog、裸 HH:MM:SS
    let start = if b.first() == Some(&b'[') { 1 } else { 0 };
    if let Some(k) = iso(b, start) {
        return Some(k);
    }
    if b.len() > start + 15 {
        if let Some(mo) = month(&b[start..]) {
            // `Aug 24 14:03:21` / `Aug  4 14:03:21`
            let mut i = start + 3;
            while b.get(i) == Some(&b' ') {
                i += 1;
            }
            let dlen = if b.get(i + 1).is_some_and(|c| c.is_ascii_digit()) { 2 } else { 1 };
            if let Some(d) = digits(b, i, dlen) {
                if b.get(i + dlen) == Some(&b' ') {
                    if let Some((h, m, s, ms, _)) = hms(b, i + dlen + 1) {
                        return Some(pack(0, mo, d, h, m, s, ms));
                    }
                }
            }
        }
    }
    if let Some((h, m, s, ms, _)) = hms(b, start) {
        if b.get(start + 2) == Some(&b':') && b.get(start + 5) == Some(&b':') {
            return Some(pack(0, 0, 0, h, m, s, ms));
        }
    }
    // nginx：`… [24/Aug/2026:14:03:21 +0800]`
    if let Some(lb) = memchr::memchr(b'[', b) {
        let at = lb + 1;
        if let (Some(d), Some(&b'/')) = (digits(b, at, 2), b.get(at + 2)) {
            if let Some(mo) = month(b.get(at + 3..)?) {
                if b.get(at + 6) == Some(&b'/') {
                    if let (Some(y), Some(&b':')) = (digits(b, at + 7, 4), b.get(at + 11)) {
                        if let Some((h, m, s, ms, _)) = hms(b, at + 12) {
                            return Some(pack(y, mo, d, h, m, s, ms));
                        }
                    }
                }
            }
        }
    }
    // JSON：`"time":"2026-…"` 这几种键
    if b.first() == Some(&b'{') {
        for key in [&b"\"time\":\""[..], b"\"ts\":\"", b"\"timestamp\":\"", b"\"@timestamp\":\""] {
            let hay = &line[..line.len().min(200)];
            if let Some(p) = memchr::memmem::find(hay, key) {
                if let Some(k) = iso(hay, p + key.len()) {
                    return Some(k);
                }
            }
        }
    }
    None
}

/// 人输入的目标：`(日期?, 时分秒毫秒)`。日期缺省由调用方按光标附近的行补
pub struct Target {
    pub date: Option<(u32, u32, u32)>,
    pub h: u32,
    pub m: u32,
    pub s: u32,
    pub ms: u32,
}

pub fn parse_target(q: &str) -> Option<Target> {
    let b = q.trim().as_bytes();
    let mut at = 0;
    let mut date = None;
    if b.len() >= 11 && b.get(4) == Some(&b'-') {
        let y = digits(b, 0, 4)?;
        let mo = digits(b, 5, 2)?;
        let d = digits(b, 8, 2)?;
        if !matches!(b.get(10), Some(b' ') | Some(b'T')) {
            return None;
        }
        date = Some((y, mo, d));
        at = 11;
    }
    let (h, m, s, ms, len) = hms(b, at)?;
    if at + len != b.len() {
        return None;
    }
    Some(Target { date, h, m, s, ms })
}

fn line_bytes<'a>(data: &'a [u8], ix: &LineIndex, line: u64) -> Option<&'a [u8]> {
    let start = ix.offset_of_line(data, line)? as usize;
    let end = memchr::memchr(b'\n', &data[start..]).map_or(data.len(), |n| start + n);
    Some(&data[start..end])
}

/// 从 `line` 往后（最多 `limit` 行）找第一条有时间戳的，返回 (行号, 键)
fn key_at_or_after(data: &[u8], ix: &LineIndex, line: u64, limit: u64) -> Option<(u64, u64)> {
    let total = ix.line_count();
    let mut l = line;
    while l < total && l < line + limit {
        if let Some(k) = line_bytes(data, ix, l).and_then(ts_key) {
            return Some((l, k));
        }
        l += 1;
    }
    None
}

/// 光标附近那一行的日期（先往后再往前各找 512 行）；文件里的时间戳没有日期就是 (0,0,0)
fn date_near(data: &[u8], ix: &LineIndex, near: u64) -> Option<(u32, u32, u32)> {
    let key = key_at_or_after(data, ix, near, 512)
        .map(|(_, k)| k)
        .or_else(|| {
            let mut l = near;
            while l > 0 && near - l < 512 {
                l -= 1;
                if let Some(k) = line_bytes(data, ix, l).and_then(ts_key) {
                    return Some(k);
                }
            }
            None
        })?;
    let day = key / (24 * 60 * 60 * 1000);
    Some(((day / (13 * 32)) as u32, ((day / 32) % 13) as u32, (day % 32) as u32))
}

/// 第一条时间 ≥ 目标的行。`near` 是光标附近的行（补日期用）。
/// 整个文件一条时间戳都认不出 → None。
pub fn seek(data: &[u8], ix: &LineIndex, q: &str, near: u64) -> Option<u64> {
    let total = ix.line_count();
    if total == 0 {
        return None;
    }
    let t = parse_target(q)?;
    let (y, mo, d) = match t.date {
        Some(d) => d,
        None => date_near(data, ix, near.min(total - 1))?,
    };
    let target = pack(y, mo, d, t.h, t.m, t.s, t.ms);

    /*
     * 二分的谓词是「从这一行往后最近的时间戳 ≥ 目标」—— 它关于行号单调：mid..l-1 没有
     * 时间戳而 l 的键 ≥ 目标，那 (mid, l] 里每一行的谓词都成立；反过来 l 的键 < 目标，
     * [mid, l] 全都不成立，lo 可以直接跳到 l+1。标准 lower bound，收敛到第一个成立的行。
     */
    let (mut lo, mut hi) = (0u64, total);
    let mut found_any = false;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match key_at_or_after(data, ix, mid, 512) {
            Some((l, k)) => {
                found_any = true;
                if k >= target {
                    hi = mid;
                } else {
                    lo = l + 1;
                }
            }
            // 往后 512 行都没时间戳：只能当作这一段在目标之后，往左收
            None => hi = mid,
        }
    }
    if !found_any {
        return None;
    }
    // 收敛到的可能是目标那条前面的横幅 / 堆栈行（谓词对它们也成立）：落到真正带时间戳的那条。
    // 全文件都比目标早 → lo == total → 最后一行
    Some(key_at_or_after(data, ix, lo, 512).map_or(total - 1, |(l, _)| l))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 认时间戳() {
        let java = ts_key(b"2026-08-24 14:03:21.442 INFO x").unwrap();
        assert_eq!(java, pack(2026, 8, 24, 14, 3, 21, 442));
        assert_eq!(ts_key(b"2026-08-24 14:03:21,442 - x"), Some(java), "Python 的逗号毫秒");
        assert_eq!(ts_key(b"2026-08-24T14:03:21.442Z x"), Some(java), "ISO 的 T");
        assert_eq!(ts_key(b"[2026-08-24T14:03:21Z INFO x"), Some(pack(2026, 8, 24, 14, 3, 21, 0)), "Rust 的方括号");
        assert_eq!(ts_key(b"2026-08-24 14:03:21.4 x"), Some(pack(2026, 8, 24, 14, 3, 21, 400)), "一位小数补成毫秒");
        assert_eq!(ts_key(b"Aug 24 14:03:21 host proc: x"), Some(pack(0, 8, 24, 14, 3, 21, 0)), "syslog");
        assert_eq!(ts_key(b"Aug  4 14:03:21 host x"), Some(pack(0, 8, 4, 14, 3, 21, 0)), "syslog 单位日");
        assert_eq!(
            ts_key(b"1.2.3.4 - - [24/Aug/2026:14:03:21 +0800] \"GET / HTTP/1.1\" 200"),
            Some(java - 442),
            "nginx"
        );
        assert_eq!(ts_key(b"14:03:21 something"), Some(pack(0, 0, 0, 14, 3, 21, 0)), "裸 HH:MM:SS");
        assert_eq!(ts_key(b"{\"level\":\"info\",\"time\":\"2026-08-24T14:03:21.442Z\",\"msg\":\"x\"}"), Some(java), "JSON time");
        assert_eq!(ts_key(b"{\"ts\":\"2026-08-24 14:03:21\",\"lvl\":\"info\"}"), Some(java - 442), "JSON ts");
        assert_eq!(ts_key(b"\tat com.foo.Bar(Bar.java:12)"), None, "堆栈帧");
        assert_eq!(ts_key(b"INFO started"), None, "没时间戳");
        assert_eq!(ts_key(b"2026-13-40 14:03:21 x"), None, "月日越界不认");
        assert_eq!(ts_key(b""), None);
    }

    #[test]
    fn 解输入() {
        let t = parse_target("14:32").unwrap();
        assert_eq!((t.date, t.h, t.m, t.s, t.ms), (None, 14, 32, 0, 0));
        let t = parse_target(" 14:32:05.4 ").unwrap();
        assert_eq!((t.h, t.m, t.s, t.ms), (14, 32, 5, 400));
        let t = parse_target("2026-08-24 14:32:05").unwrap();
        assert_eq!(t.date, Some((2026, 8, 24)));
        assert!(parse_target("1432").is_none(), "没冒号不是时间");
        assert!(parse_target("14:32x").is_none(), "尾巴上有别的");
        assert!(parse_target("25:00").is_none(), "小时越界");
    }

    fn build(body: &[u8]) -> LineIndex {
        let mut ix = LineIndex::new(4);
        ix.extend(body);
        ix.seal(body);
        ix
    }

    const LOG: &[u8] = b"banner line without time\n\
2026-08-24 14:00:00.000 INFO a\n\
2026-08-24 14:01:00.000 INFO b\n\
java.lang.RuntimeException: boom\n\
\tat x.y(Z.java:1)\n\
2026-08-24 14:02:30.000 ERROR c\n\
2026-08-25 09:00:00.000 INFO next day\n\
2026-08-25 09:30:00.000 INFO d\n";

    #[test]
    fn 二分到第一条不早于目标的行() {
        let ix = build(LOG);
        assert_eq!(seek(LOG, &ix, "14:01", 0), Some(2), "正好命中");
        assert_eq!(seek(LOG, &ix, "14:01:30", 0), Some(5), "落在两条之间 → 后一条；中间的堆栈行跳过");
        assert_eq!(seek(LOG, &ix, "13:00", 0), Some(1), "比全文早 → 第一条有时间的（横幅那行不算）");
        assert_eq!(seek(LOG, &ix, "23:00", 0), Some(6), "24 号 23 点没有 → 落到 25 号第一条");
        assert_eq!(seek(LOG, &ix, "09:10", 7), Some(7), "光标在 25 号：日期按 25 号补");
        assert_eq!(seek(LOG, &ix, "2026-08-25 09:10", 0), Some(7), "带日期就不看光标");
        assert_eq!(seek(LOG, &ix, "23:59", 7), Some(7), "比全文晚 → 最后一行");
        assert_eq!(seek(LOG, &ix, "abc", 0), None, "解不出");
        let none = b"no time here\nnor here\n";
        assert_eq!(seek(none, &build(none), "14:00", 0), None, "文件里没时间戳");
    }
}
