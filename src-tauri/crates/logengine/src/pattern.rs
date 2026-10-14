//! 日志归类（2026-10-14，docs/PATTERNS.md）：一百万行归成几十类，「这一类出现了 3,214 次」。
//!
//! 打开一份从服务器拉下来的日志，第一个问题往往不是「第几行写了什么」，而是**「这里面都发生了些什么、各多少」**。
//! 级别计数只说了「有 4 千条 ERROR」，说不出它们是一种错还是四十种。
//!
//! # 算法：Drain（He 等，ICWS 2017），按这个应用的需要改了两处
//!
//! 一行日志 = 固定的模板 + 变量（`用户 <*> 登录，来自 <*>`）。Drain 一行一行地在线归类，每行近似 O(1)：
//!
//! 1. **切词、把变量换成 `<*>`**。原版把「带数字的整个词」换掉；这里按非字母数字再切一层，只换带数字的那一段 ——
//!    `userId=1024` 归一成 `userId=<*>`、`[http-nio-8080-exec-2]` 归一成 `[http-nio-<*>-exec-<*>]`。
//!    `userId=` 这类常量留下来，后面拿模板当过滤条件时才筛得准（见 `templateQuery`，前端）。
//! 2. **按「词数 + 前两个常量词」分桶**（Drain 的前缀树压平成一个 HashMap）。词数不同的行不可能是同一个模板；
//!    前两个常量词通常是级别和类名。时间戳、线程号这些已经是 `<*>`，跳过它们取常量词，不然每一行的前缀都不一样。
//! 3. **桶里挑最像的那一类**：相似度 = 这一行的常量词里，有多少在模板的同一位置上一模一样。**过半就归进去**，
//!    模板里对不上的位置变成 `<*>`；没有过半的就自立一类。
//!
//! 原版的相似度是「一样的词数 / 总词数」，模板里的 `<*>` 不算一样 —— 一行里时间戳、线程号、id 占了一半，
//! 相似度就永远过不了半，每一行自成一类。这里分母只数这一行的**常量词**：变量本来就该不一样。
//!
//! # 归哪些行
//!
//! 带时间戳的文件只归**有时间戳的行**（一条记录的头）—— 堆栈帧、多行 SQL 是那条记录的附属，自己成类只会淹掉真正的类。
//! 没有时间戳的文件归所有非空行，跳过堆栈帧。
//!
//! # 有上限，而且说出来
//!
//! 最多 [`MAX_PATTERNS`] 类：乱到这个地步的日志，再多分也没用了；超出的行记进 `overflow`，界面上照实说「还有 N 行没归进去」。
//! 一行最多看前 [`MAX_TOKENS`] 个词（一整行 JSON 能有上千个），后面的折成一个 `…`。

use crate::level::{self, Level};
use crate::{filter, merge, seek};
use memmap2::Mmap;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 最多分几类
pub const MAX_PATTERNS: usize = 20_000;
/// 一行最多看前几个词
const MAX_TOKENS: usize = 40;
/// 快照里留多少类（按次数从多到少）。界面一次画不了两万行，也没人会往下翻到第一千类
const KEEP_TOP: usize = 1000;
/// 每归多少行发布一次快照：大文件上边归边看，前几十万行的类和最后的结果差不多
const PUBLISH_EVERY: u64 = 1 << 18;
const WILD: &[u8] = b"<*>";
/// 词数超过 MAX_TOKENS 时，后面的折成这一个词
const MORE: &[u8] = "…".as_bytes();

#[inline]
fn alnum(c: u8) -> bool {
    // 非 ASCII 的字节（中文、GBK）都算「字」：「下单」是一个常量词，不是一串标点
    c.is_ascii_alphanumeric() || c >= 0x80
}

