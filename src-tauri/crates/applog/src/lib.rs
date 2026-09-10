//! 应用自己的运行日志。
//!
//! # 为什么要有
//!
//! 在这之前，一个装好的 `.app` 出了问题**不留任何痕迹**：
//!
//! | 出了什么事 | 去哪了 |
//! |---|---|
//! | 前端 `window.onerror` / `unhandledrejection` | `diag` → stderr，而 `diag` 默认闭嘴 |
//! | Rust 侧 panic | 默认 hook → stderr |
//! | IPC 命令报错 | 变成界面上一句话，说完就没了 |
//!
//! 而双击启动的 `.app` **没有 stderr** —— 那几行字写给谁都不知道。
//! 于是「用户说昨天崩了一次」这件事，除了让他再复现一遍，没有第二条路。
//!
//! `LITE_IDE_DEBUG=1` 挡在前面这一点尤其要命：**默认关掉的可观测性等于没有**。
//! 真出事的那一次，人不会正好带着那个环境变量在跑。所以这条通道**默认开**。
//!
//! # 为什么不是遥测
//!
//! 立项时就排除了遥测（PLAN.md）。这里一个字节都不出这台机器：写到
//! `~/Library/Logs/com.liteide.app/`，和别的 macOS 应用放在一起，用户自己能看、
//! 能删、能拖给别人。
//!
//! 而这个应用**自己就是个日志查看器** —— 菜单里「打开应用日志」
//! 就是用它自己的 mmap 引擎打开这个文件，级别过滤和 tail 全是现成的。
//! 这是别的项目做不到的一件事，成本只剩「把字写下去」这一半。
//!
//! # 保留策略（写死，改这几个数就要改 UNINSTALL.md）
//!
//! | | |
//! |---|---|
//! | 盘上放几份 | **2** —— `app.log` 和轮转出去的 `app.log.1` |
//! | 每份多大 | **2MB**（[`MAX_BYTES`]），满了就轮转 |
//! | 合计上限 | **4MB，永远不会再长** |
//! | 按时间删吗 | **不** —— 见下 |
//! | 写什么进去 | **只写异常**，不写执行轨迹 |
//! | 出得了这台机器吗 | **不**。没有网络代码，没有上报，没有遥测 |
//! | 用户怎么清 | 菜单「帮助 → 清空应用日志」，或者直接删文件 |
//!
//! **为什么不按天数删。** 按大小封顶已经把「无限长大」这个真问题解决了，
//! 再加一条按天数的规则要多一个「上次清理是什么时候」的状态，而它换来的
//! 只是「把很久以前的错误提前删掉」—— 那恰恰是最值钱的那几行
//! （「上个月崩过一次，当时是什么样」）。**留着不花钱，删掉可能花钱。**
//!
//! **为什么只写异常。** 把执行轨迹也塞进去的话，2MB 撑不过一小时，
//! 真正的错误会被一堆「打开了文件」冲走 —— 那时这个文件就从证据
//! 变成了噪音。执行轨迹归 `diag`（stderr，默认关）。
//!
//! **直接删文件是安全的。** 下次启动 `install` 会重新建；应用正开着的时候
//! 删掉它，写入会静默失败（句柄还指着一个没有名字的 inode），下次启动恢复正常 ——
//! 不会崩，也不会把别的东西弄坏。
//!
//! # 三条硬约束
//!
//! 1. **绝不 panic，绝不往上抛。** 这段代码跑在出错的路径上。
//!    一个会把应用带崩的日志器，比没有日志器糟得多。所有 IO 错误就地吞掉。
//! 2. **有上限。** 一个只涨不缩的文件迟早变成用户磁盘上的问题。
//!    两文件轮转，总量封顶在 `2 × MAX_BYTES`。
//! 3. **一行一条，时间戳在最前。** 这是给自己的日志引擎读的 ——
//!    `logengine` 的级别探测认的就是 `INFO` / `WARN` / `ERROR` 这几个词。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 单个文件的上限。超了就轮转，盘上最多两份，合计 4MB。
///
/// 2MB 大约是几万行 —— 足够覆盖「上周崩的那次」，又不至于让
/// 「打开应用日志」变成一次要等的操作。
pub const MAX_BYTES: u64 = 2 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }

    /// 从前端传过来的字符串认级别。**认不出来一律当 INFO**，不报错 ——
    /// 前端传了个没见过的词，不该让这条日志整条丢掉。
    pub fn parse(s: &str) -> Level {
        match s.to_ascii_uppercase().as_str() {
            "ERROR" | "FATAL" => Level::Error,
            "WARN" | "WARNING" => Level::Warn,
            _ => Level::Info,
        }
    }
}

