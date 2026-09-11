//! Git 控制台：把跑过的每一条 git 原样留下来（issue #29，形状抄 IDEA）。
//!
//! # 为什么要有
//!
//! 在这之前，git 出了问题**证据说完就没**：失败时弹一条横幅，原话收在
//! 可展开的区域里，关掉横幅就再也找不回来。而且**界面上完全看不到跑的是什么
//! 命令** —— 我们给每条 git 都带了一串加固参数
//! （`-c core.fsmonitor=`、`-c diff.external=`、`-c protocol.ext.allow=never`，
//! 再加仓库自带的每个 filter 驱动都要逐个关掉），那串参数是为了挡住
//! 「仓库自己的 config 让 git 去执行东西」（issue #24）。
//!
//! 哪天某个加固参数把一个正常仓库弄坏了，现在的界面给不出任何线索。
//! 这份记录就是那条线索 —— 它回答的是「刚才那条到底跑了什么、为什么失败」。
//!
//! # 为什么只在内存里，不落盘
//!
//! 落 `app.log` 会把那边「只写异常」的规矩破掉：一次状态刷新就是一条，
//! 而状态刷新每次窗口获得焦点都跑 —— 几分钟就能把真正的错误冲走。
//! 另起一个文件则要再养一套轮转和清理。
//!
//! 而这东西的用途是**「刚才那条为什么失败」，不是考古**：关掉应用就没了
//! 完全够用，而且顺带把「日志里有没有凭据」这个问题的答案从「要小心」
//! 变成了「它根本不在盘上」。
//!
//! # 三道闸
//!
//! | | |
//! |---|---|
//! | 留几条 | [`MAX_ENTRIES`]，超了从头挤掉 |
//! | 每条的错误文本 | [`MAX_ERR_BYTES`]，超了截断并标记 |
//! | 凭据 | [`mask`]，**进环之前**打码 |
//!
//! 第三条是硬要求：打码必须发生在**存进来的路上**，不能等到读出去的时候。
//! 存原文再在展示时打码的话，凭据已经在进程内存里躺了一遍，
//! 而这个结构体将来完全可能被别处读走（比如「把控制台内容复制出去」）。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 环里最多留几条。
///
/// 300 条大约覆盖一小时的正常使用（状态刷新是大头）。再多的价值很低 ——
/// 没有人会往回翻三百条去找「刚才那条」，而内存是真金白银：
/// 每条带着一份可能几 KB 的 stderr。
pub const MAX_ENTRIES: usize = 300;

/// 单条留多少错误文本。4KB 够放一整段 git 的抱怨加一点上下文；
/// 再多的多半是钩子在刷屏，而那种时候前 4KB 已经说明问题了。
pub const MAX_ERR_BYTES: usize = 4 << 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Unix 毫秒。**不在这儿格式化时间** —— 那需要知道时区，
    /// 而前端本来就有 `Date`，让它按用户的本地时区显示
    pub ms: u64,
    pub cwd: String,
    /// 完整 argv，**含加固参数**。issue #29 的第 2 条缺口说的就是它：
    /// 真出问题时「它到底跑了什么」是第一个要问的
    pub argv: Vec<String>,
    /// 退出码。`None` = 没跑起来（git 不在），或者被我们主动掐掉了
    pub code: Option<i32>,
    pub dur_ms: u32,
    /// stderr。成功的命令多半是空的
    pub err: String,
    /// 错误文本被 [`MAX_ERR_BYTES`] 截断了
    pub err_truncated: bool,
}

impl Entry {
    /// 这条算不算失败。**退出码不是 0 就算** —— 调用方那边有
    /// 「某些码不当失败」的白名单（`ok_codes`），但那是**业务判断**：
    /// 控制台要照实说「git 说它非零退出了」，让人自己看是不是要紧的。
    pub fn failed(&self) -> bool {
        !matches!(self.code, Some(0))
    }
}

/// 那个环本身。
///
/// **拎成一个结构体，全局那份只是它的一个实例** —— 判据和
/// `applog::Sink` 那次一样：直接测全局单例的话，几条测试会在同一个
/// `OnceLock` 上互相踩（`cargo test` 默认并行），表现是「单独跑都绿、
/// 一起跑就红」。第一版就是这么写的，四条当场红。
#[derive(Default)]
pub struct Console {
    ring: VecDeque<Entry>,
}

