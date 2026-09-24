//! 注解（blame）：每一段行出自哪次提交（issue #33 ⑭）。

use super::*;

/// blame 的一段：连续几行出自同一次提交（issue #33 ⑭）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameHunk {
    /// 全零 = 工作区里还没提交的行
    pub sha: String,
    pub short: String,
    pub author: String,
    /// 作者时间，unix 秒
    pub time: i64,
    pub summary: String,
    /// 在当前文件里从第几行开始（1-based）
    pub start: u32,
    pub count: u32,
}

/// `git blame --line-porcelain` 一次，压成段。
///
/// 用 `--line-porcelain` 而不是 `--porcelain`：后者只在一段的第一行给作者等元数据，
/// 后面的行只有一行 sha —— 省的是子进程输出，换来的是解析器要自己维护「上一次见到
/// 这个 sha 时的元数据」。这里的输出走 `run_capped`（1MB 上限），几千行的文件
/// 也就几百 KB；超了就截断，界面按「注解不全」处理，不猜。
///
/// 每行的头是 `<sha> <原行号> <现行号> [<段行数>]`，后面跟 `author` / `author-time` /
/// `summary` 等键值行，最后一行以 TAB 开头是内容。**按现行号连续 + sha 相同**合并成段，
/// 不信头里那个 `<段行数>` —— 它是 git 按原文件算的，和现文件里的连续性不是一回事。
pub fn blame(root: impl AsRef<Path>, path: &str) -> R<(Vec<BlameHunk>, bool)> {
    let out = run_capped(root.as_ref(), &["blame", "--line-porcelain", "--", path], &[])?;
    Ok((parse_blame(&out.text), out.truncated))
}

pub(crate) fn parse_blame(text: &str) -> Vec<BlameHunk> {
    let mut hunks: Vec<BlameHunk> = Vec::new();
    let mut cur: Option<BlameHunk> = None;
    for line in text.lines() {
        if let Some(body) = line.strip_prefix('\t') {
            let _ = body;
            // 内容行 = 这一条记录结束
            if let Some(h) = cur.take() {
                match hunks.last_mut() {
                    Some(last) if last.sha == h.sha && last.start + last.count == h.start => last.count += 1,
                    _ => hunks.push(h),
                }
            }
            continue;
        }
        if let Some(h) = cur.as_mut() {
            if let Some(v) = line.strip_prefix("author ") {
                h.author = v.to_string();
            } else if let Some(v) = line.strip_prefix("author-time ") {
                h.time = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("summary ") {
                h.summary = v.to_string();
            }
            continue;
        }
        // 头行：sha 原行 现行 [段行数]
        let mut it = line.split(' ');
        let (Some(sha), Some(_orig), Some(now)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        if sha.len() != 40 {
            continue;
        }
        let Ok(start) = now.parse::<u32>() else { continue };
        cur = Some(BlameHunk {
            sha: sha.to_string(),
            short: sha[..7].to_string(),
            author: String::new(),
            time: 0,
            summary: String::new(),
            start,
            count: 1,
        });
    }
    hunks
}