/// 一个词归一化：按非字母数字切段，带数字的那段换成 `<*>`，写进 `out`
fn normalize(tok: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < tok.len() {
        if !alnum(tok[i]) {
            out.push(tok[i]);
            i += 1;
            continue;
        }
        let start = i;
        let mut digit = false;
        while i < tok.len() && alnum(tok[i]) {
            digit |= tok[i].is_ascii_digit();
            i += 1;
        }
        out.extend_from_slice(if digit { WILD } else { &tok[start..i] });
    }
}

/// 一类：模板（`None` 是变量位置）、出现次数、各级别的次数
struct Cluster {
    tpl: Vec<Option<Box<[u8]>>>,
    count: u64,
    /// 对比时「基准那边」（昨天的、上线前的）出现了几次；不对比时恒为 0
    base: u64,
    levels: [u64; Level::COUNT],
}

/// 对外的一类
#[derive(Debug, Clone)]
pub struct Pattern {
    /// 模板原样的字节，变量位置是 `<*>`（不在这层转码：文件可能是 GBK，同 `block.rs`）
    pub template: Vec<u8>,
    pub count: u64,
    /// 对比时基准那边出现了几次（见 [`spawn_compare`]）；不对比时恒为 0
    pub base: u64,
    /// 各级别各几行（两边加起来），顺序同 [`Level`]
    pub levels: [u64; Level::COUNT],
}

struct Drain {
    /// 桶：「词数 + 前两个常量词」→ 这个桶里的类
    leaves: HashMap<Box<[u8]>, Vec<u32>>,
    clusters: Vec<Cluster>,
    max: usize,
    lines: u64,
    /// 基准那边归了多少行（对比时）
    base_lines: u64,
    overflow: u64,
    // 下面三个每行复用，不每行分配
    norm: Vec<u8>,
    spans: Vec<(usize, usize)>,
    key: Vec<u8>,
}

impl Drain {
    fn new(max: usize) -> Self {
        Drain { leaves: HashMap::new(), clusters: Vec::new(), max, lines: 0, base_lines: 0, overflow: 0, norm: Vec::new(), spans: Vec::new(), key: Vec::new() }
    }

    /// 归一行。`base` 是这一行来自对比的基准那边
    fn add(&mut self, line: &[u8], lvl: Level, base: bool) {
        self.norm.clear();
        self.spans.clear();
        let mut n = 0;
        for tok in line.split(|c| c.is_ascii_whitespace()).filter(|t| !t.is_empty()) {
            if n == MAX_TOKENS {
                let s = self.norm.len();
                self.norm.extend_from_slice(MORE);
                self.spans.push((s, self.norm.len()));
                break;
            }
            let s = self.norm.len();
            normalize(tok, &mut self.norm);
            self.spans.push((s, self.norm.len()));
            n += 1;
        }
        if self.spans.is_empty() {
            return;
        }
        let norm = &self.norm;
        let toks: Vec<Option<&[u8]>> = self
            .spans
            .iter()
            .map(|&(a, b)| {
                let t = &norm[a..b];
                (t != WILD).then_some(t)
            })
            .collect();

        self.key.clear();
        self.key.push(toks.len() as u8);
        for t in toks.iter().flatten().take(2) {
            self.key.push(0);
            self.key.extend_from_slice(t);
        }

        // 桶里挑最像的：先比「对得上的常量词」，一样多再比「模板里的变量多」（更泛的那个先吃，少分裂）
        let consts = toks.iter().filter(|t| t.is_some()).count();
        let mut best: Option<(usize, usize, usize)> = None; // (类号, 对得上几个, 变量几个)
        if let Some(ids) = self.leaves.get(&self.key[..]) {
            for &id in ids {
                let c = &self.clusters[id as usize];
                let (mut same, mut params) = (0, 0);
                for (t, p) in toks.iter().zip(&c.tpl) {
                    match (t, p) {
                        (_, None) => params += 1,
                        (Some(a), Some(b)) if *a == &b[..] => same += 1,
                        _ => {}
                    }
                }
                if best.is_none_or(|(_, s, p)| (same, params) > (s, p)) {
                    best = Some((id as usize, same, params));
                }
            }
        }
        if base {
            self.base_lines += 1;
        } else {
            self.lines += 1;
        }
        let (cur, old) = if base { (0, 1) } else { (1, 0) };
        let li = lvl.index();
        // 过半：对得上的常量词 ≥ 这一行常量词的一半（一行全是变量的，任何同长度的类都收）
        if let Some((id, same, _)) = best.filter(|&(_, same, _)| same * 2 >= consts) {
            let _ = same;
            let c = &mut self.clusters[id];
            for (p, t) in c.tpl.iter_mut().zip(&toks) {
                if p.as_deref().is_some_and(|p| Some(p) != *t) {
                    *p = None;
                }
            }
            c.count += cur;
            c.base += old;
            c.levels[li] += 1;
            return;
        }
        if self.clusters.len() >= self.max {
            self.overflow += 1;
            return;
        }
        let mut levels = [0; Level::COUNT];
        levels[li] = 1;
        let id = self.clusters.len() as u32;
        self.clusters.push(Cluster { tpl: toks.iter().map(|t| t.map(Box::from)).collect(), count: cur, base: old, levels });
        self.leaves.entry(Box::from(&self.key[..])).or_default().push(id);
    }