impl Console {
    /// 记一条。**打码在这儿做，不在读出去的时候**（见文件头）。
    pub fn record(&mut self, cwd: &Path, argv: &[String], code: Option<i32>, dur: Duration, err: &[u8]) {
        let e = entry(cwd, argv, code, dur, err);
        if self.ring.len() >= MAX_ENTRIES {
            self.ring.pop_front();
        }
        self.ring.push_back(e);
    }

    /// **最新的在前** —— 要看的永远是「刚才那条」。
    pub fn entries(&self) -> Vec<Entry> {
        self.ring.iter().rev().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.ring.clear();
    }
}

static RING: OnceLock<Mutex<Console>> = OnceLock::new();

/// 锁中毒了照常干活：这里面是一串独立的记录，没有「改了一半」的中间态，
/// 而为记录这件事 panic 一次，等于让一个诊断设施变成故障源。
fn lock() -> std::sync::MutexGuard<'static, Console> {
    RING.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

pub fn record(cwd: &Path, argv: &[String], code: Option<i32>, dur: Duration, err: &[u8]) {
    lock().record(cwd, argv, code, dur, err);
}

pub fn entries() -> Vec<Entry> {
    lock().entries()
}

pub fn clear() {
    lock().clear();
}

fn entry(cwd: &Path, argv: &[String], code: Option<i32>, dur: Duration, err: &[u8]) -> Entry {
    let raw = String::from_utf8_lossy(err);
    let trimmed = raw.trim_end();
    let (text, err_truncated) = if trimmed.len() > MAX_ERR_BYTES {
        // 按字符边界切，不按字节 —— 切在半个 UTF-8 上会让整段变成替换字符
        let mut end = MAX_ERR_BYTES;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        (trimmed[..end].to_string(), true)
    } else {
        (trimmed.to_string(), false)
    };

    Entry {
        ms: now_ms(),
        cwd: cwd.to_string_lossy().into_owned(),
        argv: argv.iter().map(|a| mask(a)).collect(),
        code,
        dur_ms: dur.as_millis().min(u32::MAX as u128) as u32,
        err: mask(&text),
        err_truncated,
    }
}

