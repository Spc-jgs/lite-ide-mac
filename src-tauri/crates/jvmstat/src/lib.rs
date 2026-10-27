//! 跑着的 JVM 堆用了多少、GC 了几次（docs/IDEA.md 第 27 节）：读 JVM 自己写的性能计数文件 `hsperfdata`，`jstat` 读的就是它。
//!
//! 几条为什么：
//! - **不起子进程**：`jcmd <pid> GC.heap_info`、`jstat` 每次都要起一个 JVM（几百毫秒、几十 MB），每秒问一次就是另一个负担。
//!   `hsperfdata` 是 JVM 映射到磁盘上的一块共享内存（`-XX:+UsePerfData`，默认开着），读一次几十 KB、几微秒，对被看的 JVM 零开销
//! - **不连 JMX**：要被看的程序开端口、带参数起，而且是在它的 JVM 里跑代码（MBean）—— 读文件什么都不用它做
//! - **文件格式**按 jdk21u 的 `perfMemory.hpp`（`PerfDataPrologue` / `PerfDataEntry`）读：开头固定一段头（魔数、字节序、条目从哪开始、几条），
//!   之后一条条「名字 + 类型 + 值」。取标量的 long（`J`）和字符串（`B` 的向量，到 0 为止）—— 字符串只用一个：收集器叫什么（见 [`heap_of`] 的上限）
//! - **在哪**：`<java.io.tmpdir>/hsperfdata_<用户>/<pid>`。macOS 上 JVM 的临时目录就是用户的 `$TMPDIR`（和我们的 `temp_dir()` 是同一个）；
//!   用户名不自己猜，把临时目录下 `hsperfdata_*` 里叫这个 pid 的找出来

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 一个 JVM 此刻的堆，字节
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Heap {
    /// 各代各区已经用了的加起来
    pub used: u64,
    /// JVM 已经向系统要来的（`capacity`）
    pub committed: u64,
    /// 最多能长到多大（`-Xmx` 那个数，各代的 `maxCapacity` 加起来）
    pub max: u64,
    /// 一共 GC 了几次（各个收集器加起来：新生代的、老年代的）
    pub gc_count: u64,
    /// GC 一共花了多少毫秒
    pub gc_ms: u64,
}

/// 这个 pid 的计数文件。找不到（不是 JVM、关了 UsePerfData、已经退了）是 `None`
pub fn perf_file(tmp: &Path, pid: u32) -> Option<PathBuf> {
    let rd = std::fs::read_dir(tmp).ok()?;
    rd.flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with("hsperfdata_")))
        .map(|e| e.path().join(pid.to_string()))
        .find(|p| p.is_file())
}

/// 读一个 JVM 的堆。文件只读不写；JVM 在一直改它，读到的是那一刻的值
pub fn read(tmp: &Path, pid: u32) -> Result<Heap, String> {
    let f = perf_file(tmp, pid).ok_or("未找到性能计数器（不是 JVM，或启用了 -XX:-UsePerfData）")?;
    let bytes = std::fs::read(&f).map_err(|e| format!("无法读取 {}：{e}", f.display()))?;
    let c = parse(&bytes).ok_or("无法识别性能计数器的格式")?;
    heap_of(&c).ok_or_else(|| "性能计数器中没有堆数据（可能不是 HotSpot 的分代收集器）".into())
}

/// 一个 JVM 的计数：数字的和字符串的
#[derive(Debug, Default, PartialEq)]
pub struct Counters {
    pub longs: HashMap<String, i64>,
    pub texts: HashMap<String, String>,
}

