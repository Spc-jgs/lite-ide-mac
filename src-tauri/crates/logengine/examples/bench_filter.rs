//! M1 过滤性能验证。
//!
//!   cargo run -p logengine --release --example bench_filter -- <日志路径>

use logengine::{FilterSpec, Level, LevelMask, LogFile};
use std::time::Instant;

fn wait(task: &logengine::FilterTask) -> (u64, f64) {
    let t = Instant::now();
    while !task.is_complete() {
        std::thread::yield_now();
    }
    (task.hits().len() as u64, t.elapsed().as_secs_f64() * 1000.0)
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or("/tmp/big.log".into());
    let size = std::fs::metadata(&path).expect("文件不存在").len();
    let f = LogFile::open(&path).expect("打开失败");
    f.wait_indexed();
    f.wait_levels();
    let s = f.stat();
    println!(
        "文件 {:.2} GB / {} 行，索引与级别扫描已就绪\n",
        size as f64 / (1u64 << 30) as f64,
        s.line_count
    );

    let only = |l: Level| LevelMask::from_bits(1 << l.index());

    println!("{:<34} {:>10} {:>12}", "过滤条件", "耗时", "命中");
    println!("{}", "─".repeat(58));

    let cases: Vec<(&str, FilterSpec)> = vec![
        (
            "仅 ERROR（纯级别）",
            FilterSpec {
                levels: only(Level::Error),
                pattern: String::new(),
                case_sensitive: false,
                collapse_stacks: false,
            },
        ),
        (
            "仅 INFO（大头，考验分配）",
            FilterSpec {
                levels: only(Level::Info),
                pattern: String::new(),
                case_sensitive: false,
                collapse_stacks: false,
            },
        ),
        (
            "文本 OrderService（敏感）",
            FilterSpec {
                levels: LevelMask::ALL,
                pattern: "OrderService".into(),
                case_sensitive: true,
                collapse_stacks: false,
            },
        ),
        (
            "文本 orderservice（不敏感）",
            FilterSpec {
                levels: LevelMask::ALL,
                pattern: "orderservice".into(),
                case_sensitive: false,
                collapse_stacks: false,
            },
        ),
        (
            "ERROR + 文本 Deadlock",
            FilterSpec {
                levels: only(Level::Error),
                pattern: "Deadlock".into(),
                case_sensitive: false,
                collapse_stacks: false,
            },
        ),
        (
            "折叠堆栈（只留每段第一帧）",
            FilterSpec {
                levels: LevelMask::ALL,
                pattern: String::new(),
                case_sensitive: false,
                collapse_stacks: true,
            },
        ),
        (
            "折叠 + 只看 ERROR",
            FilterSpec {
                levels: only(Level::Error),
                pattern: String::new(),
                case_sensitive: false,
                collapse_stacks: true,
            },
        ),
        (
            "文本 无此内容（全扫不中）",
            FilterSpec {
                levels: LevelMask::ALL,
                pattern: "zzz-not-present".into(),
                case_sensitive: false,
                collapse_stacks: false,
            },
        ),
    ];

    for (name, spec) in cases {
        let task = f.start_filter(spec).expect("启动失败");
        let (hits, ms) = wait(&task);
        println!("{name:<34} {ms:>8.1} ms {hits:>12}");
    }

    println!("\n60fps 单帧预算 16ms；过滤是后台任务，界面全程可滚动。");
}
