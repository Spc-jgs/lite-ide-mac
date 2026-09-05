//! 解析 git 的进度输出。
//!
//! # 为什么单独一个模块
//!
//! 这里全是**纯函数**：喂字节，吐 [`Progress`]。跟子进程、跟 Tauri 都无关，
//! 所以能拿真实的 stderr 当数据单测 —— 而这正是最容易悄悄错掉的一段。
//!
//! # 先看清 git 到底吐什么
//!
//! 本地 clone 一个 1248 对象的仓库，把 stderr 原样存下来量的：
//!
//! | | |
//! |---|---|
//! | 总字节 | 17,545 |
//! | 按 `\n` 分 | **7 行** |
//! | 按 `\r` 分 | **411 段** |
//! | 单段最长 | 86 字节 |
//!
//! **进度是用回车 `\r` 分隔的，不是换行** —— 它本来是给终端「原地刷新」用的。
//! 按行读只能读到 7 段而且全在结尾，进度条会一直 0% 然后突然 100%。
//!
//! 格式是 `阶段: 百分比% (当前/总数)`，收尾那段带 `, done.`：
//!
//! ```text
//! remote: Enumerating objects: 1248, done.
//! remote: Counting objects:   8% (100/1248)
//! Receiving objects:  62% (774/1248)
//! Resolving deltas: 100% (722/722), done.
//! ```

/// 一条解析出来的进度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// 阶段原文，`remote: ` 前缀已经去掉（那只是「这是远端在干活」的标记）
    pub phase: String,
    /// 百分比。解析不出来就是 `None` —— **那时 `phase` 里是整段原文**
    pub percent: Option<u8>,
    /// 已完成 / 总数
    pub done: Option<(u64, u64)>,
    /// 这一阶段结束了（`, done.`）
    pub finished: bool,
}

impl Progress {
    /// 只有一句话、没有数字的那种。
    ///
    /// **解析失败不是错误**：git 的进度文案不是稳定接口
    /// （`LC_ALL=C` 只保证是英文，不保证措辞不变）。认不出来就原样往上抛，
    /// 界面显示成一行状态而不是进度条 —— **绝不能因此把一次成功的 fetch 报成失败**。
    fn plain(text: &str) -> Self {
        Progress { phase: text.to_string(), percent: None, done: None, finished: false }
    }
}

/// 解析一段（已经按 `\r` / `\n` 切好的）文本。
///
/// 空白段返回 `None` —— git 会吐不少纯空白的填充段（原地刷新时用空格擦掉上一行的尾巴）。
pub fn parse_line(raw: &str) -> Option<Progress> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }
    // `remote: ` 只是说「这活是远端干的」，对进度条没有意义
    let line = line.strip_prefix("remote: ").unwrap_or(line);

    let finished = line.ends_with(", done.") || line.ends_with(" done.");

    // 冒号后面才是数字部分。没有冒号的（"Cloning into 'x'..."）当纯文本
    let Some((phase, rest)) = line.split_once(以冒号分隔) else {
        return Some(Progress { finished, ..Progress::plain(line) });
    };
    let phase = phase.trim().to_string();
    let rest = rest.trim();

    let percent = rest
        .split('%')
        .next()
        .and_then(|p| p.trim().parse::<u8>().ok())
        .filter(|_| rest.contains('%'));

    // `(774/1248)` —— 注意 `Enumerating objects: 1248, done.` 没有括号，
    // 那时 done 就是 None，只有 finished 是真
    let done = rest.split_once('(').and_then(|(_, r)| {
        let inner = r.split(')').next()?;
        let (a, b) = inner.split_once('/')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    });

    if percent.is_none() && done.is_none() && !finished {
        // 冒号后面没有任何数字：整段当文本，别把 phase 截半截
        return Some(Progress::plain(line));
    }
    Some(Progress { phase, percent, done, finished })
}

fn 以冒号分隔(c: char) -> bool {
    c == ':'
}