/// 解析出全部标量 long 和字符串计数。不是 `hsperfdata`、JVM 还没写好（`accessible` 是 0）、读到一半越界的都是 `None`
pub fn parse(b: &[u8]) -> Option<Counters> {
    // 魔数按字节就是 CA FE C0 C0，不管机器是哪种字节序（JVM 写之前按需要翻过）
    if b.get(..4)? != [0xCA, 0xFE, 0xC0, 0xC0] {
        return None;
    }
    let little = match *b.get(4)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if *b.get(7)? == 0 {
        return None;
    }
    let u4 = |at: usize| -> Option<u32> {
        let s: [u8; 4] = b.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if little { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    };
    let i8_ = |at: usize| -> Option<i64> {
        let s: [u8; 8] = b.get(at..at.checked_add(8)?)?.try_into().ok()?;
        Some(if little { i64::from_le_bytes(s) } else { i64::from_be_bytes(s) })
    };
    let mut at = u4(24)? as usize;
    let n = u4(28)?;
    let mut out = Counters::default();
    for _ in 0..n {
        let len = u4(at)? as usize;
        if len == 0 {
            return None;
        }
        let name_at = at.checked_add(u4(at + 4)? as usize)?;
        let vector = u4(at + 8)?;
        let ty = *b.get(at + 12)?;
        let data_at = at.checked_add(u4(at + 16)? as usize)?;
        let name_end = name_at + b.get(name_at..)?.iter().position(|&c| c == 0)?;
        if ty == b'J' && vector == 0 {
            // 值读不全（文件比头上说的短）整份不认：悄悄少一项的话，算出来的堆看着对、其实少了一块
            let v = i8_(data_at)?;
            if let Ok(name) = std::str::from_utf8(&b[name_at..name_end]) {
                out.longs.insert(name.to_string(), v);
            }
        } else if ty == b'B' && vector > 0 {
            let raw = b.get(data_at..data_at.checked_add(vector as usize)?)?;
            let text = &raw[..raw.iter().position(|&c| c == 0).unwrap_or(raw.len())];
            if let (Ok(name), Ok(text)) = (std::str::from_utf8(&b[name_at..name_end]), std::str::from_utf8(text)) {
                out.texts.insert(name.to_string(), text.to_string());
            }
        }
        at = at.checked_add(len)?;
    }
    Some(out)
}

/// 从计数里算堆。名字是 HotSpot 的（`jstat -gc` 用的同一组）：`sun.gc.generation.<代>.space.<区>.used`、`sun.gc.generation.<代>.capacity` / `maxCapacity`、
/// `sun.gc.collector.<n>.invocations` / `time`（时间是高精度计时器的刻度，除以 `sun.os.hrt.frequency` 才是秒）。没有任何一代的数就是 `None`。
///
/// **上限不能一律相加**（真 JVM 上撞出来的：`-Xmx256m` 读成了 512 MB）：G1 的新生代、老年代是同一堆区域换着用，两代的 `maxCapacity`
/// 都报整个堆（`jstat -gccapacity` 里 NGCMX = OGCMX = 256 MB）；串行、并行收集器两代是分开的两块，相加才是整个堆。
/// 收集器叫什么看 `sun.gc.policy.name`（G1 是 `GarbageFirst`）
pub fn heap_of(cs: &Counters) -> Option<Heap> {
    let c = &cs.longs;
    let shared = cs.texts.get("sun.gc.policy.name").is_some_and(|n| n.contains("GarbageFirst"));
    let pos = |v: &i64| (*v).max(0) as u64;
    let mut h = Heap::default();
    let mut any = false;
    for (k, v) in c {
        let Some(rest) = k.strip_prefix("sun.gc.generation.") else { continue };
        let mut parts = rest.split('.');
        let (Some(_gen), Some(what)) = (parts.next(), parts.next()) else { continue };
        match (what, parts.next(), parts.next(), parts.next()) {
            ("space", Some(_), Some("used"), None) => {
                h.used += pos(v);
                any = true;
            }
            ("capacity", None, _, _) => h.committed += pos(v),
            ("maxCapacity", None, _, _) => h.max = if shared { h.max.max(pos(v)) } else { h.max + pos(v) },
            _ => {}
        }
    }
    let freq = c.get("sun.os.hrt.frequency").copied().filter(|f| *f > 0).unwrap_or(1_000_000_000);
    let mut ticks: u128 = 0;
    for (k, v) in c {
        let Some(rest) = k.strip_prefix("sun.gc.collector.") else { continue };
        match rest.split_once('.') {
            Some((_, "invocations")) => h.gc_count += pos(v),
            Some((_, "time")) => ticks += pos(v) as u128,
            _ => {}
        }
    }
    h.gc_ms = (ticks * 1000 / freq as u128) as u64;
    any.then_some(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 照 `perfMemory.hpp` 的布局拼一份：头 32 字节，之后每条「定长 20 字节 + 名字（含结尾的 0）+ 补齐到 8 + 8 字节的值」
    fn file(little: bool, entries: &[(&str, i64)]) -> Vec<u8> {
        let w4 = |v: u32| if little { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut b = vec![0xCA, 0xFE, 0xC0, 0xC0, little as u8, 2, 0, 1];
        b.extend_from_slice(&w4(0));
        b.extend_from_slice(&w4(0));
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&w4(32));
        b.extend_from_slice(&w4(entries.len() as u32 + 1));
        let mut push = |name: &str, ty: u8, vector: u32, val: &[u8]| {
            let name_len = name.len() + 1;
            let data_off = (20 + name_len).div_ceil(8) * 8;
            let len = data_off + val.len();
            b.extend_from_slice(&w4(len as u32));
            b.extend_from_slice(&w4(20));
            b.extend_from_slice(&w4(vector));
            b.extend_from_slice(&[ty, 0, 0, 0]);
            b.extend_from_slice(&w4(data_off as u32));
            b.extend_from_slice(name.as_bytes());
            b.resize(b.len() + data_off - 20 - name.len(), 0);
            b.extend_from_slice(val);
        };
        // 字符串（vector 的 B）：不能把它的字节当成 long
        push("java.property.java.version", b'B', 8, b"21\0\0\0\0\0\0");
        for (n, v) in entries {
            push(n, b'J', 0, &if little { v.to_le_bytes() } else { v.to_be_bytes() });
        }
        b
    }

    const MB: i64 = 1 << 20;

    fn sample() -> Vec<(&'static str, i64)> {
        vec![
            ("sun.gc.generation.0.space.0.used", 10 * MB),
            ("sun.gc.generation.0.space.1.used", MB),
            ("sun.gc.generation.0.space.2.used", 0),
            ("sun.gc.generation.1.space.0.used", 30 * MB),
            ("sun.gc.generation.0.capacity", 20 * MB),
            ("sun.gc.generation.1.capacity", 60 * MB),
            ("sun.gc.generation.0.maxCapacity", 100 * MB),
            ("sun.gc.generation.1.maxCapacity", 412 * MB),
            // 每一区自己的 capacity 不能再加一遍（代的 capacity 已经包含了）
            ("sun.gc.generation.0.space.0.capacity", 15 * MB),
            ("sun.gc.collector.0.invocations", 7),
            ("sun.gc.collector.1.invocations", 1),
            ("sun.gc.collector.0.time", 30_000_000),
            ("sun.gc.collector.1.time", 12_000_000),
            ("sun.os.hrt.frequency", 1_000_000_000),
        ]
    }

    #[test]
    fn 两种字节序都读得对_字符串读到0为止() {
        for little in [true, false] {
            let c = parse(&file(little, &sample())).unwrap();
            assert_eq!(c.longs.get("sun.gc.generation.1.space.0.used"), Some(&(30 * MB)), "little={little}");
            assert!(!c.longs.contains_key("java.property.java.version"), "字符串的字节不当成数");
            assert_eq!(c.texts.get("java.property.java.version").map(String::as_str), Some("21"));
        }
    }

    #[test]
    fn 上限_g1两代共用一个堆取最大的_分代收集器相加() {
        let mut c = parse(&file(true, &sample())).unwrap();
        assert_eq!(heap_of(&c).unwrap().max, 512 * MB as u64, "并行 / 串行：两代各自的上限加起来");
        c.texts.insert("sun.gc.policy.name".into(), "GarbageFirst".into());
        c.longs.insert("sun.gc.generation.0.maxCapacity".into(), 256 * MB);
        c.longs.insert("sun.gc.generation.1.maxCapacity".into(), 256 * MB);
        assert_eq!(heap_of(&c).unwrap().max, 256 * MB as u64, "G1：两代都报整个堆，相加就算了两遍");
    }

    #[test]
    fn 堆_各区用量相加_代的容量相加_区的容量不重复算_gc按频率换成毫秒() {
        let h = heap_of(&parse(&file(true, &sample())).unwrap()).unwrap();
        assert_eq!(h, Heap { used: 41 * MB as u64, committed: 80 * MB as u64, max: 512 * MB as u64, gc_count: 8, gc_ms: 42 });
    }

    #[test]
    fn 不是这种文件_还没写好_截断的_都不当真() {
        assert_eq!(parse(b"not perf data at all, definitely"), None);
        let mut b = file(true, &sample());
        b[7] = 0;
        assert_eq!(parse(&b), None, "accessible 还是 0：JVM 还没写好");
        let b = file(true, &sample());
        assert_eq!(parse(&b[..b.len() - 4]), None, "最后一条的值读到一半");
        assert_eq!(heap_of(&Counters::default()), None, "一个代的数都没有（不是 HotSpot 的分代收集器）");
    }
}