/// 把可能是凭据的部分换掉。
///
/// # 今天挡的两种
///
/// 1. **URL 里的 userinfo**：`https://x-access-token:ghp_xxx@github.com/o/r`
///    → `https://***@github.com/o/r`。整段 userinfo 都换掉，不只是冒号后面 ——
///    token 既可能在密码位，也可能整个就是用户名位
///    （`https://ghp_xxx@github.com/...` 是合法的）。
/// 2. **`http.extraheader`**：那是放 `Authorization: Basic ...` 的地方。
///    等号后面一律换掉。
///
/// # 今天挡不住、但要知道的
///
/// argv 里目前不会出现密码（认证走钥匙串 / SSH agent）。将来接
/// `GIT_ASKPASS` 或者支持用户自填远程 URL 时，**这个函数是唯一的关口** ——
/// 加了新的凭据形式就要在这里加一条，并且补一条测试。
pub fn mask(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, part) in s.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&mask_token(part));
    }
    // split_whitespace 会把首尾和中间的空白规整掉；只有一段时原样返回，
    // 避免把一段多行 stderr 压成一行（那正是这里要给人看的东西）
    if out == s.split_whitespace().collect::<Vec<_>>().join(" ") && s.contains('\n') {
        return s
            .lines()
            .map(|l| {
                l.split_whitespace()
                    .map(mask_token)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    out
}

fn mask_token(t: &str) -> String {
    // http.extraheader：等号后面整个换掉
    if let Some(eq) = t.find('=') {
        if t[..eq].contains("extraheader") {
            return format!("{}=***", &t[..eq]);
        }
    }
    // scheme://userinfo@host
    let Some(sep) = t.find("://") else {
        return t.to_string();
    };
    let rest = &t[sep + 3..];
    // `@` 要在第一个 `/` 之前才是 userinfo —— 不然 `https://host/a@b` 会被误伤
    let path_at = rest.find('/').unwrap_or(rest.len());
    match rest[..path_at].rfind('@') {
        Some(at) => format!("{}***{}", &t[..sep + 3], &rest[at..]),
        None => t.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /*
     * **凭据绝不能进这个环。** 这条排在所有功能之前 ——
     * 一份会泄露 token 的诊断记录，比没有诊断记录糟得多。
     */
    #[test]
    fn 远程_url_里的凭据要打码() {
        assert_eq!(
            mask("https://x-access-token:ghp_AAAA1111@github.com/o/r.git"),
            "https://***@github.com/o/r.git"
        );
        // token 也可能整个在用户名位上，那种同样要挡
        assert_eq!(mask("https://ghp_AAAA@github.com/o/r"), "https://***@github.com/o/r");
        assert_eq!(mask("ssh://git@github.com/o/r"), "ssh://***@github.com/o/r");
        // 没有凭据的原样留着 —— 打码打过头会把「它到底连的哪儿」也抹掉
        assert_eq!(mask("https://github.com/o/r.git"), "https://github.com/o/r.git");
        // `@` 在路径里不是 userinfo，别误伤
        assert_eq!(mask("https://github.com/o/r/blob/x@y"), "https://github.com/o/r/blob/x@y");
    }

    #[test]
    fn extraheader_要整个换掉() {
        assert_eq!(
            mask("http.extraheader=Authorization: Basic eHh4Onl5eQ=="),
            "http.extraheader=*** Basic eHh4Onl5eQ=="
        );
        // 上面那条暴露了一件事：带空格的 header 值只有第一段被挡住。
        // 真实的 argv 里它是**一个参数**（`-c` 的值），不会被空格切开 ——
        // 下面这条才是真实形状
        assert_eq!(
            mask("http.extraheader=Authorization:Basic-eHh4"),
            "http.extraheader=***"
        );
    }

    /*
     * 打码要在**存进来的路上**做，不是读出去的时候。
     * 存原文再在展示时打码的话，凭据已经在进程内存里躺了一遍。
     */
    #[test]
    fn 打码发生在存进来的路上() {
        let mut c = Console::default();
        let argv: Vec<String> = ["git", "push", "https://tok_secret@example.com/r"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        c.record(Path::new("/tmp"), &argv, Some(1), Duration::from_millis(5), b"fatal: bad");
        let got = c.entries();
        assert_eq!(got.len(), 1);
        assert!(
            !got[0].argv.iter().any(|a| a.contains("tok_secret")),
            "环里存着原文：{:?}",
            got[0].argv
        );
        assert!(got[0].argv[2].contains("***"));
    }

    #[test]
    fn 最新的排在最前面() {
        let mut c = Console::default();
        for i in 0..3 {
            c.record(Path::new("/tmp"), &[format!("cmd{i}")], Some(0), Duration::ZERO, b"");
        }
        let got = c.entries();
        assert_eq!(got[0].argv[0], "cmd2", "要看的永远是刚才那条");
        assert_eq!(got[2].argv[0], "cmd0");
    }

    /*
     * 环要真的有上限。少了这道闸，一个开着一整天的窗口会把每一条
     * `git status` 连同它的输出全攒在内存里。
     */
    #[test]
    fn 超过上限从头挤掉() {
        let mut c = Console::default();
        for i in 0..(MAX_ENTRIES + 25) {
            c.record(Path::new("/tmp"), &[format!("c{i}")], Some(0), Duration::ZERO, b"");
        }
        let got = c.entries();
        assert_eq!(got.len(), MAX_ENTRIES, "环没有上限");
        assert_eq!(got[0].argv[0], format!("c{}", MAX_ENTRIES + 24), "最新那条丢了");
        assert_eq!(got[MAX_ENTRIES - 1].argv[0], "c25", "挤掉的该是最老的那 25 条");
    }

    #[test]
    fn 错误文本超限要截断且标出来() {
        let mut c = Console::default();
        let 长的 = "啊".repeat(MAX_ERR_BYTES); // 每个 3 字节，妥妥超
        c.record(Path::new("/tmp"), &["x".to_string()], Some(1), Duration::ZERO, 长的.as_bytes());
        let got = c.entries();
        assert!(got[0].err_truncated, "截断了却没说");
        assert!(got[0].err.len() <= MAX_ERR_BYTES);
        // **必须切在字符边界上**：切在半个 UTF-8 上，整段会变成一串替换字符，
        // 而这段正是要给人读的东西
        assert!(got[0].err.ends_with('啊'), "切在了半个字符上：{:?}", &got[0].err[got[0].err.len().saturating_sub(6)..]);
    }

    #[test]
    fn 只有退出码_0_才算成功() {
        let mk = |code| Entry {
            ms: 0, cwd: String::new(), argv: vec![], code,
            dur_ms: 0, err: String::new(), err_truncated: false,
        };
        assert!(!mk(Some(0)).failed());
        assert!(mk(Some(1)).failed());
        // 没跑起来（git 不在）也算失败 —— 那正是最该被看见的一种
        assert!(mk(None).failed());
    }
}