    /// 前 `k` 类。不对比时按次数从多到少；对比时按「变化有多大」，见 [`change_rank`]
    fn top(&self, k: usize, compare: bool) -> Vec<Pattern> {
        let mut order: Vec<usize> = (0..self.clusters.len()).collect();
        if compare {
            let keys: Vec<(u8, f64, u64)> = self.clusters.iter().map(|c| change_rank(c.count, c.base, self.lines, self.base_lines)).collect();
            order.sort_unstable_by(|&a, &b| {
                let (ka, kb) = (keys[a], keys[b]);
                ka.0.cmp(&kb.0).then(kb.1.total_cmp(&ka.1)).then(kb.2.cmp(&ka.2)).then(a.cmp(&b))
            });
        } else {
            order.sort_unstable_by(|&a, &b| self.clusters[b].count.cmp(&self.clusters[a].count).then(a.cmp(&b)));
        }
        order
            .into_iter()
            .take(k)
            .map(|i| {
                let c = &self.clusters[i];
                let mut t = Vec::new();
                for (j, p) in c.tpl.iter().enumerate() {
                    if j > 0 {
                        t.push(b' ');
                    }
                    t.extend_from_slice(p.as_deref().unwrap_or(WILD));
                }
                Pattern { template: t, count: c.count, base: c.base, levels: c.levels }
            })
            .collect()
    }
}

/**
 * 对比时一类排在哪：(档, 变化幅度, 次数)，档小的在前、幅度大的在前、次数多的在前。
 *
 * - 第 0 档 **新出现的**（基准那边一次都没有）—— 上线之后多出来的报错，最该先看
 * - 第 1 档 **没了的**（这边一次都没有）—— 定时任务不跑了、某个服务不打日志了，同样要紧
 * - 第 2 档 两边都有：按**频率**比，不按次数比。今天的日志比昨天长一倍，每一类的次数都会翻倍，那不是变化；
 *   频率 = 这一类的次数 / 那边一共归了多少行。幅度取「涨了几倍」和「降到几分之一」里大的那个，
 *   所以 ×8 和 ÷8 一样靠前。次数做最后的比较：同样 ×3，三百条比三条值得看
 */
fn change_rank(count: u64, base: u64, lines: u64, base_lines: u64) -> (u8, f64, u64) {
    if base == 0 {
        return (0, 0.0, count);
    }
    if count == 0 {
        return (1, 0.0, base);
    }
    let now = count as f64 / lines.max(1) as f64;
    let then = base as f64 / base_lines.max(1) as f64;
    let r = now / then;
    (2, r.max(1.0 / r), count + base)
}

