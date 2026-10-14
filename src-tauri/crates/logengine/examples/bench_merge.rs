//! 合并视图（`merge.rs`）的性能：首屏、合完、内存、随机定位、按时间跳、过滤。
//!
//!   cargo run -p logengine --release --example gen_log -- /tmp/a.log 67108864   （造几份，各 64MB 就够看出量级）
//!   cargo run -p logengine --release --example bench_merge -- /tmp/a.log /tmp/b.log /tmp/c.log

use logengine::merge::MergedLog;
use logengine::{FilterSpec, LevelMask, TextFilter};
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let total: u64 = paths.iter().map(|p| std::fs::metadata(p).expect("文件不存在").len()).sum();
    println!("{} 份，共 {:.1} MB\n", paths.len(), total as f64 / (1 << 20) as f64);

    let t = Instant::now();
    let m = MergedLog::open(&paths).expect("打开失败");
    println!("① 打开                {:>9.2} ms", ms(t));

    // 首屏：第一块（6.5 万行）合好就能读，不等整个合并跑完
    let t = Instant::now();
    while m.stat().line_count == 0 {
        std::thread::yield_now();
    }
    let first = ms(t);
    let t2 = Instant::now();
    let head = m.read_block(0, 50);
    println!("② 首屏出现            {first:>9.2} ms   （之后读 50 行 {:.3} ms，{} 字节）", ms(t2), head.len());

    let t = Instant::now();
    m.wait_merged();
    let s = m.stat();
    let merge_ms = ms(t) + first;
    println!("③ 合完                {merge_ms:>9.2} ms   （{} 行，{:.2} GB/s）", s.line_count, total as f64 / (1u64 << 30) as f64 / (merge_ms / 1000.0));
    println!(
        "④ 合并索引内存        {:>9.2} MB   （每行 {:.2} 字节；若每行存 (文件, 行号) 两个 u64 是 {:.1} MB）",
        s.index_bytes as f64 / (1 << 20) as f64,
        s.index_bytes as f64 / s.line_count as f64,
        (s.line_count * 16) as f64 / (1 << 20) as f64
    );

    // 随机定位：最坏要走一个检查点间隔（1023 行）
    let n = s.line_count;
    let picks: Vec<u64> = (0..1000u64).map(|i| (i.wrapping_mul(2_654_435_761) ^ 0x9E37) % n).collect();
    let t = Instant::now();
    for &r in &picks {
        std::hint::black_box(m.read_block(r, 1));
    }
    println!("⑤ 随机取一行          {:>9.4} ms / 次（1000 次平均）", ms(t) / 1000.0);
    let t = Instant::now();
    for _ in 0..20 {
        std::hint::black_box(m.read_block(n / 2, 1000));
    }
    println!("   连续取 1000 行      {:>9.3} ms / 次", ms(t) / 20.0);

    let t = Instant::now();
    let row = m.seek_time("03:30", 0);
    println!("⑥ 按时间跳 03:30      {:>9.3} ms   → 第 {:?} 行", ms(t), row);

    let spec = FilterSpec { levels: LevelMask::ALL, text: TextFilter::single(b"IllegalStateException".to_vec()), case_sensitive: false, collapse_stacks: false };
    let t = Instant::now();
    let task = m.start_filter(spec, u32::MAX).unwrap();
    while !task.is_complete() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    println!("⑦ 过滤一个异常名      {:>9.2} ms   → {} 条", ms(t), task.hit_count());
    let only = FilterSpec { levels: LevelMask::ALL, text: TextFilter::default(), case_sensitive: false, collapse_stacks: false };
    let t = Instant::now();
    let task = m.start_filter(only, 0b1).unwrap();
    while !task.is_complete() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    println!("   只看第 1 份         {:>9.2} ms   → {} 条", ms(t), task.hit_count());
}
