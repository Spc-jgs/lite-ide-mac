//! M0 出口标准的引擎侧验证。
//!
//!   cargo run -p logengine --release --example bench -- <日志路径>

use logengine::LogFile;
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).unwrap_or("/tmp/big.log".into());
    let size = std::fs::metadata(&path).expect("文件不存在").len();
    println!("文件：{path}");
    println!("大小：{:.2} GB\n", size as f64 / (1u64 << 30) as f64);

    // ① 打开：mmap 是 O(1)，只同步扫 1MB 首块
    let t = Instant::now();
    let f = LogFile::open(&path).expect("打开失败");
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    let first = f.stat();
    println!("① 打开耗时          {open_ms:>8.2} ms   （目标 < 1000ms）");
    println!(
        "   首屏可见行数      {:>8}      （不等全量索引）",
        first.line_count
    );

    // ② 首屏立刻能读 —— 这是「秒开」的真正含义
    let t = Instant::now();
    let head = f.read_block(0, 50);
    println!(
        "② 首屏 50 行读取    {:>8.3} ms   （{} 字节）",
        t.elapsed().as_secs_f64() * 1000.0,
        head.len()
    );

    // ③ 全量索引
    let t = Instant::now();
    f.wait_indexed();
    let idx_ms = t.elapsed().as_secs_f64() * 1000.0;
    let s = f.stat();
    println!("\n③ 全量索引耗时      {idx_ms:>8.2} ms");
    println!("   总行数            {:>8}", s.line_count);
    println!(
        "   索引吞吐          {:>8.2} GB/s",
        (size as f64 / (1u64 << 30) as f64) / (idx_ms / 1000.0)
    );

    // ④ 索引内存 —— 「与文件大小无关」的兑现证据
    let full = s.line_count * 8;
    // 级别扫描是与索引并行的第二个后台任务
    let t = Instant::now();
    f.wait_levels();
    let lvl_ms = t.elapsed().as_secs_f64() * 1000.0;
    let s2 = f.stat();
    let lv = s2.levels;
    println!("\n③b 级别扫描（并行）  {lvl_ms:>8.2} ms   （不阻塞索引与首屏）");
    println!(
        "   级别分布          ERROR {}  WARN {}  INFO {}  DEBUG {}  其他 {}",
        lv.get(logengine::Level::Error),
        lv.get(logengine::Level::Warn),
        lv.get(logengine::Level::Info),
        lv.get(logengine::Level::Debug),
        lv.get(logengine::Level::None)
    );
    println!(
        "   级别表内存        {:>8.1} MB   （每行 4 bit）",
        f.levels().memory_footprint() as f64 / 1048576.0
    );

    println!(
        "\n④ 稀疏索引占用      {:>8.1} KB   （全量偏移需 {:.1} MB，省 {:.0}×）",
        s.index_bytes as f64 / 1024.0,
        full as f64 / (1024.0 * 1024.0),
        full as f64 / s.index_bytes as f64
    );

    // ⑤ 随机定位：滚动条乱拖的场景
    let t = Instant::now();
    let n = 2000;
    let mut sink = 0usize;
    for i in 0..n {
        // 便宜的散布，覆盖全文件
        let line = (i as u64 * 7_919_137) % s.line_count;
        sink += f.read_block(line, 1).len();
    }
    let per = t.elapsed().as_secs_f64() * 1_000_000.0 / n as f64;
    println!("\n⑤ 随机定位单行      {per:>8.2} μs   （{n} 次平均，校验和 {sink}）");

    // ⑥ 顺序块读取：连续滚动的主路径
    let t = Instant::now();
    let blocks = 400;
    let mut bytes = 0usize;
    for i in 0..blocks {
        bytes += f.read_block(i * 512, 512).len();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "⑥ 顺序读 {} 行     {:>8.2} ms   （{:.2} ms/块，60fps 单帧预算 16ms）",
        blocks * 512,
        ms,
        ms / blocks as f64
    );
    println!(
        "   吞吐              {:>8.1} MB/s",
        bytes as f64 / 1048576.0 / (ms / 1000.0)
    );
}
