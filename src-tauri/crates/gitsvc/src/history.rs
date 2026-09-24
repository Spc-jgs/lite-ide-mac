//! 提交历史：日志、某次提交改了哪些文件、提交的差异、cherry-pick。

use super::*;

// ─────────────────── 历史 · 分支 · 工作树 ───────────────────

/// 提交历史里的一条。比 [`Commit`] 多带画图和跳转需要的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub sha: String,
    pub short: String,
    pub author: String,
    pub email: String,
    /// 相对时间（"3 hours ago"），git 自己算
    pub when: String,
    /// 绝对日期 YYYY-MM-DD
    pub date: String,
    pub subject: String,
    /// 父提交的完整 sha。合并提交有两个及以上 —— 画泳道图全靠它
    pub parents: Vec<String>,
    /// 指向这条提交的引用名（分支、标签、HEAD）
    pub refs: Vec<String>,
}

/// 提交历史。
///
/// `all` 为真时把所有分支都算进来（IDEA 的「全部分支」），否则只看当前 HEAD 这条线。
/// `path` 非空时只看某个文件的历史。
pub fn log_entries(
    root: impl AsRef<Path>,
    limit: usize,
    all: bool,
    path: &str,
) -> R<Vec<LogEntry>> {
    let n = format!("-{limit}");
    // %H sha · %h 短 sha · %an 作者 · %ae 邮箱 · %ar 相对时间 · %ad 日期
    // %s 标题 · %P 父提交们 · %D 引用名
    let fmt = format!(
        "--format=%H{US}%h{US}%an{US}%ae{US}%ar{US}%ad{US}%s{US}%P{US}%D"
    );
    // --topo-order 不是可选项，是泳道图的前提：
    // 默认的提交时间序里，父提交完全可能排在子提交前面（两条提交时间戳相同时
    // 就会这样，合并操作尤其常见）。一旦父先于子出现，泳道算法「认领正在等我的
    // 那条泳道」的前提就不成立，主线会莫名其妙断掉、跳到别的泳道去。
    // gitk 和 IDEA 用拓扑序也是为这个。
    let mut args = vec![
        "--no-pager",
        "log",
        &n,
        &fmt,
        "--date=short",
        "--topo-order",
    ];
    if all {
        args.push("--all");
    }
    if !path.is_empty() {
        args.push("--");
        args.push(path);
    }
    let out = match run(root.as_ref(), &args) {
        Ok(s) => s,
        // 空仓库没有历史，这不是错误
        Err(Error::Git(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(out.lines().filter(|l| !l.is_empty()).filter_map(parse_log_line).collect())
}

fn parse_log_line(l: &str) -> Option<LogEntry> {
    let f: Vec<&str> = l.split(US).collect();
    if f.len() < 9 {
        return None;
    }
    Some(LogEntry {
        sha: f[0].to_string(),
        short: f[1].to_string(),
        author: f[2].to_string(),
        email: f[3].to_string(),
        when: f[4].to_string(),
        date: f[5].to_string(),
        subject: f[6].to_string(),
        parents: f[7].split_whitespace().map(str::to_string).collect(),
        // %D 形如 "HEAD -> main, origin/main, tag: v1.0"
        refs: f[8]
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_start_matches("HEAD -> ").to_string())
            .collect(),
    })
}

/// 一次提交里动了哪些文件。`--name-status` 给出状态字母 + 路径。
///
/// 合并提交默认什么都不输出（git 认为差异有歧义），加 `-m --first-parent`
/// 让它按「相对第一个父提交」算 —— 这也是人看合并提交时想看的东西。
pub fn commit_files(root: impl AsRef<Path>, sha: &str) -> R<Vec<Entry>> {
    let raw = run_raw(
        root.as_ref(),
        &[
            "--no-pager",
            "show",
            "--name-status",
            "--format=",
            "-m",
            "--first-parent",
            "-z",
            sha,
        ],
    )?;
    let mut out = Vec::new();
    // -z 下记录是 `<状态>\0<路径>\0`，改名则是 `<状态>\0<旧>\0<新>\0`
    let mut it = raw.split(|&b| b == 0).filter(|r| !r.is_empty());
    while let Some(st) = it.next() {
        let st = String::from_utf8_lossy(st);
        let code = st.chars().next().unwrap_or('M');
        let Some(p1) = it.next() else { break };
        let p1 = String::from_utf8_lossy(p1).into_owned();
        let (path, orig) = if code == 'R' || code == 'C' {
            match it.next() {
                Some(p2) => (String::from_utf8_lossy(p2).into_owned(), Some(p1)),
                None => (p1, None),
            }
        } else {
            (p1, None)
        };
        out.push(Entry {
            path,
            index: code,
            work: '.',
            untracked: false,
            conflicted: false,
            orig,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// 一个文件在 HEAD 里的内容（issue #33 ④：编辑器里实时算改动行要拿它当基线）。
///
/// 不在 HEAD 里（新文件、仓库还没有提交）给 `None` —— 那不是错误，是「没有基线」，
/// 界面上就不标。走 `run_capped` 是因为这也是一条「读子进程输出」的路：
/// 超过 [`MAX_DIFF_BYTES`] 的文件基线截断了就没法用，`truncated` 交给调用方判。
///
/// `HEAD:./path` 里那个 `./`：不带的话 git 把路径当成**相对仓库根**解释，
/// 带了才是相对 cwd。这里 cwd 就是仓库根，两种写法同义，选不带的那种；
/// 但 `path` 必须是仓库根下的相对路径（status 给的那种），和 [`diff`] 一致。
pub fn head_text(root: impl AsRef<Path>, path: &str) -> R<Option<Diff>> {
    let spec = format!("HEAD:{path}");
    let mut args = vec!["--no-pager", "-c", "core.pager=cat", "show"];
    args.extend_from_slice(DIFF_SAFE);
    args.push(&spec);
    match run_capped(root.as_ref(), &args, &[]) {
        Ok(d) => Ok(Some(d)),
        Err(Error::Git(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 某次提交里某个文件的差异。`path` 为空则给整次提交的差异。
///
/// 根提交要**先问有没有父**，不能靠 `sha^!` 报错来退回：`A^!` 是「A 减去它的父」这个
/// 集合，根提交没父，集合退化成单端点，而 `git diff <单端点>` 的语义是**和工作区比** ——
/// 退出码 0，退回分支永远走不到，界面上第一次提交的差异显示的是「到现在改了什么」
/// （2026-09-20 做 #39 时在临时仓库验出来的）。
pub fn commit_diff(root: impl AsRef<Path>, sha: &str, path: &str) -> R<Diff> {
    let root = root.as_ref();
    let has_parent = run(root, &["rev-parse", "--verify", "--quiet", &format!("{sha}^")])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !has_parent {
        // 和空树比：`show --root` 把根提交按「全部新增」给出来
        let mut a2 = vec!["--no-pager", "show", "--no-color", "--format=", "--root"];
        a2.extend_from_slice(DIFF_SAFE);
        a2.push(sha);
        if !path.is_empty() {
            a2.push("--");
            a2.push(path);
        }
        return run_capped(root, &a2, &[]);
    }
    let spec = format!("{sha}^!");
    let mut args = vec!["--no-pager", "-c", "core.pager=cat", "diff", "--no-color"];
    args.extend_from_slice(DIFF_SAFE);
    args.push(&spec);
    if !path.is_empty() {
        args.push("--");
        args.push(path);
    }
    run_capped(root, &args, &[])
}

/// 那次提交到**现在的工作区**的差异（issue #39「和本地比较」）：`git diff <sha> -- path`。
///
/// 和 [`commit_diff`] 是两个问题：那个答「那次提交改了什么」（`sha^!`，和它父比），
/// 这个答「从那时到现在改了什么」—— 中间的每次提交加上还没提交的都算在内。
/// 首次提交没有父在这里不是问题：比的是提交本身和工作区，不碰 `^`。
pub fn commit_vs_worktree(root: impl AsRef<Path>, sha: &str, path: &str) -> R<Diff> {
    let mut args = vec!["--no-pager", "-c", "core.pager=cat", "diff", "--no-color"];
    args.extend_from_slice(DIFF_SAFE);
    args.push(sha);
    if !path.is_empty() {
        args.push("--");
        args.push(path);
    }
    run_capped(root.as_ref(), &args, &[])
}

/// cherry-pick 一条提交到当前分支（issue #39）。
///
/// 撞冲突的路和 `stash pop` 一样：git 报错、工作区带冲突标记、`status` 里出现冲突条目，
/// **不回滚**（`.git/CHERRY_PICK_HEAD` 留着，人解完在终端 `--continue`）。
/// 调用方失败也要刷新一次状态 —— 冲突得让人看见。
/// 合并提交（多个父）不带 `-m` 会被 git 拒绝，前端在菜单上灰掉它，这里不另判。
pub fn cherry_pick(root: impl AsRef<Path>, sha: &str) -> R<String> {
    let sha = sha.trim();
    if sha.is_empty() {
        return Err(Error::Git("提交不能为空".into()));
    }
    run(root.as_ref(), &["cherry-pick", "--", sha])
}