/// 装好之后的落点。没装（测试、未初始化）时所有写入都是空操作。
static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

struct Sink {
    path: PathBuf,
    /// 当前文件已经写了多少。**自己记着，不每次去 `metadata()`** ——
    /// 每写一行 stat 一次，是把一次系统调用变成两次，而这条路上
    /// 可能正在刷一串错误。
    written: u64,
    file: Option<File>,
}

/// 日志文件的路径。`dir` 由调用方给（Tauri 那边解析 `~/Library/Logs/<产品名>`）。
pub fn log_path(dir: impl AsRef<Path>) -> PathBuf {
    dir.as_ref().join("app.log")
}

/// 轮转出去的那一份。只留一代 —— 两代以上的日志，人不会去看第三份。
fn rolled_path(dir: impl AsRef<Path>) -> PathBuf {
    dir.as_ref().join("app.log.1")
}

/// 装上日志。**重复调用只有第一次算数**，返回日志文件的路径。
///
/// 装不上（目录建不了、文件开不了）也返回路径，只是之后每次写入都会是空操作 ——
/// 「日志装不上」不该变成一条阻止应用启动的错误。
pub fn install(dir: impl AsRef<Path>) -> PathBuf {
    let dir = dir.as_ref();
    let path = log_path(dir);
    SINK.get_or_init(|| {
        let _ = fs::create_dir_all(dir);
        let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        Mutex::new(Sink {
            path: path.clone(),
            written,
            file,
        })
    });
    path
}

/// 把日志清空。用户要求的那次「删掉」。
///
/// # 这是应用里第二条真删除，所以它也自己校验
///
/// 第一条是 `fsservice::discard_empty_scratch`。判据同样写在这儿、
/// 不在调用方：**前端少一个把东西删到别处去的机会**。
///
/// 只碰两个**名字写死**的文件（`app.log` / `app.log.1`），而且只在
/// 调用方给的目录下 —— 名字不是参数，所以没有「传进来一个别的路径」这条路。
///
/// `app.log` 是**截断**不是删除：句柄还开着，删掉名字之后写入会落进一个
/// 没人找得到的 inode，看起来就是「清空之后再也不记日志了」。
/// `app.log.1` 没有句柄，直接删。
///
/// 清完写一行「已清空」——**空文件和「日志坏了」在界面上长得一模一样**，
/// 留一行下来，人才知道刚才那下是生效了。
pub fn clear() {
    let Some(lock) = SINK.get() else { return };
    let mut sink = match lock.lock() {
        Ok(s) => s,
        Err(p) => p.into_inner(),
    };
    let dir = sink.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let _ = fs::remove_file(rolled_path(&dir));
    sink.file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&sink.path)
        .ok();
    sink.written = 0;
    let line = format_line(now_ms(), Level::Info, "app", "日志已清空");
    sink.append(&line);
}

/// 写一行。**任何情况下都不会 panic，也不会返回错误。**
pub fn write(level: Level, source: &str, msg: &str) {
    let Some(lock) = SINK.get() else { return };
    // 锁中毒（别的线程在持锁时 panic 了）时照写不误：这时候**尤其**需要日志。
    let mut sink = match lock.lock() {
        Ok(s) => s,
        Err(poisoned) => poisoned.into_inner(),
    };
    let line = format_line(now_ms(), level, source, msg);
    sink.append(&line);
}

/// 拼一行。拎出来是为了能测 —— 格式是给 `logengine` 的级别探测读的，
/// 改了它等于改了「打开应用日志之后能不能按级别过滤」。
pub fn format_line(ms: u64, level: Level, source: &str, msg: &str) -> String {
    // 消息里的换行要压平：一行一条是这个格式唯一的约定，
    // 一段 JS 调用栈原样写下去会变成二十条「没有级别」的行。
    let flat = msg.replace('\n', " ⏎ ").replace('\r', "");
    format!("{} {} [{}] {}\n", stamp(ms), level.as_str(), source, flat)
}

