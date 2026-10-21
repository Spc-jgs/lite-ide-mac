//! 拿一个真 JVM 核对读出来的数（和 `jstat -gc <pid>` 对着看）：`cargo run -p jvmstat --example probe -- <pid> [名字里带的字]`
fn main() {
    let mut args = std::env::args().skip(1);
    let pid: u32 = args.next().and_then(|a| a.parse().ok()).expect("给一个 JVM 的 pid");
    let t = std::time::Instant::now();
    let h = jvmstat::read(&std::env::temp_dir(), pid);
    println!("{h:?}  （读一次 {:?}）", t.elapsed());
    if let Some(pat) = args.next() {
        let f = jvmstat::perf_file(&std::env::temp_dir(), pid).expect("没有计数文件");
        let mut all: Vec<_> = jvmstat::parse(&std::fs::read(f).unwrap()).unwrap().longs.into_iter().filter(|(k, _)| k.contains(&pat)).collect();
        all.sort();
        for (k, v) in all {
            println!("{k} = {v}");
        }
    }
}