/// 按 `\r` 和 `\n` 一起切段的增量切分器。
///
/// **两个都要认。** 只认 `\n` 的话 411 段里只看得到 7 段；
/// 只认 `\r` 的话最后那几行（真正带 `\n` 的收尾）会粘成一段。
///
/// 「增量」是关键：流式读的时候，一次 `read()` 的边界
/// **一定会落在某一段中间**，所以尾巴要留到下一次。
pub struct Splitter {
    buf: String,
    /// 缓冲区上限。
    ///
    /// 单段实测最长 86 字节，但**这不是保证** —— 万一撞上一个不带分隔符
    /// 的怪输出，无上限的缓冲就是下一个「`git diff` 吃掉 30MB」。
    /// 判据同 AGENTS.md：新加任何「跑子进程读输出」的功能，
    /// 先问一句这东西的输出有上限吗。
    cap: usize,
}

/// 缓冲区上限。8KB 是实测单段（86 字节）的近百倍，正常情况下永远撞不到。
pub const MAX_SEGMENT: usize = 8 * 1024;

impl Default for Splitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Splitter {
    pub fn new() -> Self {
        Splitter { buf: String::new(), cap: MAX_SEGMENT }
    }

    /// 吃一块字节，吐出这块里能凑齐的所有段。
    ///
    /// 非 UTF-8 的字节用 `from_utf8_lossy` 兜住 —— git 的进度里可能带
    /// 远程分支名，而那是别人仓库里的字节，不保证合法。
    /// **这里不能返回 Err**：一个乱码的分支名不该让整次 fetch 失败。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Progress> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        while let Some(i) = self.buf.find(['\r', '\n']) {
            let seg: String = self.buf.drain(..=i).collect();
            if let Some(p) = parse_line(&seg) {
                out.push(p);
            }
        }
        // 超限就把攒着的丢掉，只留尾部 —— 丢的是一段没有分隔符的怪输出，
        // 不是有效进度。留尾部是为了后面真来了分隔符还能对上
        if self.buf.len() > self.cap {
            let keep = self.buf.len() - self.cap / 2;
            self.buf.drain(..keep);
        }
        out
    }

    /// 流结束了，把最后那截不带分隔符的尾巴吐出来。
    pub fn finish(&mut self) -> Option<Progress> {
        let tail = std::mem::take(&mut self.buf);
        parse_line(&tail)
    }
}

/// 把「推给前端」这件事节流。
///
/// 411 段是**本地** clone 1248 个对象的量；去网上拉一个大仓库是几千段。
/// 一段推一次的话，IPC 和 Svelte 的响应式会被一条进度条打满 ——
/// 而人眼一秒看不了 60 次百分比。
///
/// **阶段变了必须立刻推**：那是四次里的一次，被节流吃掉的话，
/// 界面就会停在「正在压缩对象 100%」上不动，看着像卡死了。
pub struct Throttle {
    last_phase: String,
    last_percent: Option<u8>,
    last_at: Option<std::time::Instant>,
    every: std::time::Duration,
}

impl Default for Throttle {
    fn default() -> Self {
        Self::new(std::time::Duration::from_millis(100))
    }
}

impl Throttle {
    pub fn new(every: std::time::Duration) -> Self {
        Throttle { last_phase: String::new(), last_percent: None, last_at: None, every }
    }

