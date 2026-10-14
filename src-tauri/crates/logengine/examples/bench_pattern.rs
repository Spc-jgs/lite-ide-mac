//! 日志归类（`pattern.rs`）的速度和结果：几份日志一起归，打印前几类。
//!
//!   cargo run -p logengine --release --example bench_pattern -- /tmp/a.log /tmp/b.log

use logengine::{pattern, LogFile};
use std::time::Instant;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<LogFile> = paths.iter().map(|p| LogFile::open(p).expect("打开失败")).collect();
    let maps = files.iter().map(|f| f.mapped()).collect();
    let t = Instant::now();
    let task = pattern::spawn(maps).unwrap();
    task.wait();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let s = task.stat();
    println!(
        "{:.1} MB 归类 {ms:.1} ms（{:.2} GB/s）：{} 行记录头 → {} 类，没归进去 {} 行",
        s.total_bytes as f64 / (1 << 20) as f64,
        s.total_bytes as f64 / (1u64 << 30) as f64 / (ms / 1000.0),
        s.lines,
        s.patterns,
        s.overflow
    );
    for p in s.top.iter().take(8) {
        println!("{:>9}  {}", p.count, String::from_utf8_lossy(&p.template));
    }
}