/// 一次归类：后台跑、边跑边发布快照、能喊停
pub struct PatternTask {
    top: Mutex<Arc<Vec<Pattern>>>,
    patterns: AtomicU64,
    lines: AtomicU64,
    overflow: AtomicU64,
    scanned: AtomicU64,
    total: u64,
    done: AtomicBool,
    cancel: AtomicBool,
    /// 对比模式（[`spawn_compare`]）：排序按变化，`base_lines` 有意义
    compare: bool,
    base_lines: AtomicU64,
}

/// 归类的进度和结果
#[derive(Debug, Clone)]
pub struct PatternStat {
    /// 次数最多的前若干类（最多 [`KEEP_TOP`]）
    pub top: Arc<Vec<Pattern>>,
    /// 一共分出了几类
    pub patterns: u64,
    /// 归了多少行（带时间戳的文件只算记录头）
    pub lines: u64,
    /// 对比时基准那边归了多少行
    pub base_lines: u64,
    /// 类数到了上限之后没归进去的行
    pub overflow: u64,
    pub scanned_bytes: u64,
    pub total_bytes: u64,
    pub done: bool,
}

impl PatternTask {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub fn stat(&self) -> PatternStat {
        PatternStat {
            top: Arc::clone(&self.top.lock().expect("归类快照锁被毒化")),
            patterns: self.patterns.load(Ordering::Relaxed),
            lines: self.lines.load(Ordering::Relaxed),
            base_lines: self.base_lines.load(Ordering::Relaxed),
            overflow: self.overflow.load(Ordering::Relaxed),
            scanned_bytes: self.scanned.load(Ordering::Relaxed),
            total_bytes: self.total,
            done: self.done.load(Ordering::Acquire),
        }
    }

    pub fn wait(&self) {
        while !self.done.load(Ordering::Acquire) && !self.cancel.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    }

    fn publish(&self, d: &Drain, scanned: u64) {
        *self.top.lock().expect("归类快照锁被毒化") = Arc::new(d.top(KEEP_TOP, self.compare));
        self.patterns.store(d.clusters.len() as u64, Ordering::Relaxed);
        self.lines.store(d.lines, Ordering::Relaxed);
        self.base_lines.store(d.base_lines, Ordering::Relaxed);
        self.overflow.store(d.overflow, Ordering::Relaxed);
        self.scanned.store(scanned, Ordering::Relaxed);
    }
}

/// 在后台把这几份映射归一遍类（单文件就一份；合并视图把每一份都扫一遍 —— 归类和先后无关）
pub fn spawn(maps: Vec<Arc<Mmap>>) -> io::Result<Arc<PatternTask>> {
    start(maps, Vec::new(), false)
}

/**
 * 对比：`cur`（今天的、上线后的）和 `base`（昨天的、上线前的）**放进同一个 Drain 里一起归**，每一类分两边计数。
 *
 * 不是两边各归一遍再按模板文字去配：同一句日志在两次归类里可能长成不一样的模板（这边有个位置变成了 `<*>`、那边没变），
 * 按文字配就配不上，「同一类」被当成「一边新出现、一边没了」。放在一棵树里归，「是不是同一类」只判一次，两边天然对得上
 */
pub fn spawn_compare(cur: Vec<Arc<Mmap>>, base: Vec<Arc<Mmap>>) -> io::Result<Arc<PatternTask>> {
    start(cur, base, true)
}

fn start(cur: Vec<Arc<Mmap>>, base: Vec<Arc<Mmap>>, compare: bool) -> io::Result<Arc<PatternTask>> {
    let task = Arc::new(PatternTask {
        top: Mutex::new(Arc::new(Vec::new())),
        patterns: AtomicU64::new(0),
        lines: AtomicU64::new(0),
        overflow: AtomicU64::new(0),
        scanned: AtomicU64::new(0),
        total: cur.iter().chain(&base).map(|m| m.len() as u64).sum(),
        done: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        compare,
        base_lines: AtomicU64::new(0),
    });
    let t = Arc::clone(&task);
    std::thread::Builder::new().name("logengine-pattern".into()).spawn(move || {
        let cur: Vec<&[u8]> = cur.iter().map(|m| &m[..]).collect();
        let base: Vec<&[u8]> = base.iter().map(|m| &m[..]).collect();
        run(&cur, &base, &t, MAX_PATTERNS);
    })?;
    Ok(task)
}