impl Sink {
    fn append(&mut self, line: &str) {
        if self.file.is_none() {
            return;
        }
        if self.written + line.len() as u64 > MAX_BYTES {
            self.roll();
        }
        if let Some(f) = self.file.as_mut() {
            // 写失败就当没这回事。盘满、权限没了、文件被人删了 ——
            // 每一种的正确反应都是「应用继续跑」。
            if f.write_all(line.as_bytes()).is_ok() {
                self.written += line.len() as u64;
            }
        }
    }

    /// 轮转：当前这份改名成 `.1`（盖掉上一代），重新开一个空的。
    ///
    /// **要紧的是 rename 之后必须重新 `open`。** Unix 上改一个开着的文件的名字
    /// 是允许的，旧句柄跟着 inode 一起走 —— 不重开的话，之后写的每一个字
    /// 都落进 `app.log.1`，而 `app.log` 停在 0 字节。表现是「日志忽然不写了」。
    ///
    /// （写过一句「先把句柄放掉再 rename」，验红时发现**去掉那一步测试照样绿**：
    /// 下面的赋值本来就会把旧句柄 drop 掉。那句话是错的，删了。
    /// 留个记录 —— 「顺手加的防御性代码」和「真的在防什么」得分开。）
    fn roll(&mut self) {
        let _ = fs::rename(&self.path, rolled_path(self.path.parent().unwrap_or(Path::new("."))));
        self.file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)
            .ok();
        // 少了这句，`written` 停在上限之上，之后**每写一行都轮转一次** ——
        // 盘上永远只剩最后一两条
        self.written = 0;
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `2026-09-10 14:03:26.181` —— 和这个应用天天看的 Java / Logback 日志同一个形状。
///
/// **自己算，不引 chrono / time。** 这个 crate 在出错路径上跑，
/// 而这里要的只是「UTC 的年月日时分秒」。用的是民用历的标准算法
/// （Howard Hinnant 的 `civil_from_days`），闰年闰世纪都对。
///
/// 代价说清楚：**是 UTC，不是本地时间**，末尾的 `Z` 就是在说这件事。
/// 要本地时区就得知道 tzdata，那是一整个依赖 —— 而这份日志的用途是
/// 「按顺序看发生了什么」和「和别的时间戳对得上」，UTC 两件都做得到。
pub fn stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let sub = ms % 1000;
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{sub:03}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// 天数（1970-01-01 为 0）→ 年月日。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]，从 3 月起算
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 时间戳按民用历算得对() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00.000Z");
        // 1789034606181 = 2026-09-10 10:03:26.181 **UTC**（拿本机时区读出来的
        // 是 14:03，第一版就是这么写错的 —— 这行注释留着，它正是这个函数
        // 唯一容易被误解的地方）
        assert_eq!(stamp(1_789_034_606_181), "2026-09-10 10:03:26.181Z");
        // 闰日：2024 是闰年
        assert_eq!(&stamp(1_709_164_800_000)[..10], "2024-02-29");
        // 闰世纪：2000 是闰年（能被 400 整除），1900 不是
        assert_eq!(&stamp(951_782_400_000)[..10], "2000-02-29");
        // 毫秒必须补零，否则 .1 和 .100 排序会乱
        assert_eq!(&stamp(1_789_034_606_001)[20..], "001Z");
    }

    #[test]
    fn 一条日志是一行_多行消息要压平() {
        let l = format_line(0, Level::Error, "web", "崩了\nat foo()\nat bar()");
        assert_eq!(l.matches('\n').count(), 1, "一条日志只能有一个换行，就是结尾那个");
        assert!(l.contains("⏎"), "原来的换行要留个记号，不能直接拼没了");
        assert!(l.starts_with("1970-01-01 00:00:00.000Z ERROR [web] "));
    }

    #[test]
    fn 级别认得出也兜得住() {
        assert_eq!(Level::parse("error"), Level::Error);
        assert_eq!(Level::parse("WARNING"), Level::Warn);
        assert_eq!(Level::parse("info"), Level::Info);
        // 认不出来不能丢，当 INFO
        assert_eq!(Level::parse("啥玩意"), Level::Info);
        assert_eq!(Level::parse(""), Level::Info);
    }

    /*
     * 轮转要真的**换文件**，不是只改个名。
     *
     * 这条测的是 `roll()` 里「先把句柄放掉」那一步：不放的话 rename 照样成功
     * （Unix 允许改一个开着的文件的名字），而之后写的字全落进 `app.log.1`
     * 那个 inode —— `app.log` 停在 0 字节，看着像日志忽然不写了。
     * 直接测 `Sink` 而不是全局 `SINK`：那是个 `OnceLock`，一个进程里只能装一次。
     */
    #[test]
    fn 轮转之后新的字要落进新文件() {
        let dir = std::env::temp_dir().join(format!("applog-roll-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = log_path(&dir);
        let mut sink = Sink {
            path: path.clone(),
            written: 0,
            file: Some(OpenOptions::new().create(true).append(true).open(&path).unwrap()),
        };

        // 先写到接近上限
        let 一行 = "x".repeat(1024) + "\n";
        while sink.written + 一行.len() as u64 <= MAX_BYTES {
            sink.append(&一行);
        }
        let 满了 = fs::metadata(&path).unwrap().len();
        assert!(满了 > MAX_BYTES - 2048, "没写到上限附近，这条测试什么都没测到");

        sink.append("轮转之后的第一行\n");
        assert_eq!(
            fs::metadata(rolled_path(&dir)).unwrap().len(),
            满了,
            "上一代没被完整搬到 app.log.1"
        );
        let 新的 = fs::read_to_string(&path).unwrap();
        assert_eq!(新的, "轮转之后的第一行\n", "轮转之后的字没落进新文件（实得 {} 字节）", 新的.len());
        assert!(fs::metadata(&path).unwrap().len() < MAX_BYTES);

        /*
         * **轮转之后计数要归零，否则下一行又轮转一次。**
         * 只断言上面那一条的话，`self.written = 0` 删掉测试照样绿（验过）——
         * 因为那时只写了一行，看不出「每写一行滚一次」。再写一行就露馅了：
         * 归零了的话两行都在，没归零的话第一行已经被滚进 `.1`。
         */
        sink.append("紧接着的第二行\n");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "轮转之后的第一行\n紧接着的第二行\n",
            "轮转之后又滚了一次 —— written 没归零"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// 没装的时候写日志必须是彻底的空操作 —— 单测和 CLI 路径上都不该有落点
    #[test]
    fn 没装的时候写入不做任何事() {
        write(Level::Error, "test", "这条不该落到任何地方");
        // 清空同理：没装的时候它不该去动盘上任何东西
        clear();
    }

    /*
     * 清空要**两份都清掉**，而且清完 `app.log` 还得能继续写。
     *
     * 直接 `remove_file(app.log)` 是不行的：句柄还开着，之后写的字会落进
     * 一个没有名字的 inode，表现是「清空之后日志再也不更新了」，
     * 而且要到下次重启才自愈。这条测的就是那个 —— 走 `Sink` 自己那两步
     * （truncate 重开 + written 归零），和 `clear()` 里的一模一样。
     */
    #[test]
    fn 清空之后两份都没了且还能继续写() {
        let dir = std::env::temp_dir().join(format!("applog-clear-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = log_path(&dir);
        fs::write(rolled_path(&dir), "上一代").unwrap();
        let mut sink = Sink {
            path: path.clone(),
            written: 0,
            file: Some(OpenOptions::new().create(true).append(true).open(&path).unwrap()),
        };
        sink.append("清空之前的一行\n");

        // clear() 的那三步（它自己要 SINK，一个进程只能装一次，所以这里手动走）
        let _ = fs::remove_file(rolled_path(&dir));
        sink.file = OpenOptions::new().create(true).truncate(true).write(true).open(&path).ok();
        sink.written = 0;
        sink.append("清空之后的一行\n");

        assert!(!rolled_path(&dir).exists(), "上一代没删掉");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "清空之后的一行\n",
            "清空之后要么旧内容还在，要么新的字没落进来"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
