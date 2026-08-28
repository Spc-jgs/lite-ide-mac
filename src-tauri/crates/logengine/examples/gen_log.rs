//! 造测试日志：贴近真实 Java 应用的输出形态，用于验证 M0 的性能出口标准。
//!
//! 用法：
//!   cargo run -p logengine --release --example gen_log -- <输出路径> <字节数>
//!   cargo run -p logengine --release --example gen_log -- /tmp/big.log 1073741824

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

const CLASSES: [&str; 8] = [
    "c.l.OrderService",
    "c.l.CacheManager",
    "c.l.RetryPolicy",
    "c.l.PaymentGateway",
    "c.l.InventoryLock",
    "o.s.web.DispatcherServlet",
    "c.l.KafkaConsumer",
    "c.l.UserRepository",
];

const THREADS: [&str; 5] = [
    "http-nio-exec-1",
    "http-nio-exec-4",
    "pool-3-thread-2",
    "scheduler-1",
    "kafka-listener-0",
];

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).map(String::as_str).unwrap_or("/tmp/big.log");
    let target: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1 << 30);

    let f = File::create(path).expect("创建文件失败");
    let mut w = BufWriter::with_capacity(1 << 20, f);

    let mut written: u64 = 0;
    let mut n: u64 = 0;
    // 便宜的伪随机：不需要统计学质量，只要分布不规律
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    while written < target {
        let r = next();
        // 级别分布贴近真实：INFO 最多，ERROR 少而扎眼
        let (lvl, msg) = match r % 100 {
            0..=4 => (
                "ERROR",
                "订单落库失败 orderId=%ID% cause=DeadlockLoserDataAccessException",
            ),
            5..=14 => (
                "WARN ",
                "重试 attempt=2/5 backing off 800ms cause=Read timeout",
            ),
            15..=44 => ("DEBUG", "evict key=order:%ID% ttl=300s"),
            _ => ("INFO ", "处理完成 orderId=%ID% cost=%MS%ms status=SUCCESS"),
        };
        let secs = n / 40;
        let line = format!(
            "2026-08-{:02} {:02}:{:02}:{:02}.{:03} {} [{}] {} - {}\n",
            20 + (secs / 86400) % 8,
            (secs / 3600) % 24,
            (secs / 60) % 60,
            secs % 60,
            n % 1000,
            lvl,
            THREADS[(r >> 8) as usize % THREADS.len()],
            CLASSES[(r >> 16) as usize % CLASSES.len()],
            msg.replace("%ID%", &(8_000_000 + n % 900_000).to_string())
                .replace("%MS%", &(r % 400).to_string()),
        );
        w.write_all(line.as_bytes()).expect("写入失败");
        written += line.len() as u64;
        n += 1;

        // 每约 5 万行插一段异常堆栈 —— 这是日志查看器真正的压力来源
        if n.is_multiple_of(50_000) {
            let stack = "java.lang.IllegalStateException: connection pool exhausted\n\
                \tat com.zaxxer.hikari.pool.HikariPool.createTimeoutException(HikariPool.java:696)\n\
                \tat com.zaxxer.hikari.pool.HikariPool.getConnection(HikariPool.java:197)\n\
                \tat com.liteide.OrderService.persist(OrderService.java:142)\n\
                \tat java.base/java.lang.Thread.run(Thread.java:840)\n";
            w.write_all(stack.as_bytes()).expect("写入失败");
            written += stack.len() as u64;
            n += 5;
        }
    }
    w.flush().expect("flush 失败");
    println!("已生成 {path}：{written} 字节 / {n} 行");
}