    /// 这一条要不要推上去。
    pub fn allow(&mut self, p: &Progress) -> bool {
        let now = std::time::Instant::now();
        let phase_changed = p.phase != self.last_phase;
        let elapsed_ok = self.last_at.is_none_or(|t| now.duration_since(t) >= self.every);
        // 收尾那条也一定要推：它是「这一阶段 100% 了」的唯一信号
        let pass = phase_changed || p.finished || (p.percent != self.last_percent && elapsed_ok);
        if pass {
            self.last_phase = p.phase.clone();
            self.last_percent = p.percent;
            self.last_at = Some(now);
        }
        pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_line ────────────────────────────────────────────────

    #[test]
    fn 解析真实的进度段() {
        let p = parse_line("remote: Counting objects:   8% (100/1248)        ").unwrap();
        assert_eq!(p.phase, "Counting objects", "remote: 前缀要去掉");
        assert_eq!(p.percent, Some(8));
        assert_eq!(p.done, Some((100, 1248)));
        assert!(!p.finished);

        let p = parse_line("Receiving objects:  62% (774/1248)").unwrap();
        assert_eq!(p.phase, "Receiving objects");
        assert_eq!(p.percent, Some(62));
        assert_eq!(p.done, Some((774, 1248)));

        let p = parse_line("Resolving deltas: 100% (722/722), done.").unwrap();
        assert_eq!(p.percent, Some(100));
        assert!(p.finished, "结尾的 , done. 要认出来");
    }

    #[test]
    fn 没有括号的收尾段也认() {
        // Enumerating 那条只有总数，没有 (a/b)
        let p = parse_line("remote: Enumerating objects: 1248, done.").unwrap();
        assert_eq!(p.phase, "Enumerating objects");
        assert_eq!(p.done, None, "没有括号就没有 done 对");
        assert!(p.finished);
    }

    #[test]
    fn 认不出来的原样抛上去不是错误() {
        // git 的措辞不是稳定接口，认不出来只能原样给界面
        let p = parse_line("Cloning into 'c1'...").unwrap();
        assert_eq!(p.phase, "Cloning into 'c1'...");
        assert_eq!(p.percent, None);
        assert_eq!(p.done, None);

        // 有冒号但后面没数字：也要保住整句，不能截成 "warning"
        let p = parse_line("warning: redirecting to https://example.com/").unwrap();
        assert!(p.phase.starts_with("warning:"), "整句要留着，实得：{}", p.phase);
    }

    #[test]
    fn 空白段被丢掉() {
        // git 原地刷新时会吐纯空格来擦掉上一行的尾巴
        assert!(parse_line("").is_none());
        assert!(parse_line("   \r").is_none());
    }

    // ── Splitter ──────────────────────────────────────────────────

    #[test]
    fn 必须按_r_切_不能只按_n() {
        // 这是整个模块存在的理由：真实进度是 \r 分隔的
        let mut s = Splitter::new();
        let raw = "remote: Counting objects:   1% (13/1248)        \r\
                   remote: Counting objects:   2% (25/1248)        \r\
                   remote: Counting objects:   3% (38/1248)        \r";
        let got = s.feed(raw.as_bytes());
        assert_eq!(got.len(), 3, "按 \\r 切应该得到 3 段，实得 {}", got.len());
        assert_eq!(got[2].percent, Some(3));
    }

    #[test]
    fn 分段边界落在一段中间也不丢() {
        // 流式读一定会遇到：一次 read 的边界不会正好落在分隔符上
        let mut s = Splitter::new();
        let a = s.feed(b"Receiving objects:  62% (77");
        assert!(a.is_empty(), "半段不该吐出来");
        let b = s.feed(b"4/1248)\rResolving deltas: 100% (722/722), done.\n");
        assert_eq!(b.len(), 2, "接上之后要吐出两段，实得 {}", b.len());
        assert_eq!(b[0].done, Some((774, 1248)), "被切开的那段要拼回原样");
        assert!(b[1].finished);
    }

    #[test]
    fn 结尾没有分隔符时靠_finish_收尾() {
        let mut s = Splitter::new();
        assert!(s.feed(b"Resolving deltas: 100% (722/722), done.").is_empty());
        let last = s.finish().expect("尾巴要吐出来");
        assert!(last.finished);
        assert!(s.finish().is_none(), "吐过一次之后就空了");
    }

    #[test]
    fn 没有分隔符的怪输出不会把内存吃光() {
        let mut s = Splitter::new();
        // 灌 10 倍上限的无分隔符垃圾
        for _ in 0..10 {
            s.feed(&vec![b'x'; MAX_SEGMENT]);
        }
        assert!(
            s.buf.len() <= MAX_SEGMENT,
            "缓冲区涨到了 {} 字节，上限是 {MAX_SEGMENT}",
            s.buf.len()
        );
    }

    #[test]
    fn 非_utf8_字节不会让整次操作失败() {
        let mut s = Splitter::new();
        // 远程分支名可能是别人仓库里的任意字节
        let got = s.feed(b"remote: Counting objects: 5% (1/2) \xff\xfe\r");
        assert_eq!(got.len(), 1, "乱码不该把这一段吞掉");
        assert_eq!(got[0].percent, Some(5));
    }

    // ── Throttle ──────────────────────────────────────────────────

    #[test]
    fn 阶段变了必须立刻放行() {
        // 被吃掉的话，界面会停在上一阶段的 100% 上不动，看着像卡死
        let mut t = Throttle::new(std::time::Duration::from_secs(3600));
        let a = parse_line("remote: Counting objects: 100% (1248/1248)").unwrap();
        let b = parse_line("Receiving objects:   1% (13/1248)").unwrap();
        assert!(t.allow(&a));
        assert!(t.allow(&b), "换阶段了，再长的节流窗口也要放行");
    }

    #[test]
    fn 收尾那条必须放行() {
        let mut t = Throttle::new(std::time::Duration::from_secs(3600));
        let a = parse_line("Receiving objects:  10% (124/1248)").unwrap();
        let b = parse_line("Receiving objects: 100% (1248/1248), done.").unwrap();
        assert!(t.allow(&a));
        assert!(t.allow(&b), "done 是「这一阶段完了」的唯一信号");
    }

    #[test]
    fn 同阶段的高频百分比要被挡下来() {
        let mut t = Throttle::new(std::time::Duration::from_secs(3600));
        let first = parse_line("Receiving objects:   1% (13/1248)").unwrap();
        assert!(t.allow(&first));
        let mut passed = 0;
        for i in 2..=99u8 {
            let p = parse_line(&format!("Receiving objects:  {i}% ({i}/1248)")).unwrap();
            if t.allow(&p) {
                passed += 1;
            }
        }
        assert_eq!(passed, 0, "同阶段、窗口没到，一条都不该放行，实得 {passed} 条");
    }

    #[test]
    fn 窗口到了就放行() {
        let mut t = Throttle::new(std::time::Duration::from_millis(1));
        let a = parse_line("Receiving objects:   1% (13/1248)").unwrap();
        let b = parse_line("Receiving objects:   2% (25/1248)").unwrap();
        assert!(t.allow(&a));
        std::thread::sleep(std::time::Duration::from_millis(3));
        assert!(t.allow(&b), "窗口过了就该放行");
    }

    // ── 整段真实输出 ──────────────────────────────────────────────

    #[test]
    fn 拿真实的那份_stderr_跑一遍() {
        /*
         * 这份是从真的 `git clone --progress` 里存下来的形状，
         * **不是自己编的格式漂亮的样例** —— 编的那种会把
         * 「纯空白填充段」「没有括号的收尾段」这类真实情况漏掉。
         */
        let real = "Cloning into 'c1'...\n\
            remote: Enumerating objects: 1248, done.        \r\
            remote: Counting objects:   0% (1/1248)        \r\
            remote: Counting objects:  50% (624/1248)        \r\
            remote: Counting objects: 100% (1248/1248), done.        \r\
            remote: Compressing objects: 100% (498/498), done.        \r\
            Receiving objects:  62% (774/1248)\r\
            Receiving objects: 100% (1248/1248), done.\n\
            Resolving deltas: 100% (722/722), done.\n";

        let mut s = Splitter::new();
        let mut all = s.feed(real.as_bytes());
        all.extend(s.finish());

        // 一条都不该丢
        assert!(all.len() >= 8, "解析出 {} 段，太少了", all.len());
        // 收尾的阶段全认出来了
        let dones: Vec<_> = all.iter().filter(|p| p.finished).map(|p| p.phase.as_str()).collect();
        assert!(dones.contains(&"Enumerating objects"), "实得 {dones:?}");
        assert!(dones.contains(&"Resolving deltas"), "实得 {dones:?}");
        // 百分比是升序的，没有跳回去（拼段拼错的话会乱序）
        let recv: Vec<u8> =
            all.iter().filter(|p| p.phase == "Receiving objects").filter_map(|p| p.percent).collect();
        assert_eq!(recv, vec![62, 100], "实得 {recv:?}");

        // 节流之后仍然保住每一次阶段切换
        let mut t = Throttle::new(std::time::Duration::from_secs(3600));
        let kept: Vec<_> = all.iter().filter(|p| t.allow(p)).collect();
        let phases: Vec<_> = kept.iter().map(|p| p.phase.as_str()).collect();
        for want in ["Enumerating objects", "Counting objects", "Receiving objects", "Resolving deltas"] {
            assert!(phases.contains(&want), "节流把「{want}」整个吃掉了：{phases:?}");
        }
    }
}