/// 同步归类。拎成函数同 `lib.rs` 的 `scan_levels`：为了能测（预先置位取消开关、调小上限）。**被取消时不置 done**。
/// 先归基准那边、再归这边：先后只影响模板长成什么样，不影响计数
fn run(maps: &[&[u8]], base_maps: &[&[u8]], task: &PatternTask, max: usize) {
    let mut d = Drain::new(max);
    let mut scanned = 0u64;
    let mut since = 0u64;
    for (data, base) in base_maps.iter().map(|m| (m, true)).chain(maps.iter().map(|m| (m, false))) {
        // 带时间戳的文件只归记录头；没有的归所有非空行、跳过堆栈帧
        let has_ts = merge::first_key(data).is_some();
        let mut pos = 0usize;
        while pos < data.len() {
            let end = memchr::memchr(b'\n', &data[pos..]).map_or(data.len(), |n| pos + n);
            let mut line = &data[pos..end];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            pos = end + 1;
            let head = if has_ts { seek::ts_key(line).is_some() } else { !line.is_empty() && !filter::is_stack_frame(line) };
            if !head {
                continue;
            }
            d.add(line, level::detect(line), base);
            since += 1;
            if since.is_multiple_of(1 << 16) && task.cancel.load(Ordering::Relaxed) {
                return;
            }
            if since.is_multiple_of(PUBLISH_EVERY) {
                task.publish(&d, scanned + pos.min(data.len()) as u64);
            }
        }
        scanned += data.len() as u64;
    }
    task.publish(&d, scanned);
    task.done.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> String {
        let mut out = Vec::new();
        normalize(s.as_bytes(), &mut out);
        String::from_utf8(out).unwrap()
    }

    fn task(total: u64) -> PatternTask {
        PatternTask {
            top: Mutex::new(Arc::new(Vec::new())),
            patterns: AtomicU64::new(0),
            lines: AtomicU64::new(0),
            overflow: AtomicU64::new(0),
            scanned: AtomicU64::new(0),
            total,
            done: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            compare: false,
            base_lines: AtomicU64::new(0),
        }
    }

    /// 归一遍，返回 [(模板, 次数)]，按次数从多到少
    fn classify(body: &str) -> Vec<(String, u64)> {
        let t = task(body.len() as u64);
        run(&[body.as_bytes()], &[], &t, MAX_PATTERNS);
        assert!(t.stat().done);
        t.stat().top.iter().map(|p| (String::from_utf8(p.template.clone()).unwrap(), p.count)).collect()
    }

    #[test]
    fn 归一化_只换带数字的那一段() {
        assert_eq!(norm("userId=1024"), "userId=<*>", "等号前的常量留着，后面拿模板过滤才筛得准");
        assert_eq!(norm("[http-nio-8080-exec-2]"), "[http-nio-<*>-exec-<*>]");
        assert_eq!(norm("7f3a9c"), "<*>", "十六进制的 id 整段是变量");
        assert_eq!(norm("10:15:02.101"), "<*>:<*>:<*>.<*>");
        assert_eq!(norm("ERROR"), "ERROR");
        assert_eq!(norm("第2次"), "<*>", "中文和数字连着算一段");
        assert_eq!(norm("下单"), "下单");
    }

    #[test]
    fn 同一个模板的行归成一类_变量位置是星号() {
        let got = classify(
            "INFO user 1 login from 10.0.0.1\n\
             INFO user 22 login from 10.0.0.2\n\
             ERROR db timeout after 300ms\n\
             ERROR db timeout after 500ms\n\
             INFO user 7 login from 10.0.0.9\n\
             INFO cache evict key=a\n",
        );
        assert_eq!(
            got,
            [
                ("INFO user <*> login from <*>.<*>.<*>.<*>".to_string(), 3),
                ("ERROR db timeout after <*>".to_string(), 2),
                ("INFO cache evict key=a".to_string(), 1),
            ]
        );
    }

    /// 同位置上的常量词不一样、但过半一样：归进去，那个位置变成变量
    #[test]
    fn 过半的常量词对得上就归进去() {
        // alice / bob 那一位不一样，其余四个常量词对得上（4/5）：归一类。「shipped to carol」和它只对得上 INFO、order（2/5）：
        // 自立一类 —— 付款和发货本来就是两件事（第一版测试按「全并成一类」写的，是测试写错了，算法是对的）
        let got = classify("INFO order 1 paid by alice\nINFO order 2 paid by bob\nINFO order 3 shipped to carol\n");
        assert_eq!(got, [("INFO order <*> paid by <*>".to_string(), 2), ("INFO order <*> shipped to carol".to_string(), 1)]);
        let got = classify("WARN disk full on sda\nWARN cpu hot at core\n");
        assert_eq!(got.len(), 2, "只有 WARN 一样：{got:?}");
    }

    /// 整个词都是变量的占了一半以上（id、数字一串）：原版 Drain 的相似度（一样的词 / 总词数）在这里永远过不了半，每行自成一类。
    /// 这里分母只数这一行的常量词。**时间戳不是这种情况**：按段归一之后 `2026-08-24` 是 `<*>-<*>-<*>`，形状本身是常量、对得上 ——
    /// 第一版拿时间戳的例子去测这一条，把相似度改回原版测试照样绿（2026-10-14 验红时发现），换成下面这个
    #[test]
    fn 整词变量占一半以上也能归到一起() {
        let got = classify("user 1 2 3 4 login\nuser 5 6 7 8 login\n");
        assert_eq!(got, [("user <*> <*> <*> <*> login".to_string(), 2)]);
    }

    /// 时间戳、线程号按段归一后是常量形状，模板里留着它们的样子
    #[test]
    fn 时间戳和线程号留着形状() {
        let got = classify(
            "2026-08-24 10:15:02.101 INFO [http-nio-8080-exec-3] c.d.Order - paid orderId=1 userId=7\n\
             2026-08-24 10:15:03.555 INFO [http-nio-8080-exec-9] c.d.Order - paid orderId=2 userId=8\n",
        );
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "<*>-<*>-<*> <*>:<*>:<*>.<*> INFO [http-nio-<*>-exec-<*>] c.d.Order - paid orderId=<*> userId=<*>");
    }

    /// 带时间戳的文件里，堆栈帧是那条报错的附属，不自成一类；级别跟着记下
    #[test]
    fn 只归记录头_堆栈不算_级别跟着记() {
        // 异常那一行不是栈帧（没有缩进的 at），只认栈帧的话它会自成一类 —— 「只归记录头」才挡得住它
        let body = "2026-08-24 10:00:00 ERROR boom id=1\njava.lang.IllegalStateException: pool exhausted\n\tat a.B(B.java:1)\n\tat a.C(C.java:2)\n2026-08-24 10:00:01 ERROR boom id=2\n2026-08-24 10:00:02 INFO ok\n";
        let t = task(body.len() as u64);
        run(&[body.as_bytes()], &[], &t, MAX_PATTERNS);
        let s = t.stat();
        assert_eq!(s.lines, 3, "三条记录头，异常行和两行堆栈都不算");
        assert_eq!(s.patterns, 2);
        assert_eq!(s.top[0].count, 2);
        assert_eq!(s.top[0].levels[Level::Error.index()], 2);
        // 没有时间戳的文件：所有非空行都归，堆栈帧照样跳过
        let got = classify("boom 1\n\tat a.B(B.java:1)\n\nboom 2\n");
        assert_eq!(got, [("boom <*>".to_string(), 2)]);
    }

    #[test]
    fn 几份文件一起归() {
        let t = task(0);
        run(&[b"INFO a 1\n".as_slice(), b"INFO a 2\nINFO b\n".as_slice()], &[], &t, MAX_PATTERNS);
        let s = t.stat();
        assert_eq!((s.lines, s.patterns, s.top[0].count), (3, 2, 2));
    }

    /// 到了上限：不再分新类，没归进去的行照实记着（界面要说出来）
    #[test]
    fn 类数到上限_多出来的记进_overflow() {
        let t = task(0);
        run(&[b"alpha\nbeta\ngamma\nalpha\ndelta\n".as_slice()], &[], &t, 2);
        let s = t.stat();
        assert_eq!((s.patterns, s.lines, s.overflow), (2, 5, 2), "alpha / beta 两类，alpha 第二次照样归进去，gamma / delta 没地方");
    }

    #[test]
    fn 很长的行只看前四十个词() {
        let a = (0..100).map(|i| format!("w{}x", "abc".repeat(i % 3 + 1))).collect::<Vec<_>>().join(" ");
        let got = classify(&format!("{a}\n{a} extra\n"));
        assert_eq!(got.len(), 1, "前四十个词一样就是一类");
        assert!(got[0].0.ends_with(" …"), "后面折成一个 …");
    }

    fn compare(cur: &str, base: &str) -> PatternStat {
        let mut t = task(0);
        t.compare = true;
        run(&[cur.as_bytes()], &[base.as_bytes()], &t, MAX_PATTERNS);
        t.stat()
    }

    fn rep(line: &str, n: usize) -> String {
        (0..n).map(|i| format!("{line}{i}\n")).collect()
    }

    /// 对比：新出现的排最前，没了的其次，两边都有的按变化幅度；两边的同一类是同一类（一棵树里归）
    #[test]
    fn 对比_新出现的在前_没了的其次_再按变化() {
        let base = rep("A ok id=", 100) + &rep("B slow id=", 10) + &rep("G cron tick ", 5);
        let cur = rep("A ok id=", 200) + &rep("B slow id=", 80) + &rep("N disk full ", 3);
        let s = compare(&cur, &base);
        let got: Vec<(String, u64, u64)> = s.top.iter().map(|p| (String::from_utf8(p.template.clone()).unwrap(), p.base, p.count)).collect();
        assert_eq!(
            got,
            [
                ("N disk full <*>".to_string(), 0, 3),
                ("G cron tick <*>".to_string(), 5, 0),
                ("B slow id=<*>".to_string(), 10, 80),
                ("A ok id=<*>".to_string(), 100, 200),
            ]
        );
        assert_eq!((s.base_lines, s.lines), (115, 283));
    }

    /// 按**频率**比、不按次数比：今天的日志长了，次数跟着涨不算变化。按次数 A 涨了三倍、B 没动；
    /// 按频率 A 从一半涨到四分之三（×1.5），B 从一半掉到四分之一（÷2）—— B 的变化更大，排前面
    #[test]
    fn 对比_按频率不按次数() {
        let base = rep("A ok id=", 100) + &rep("B slow id=", 100);
        let cur = rep("A ok id=", 300) + &rep("B slow id=", 100);
        let s = compare(&cur, &base);
        let first = String::from_utf8(s.top[0].template.clone()).unwrap();
        assert_eq!(first, "B slow id=<*>", "按次数比的话 A（×3）会排在前面");
    }

    /// 预先置位：半路走人、不置 done（同 lib.rs 那两条取消测试：行数要跨过 64K，那个检查才执行得到）
    #[test]
    fn 被取消时不跑完也不说跑完了() {
        let body: String = (0..70_000).map(|i| format!("line {i} ok\n")).collect();
        let t = task(body.len() as u64);
        t.cancel();
        run(&[body.as_bytes()], &[], &t, MAX_PATTERNS);
        assert!(!t.stat().done);
    }
}
