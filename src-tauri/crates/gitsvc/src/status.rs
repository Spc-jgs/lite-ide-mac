//! 仓库状态：`git status --porcelain=v2` 的读取、上限和解析，以及「哪些目录被忽略」。

use super::*;

/// 读取仓库状态。
///
/// 用 `--porcelain=v2 --branch -z`：
/// - v2 把分支名和 ahead/behind 一起带出来，省掉第二次进程启动；
/// - `-z` 用 NUL 分隔记录，路径不做 C 风格转义 —— v1 遇到带空格或中文的
///   路径会加引号并转义，解析端要反过来解一遍，纯属自找麻烦。
pub fn status(root: impl AsRef<Path>) -> R<Status> {
    status_capped(root.as_ref(), MAX_STDOUT_BYTES)
}

/// 上限可注入的版本，**为了能测**（判据同 [`run_raw_capped`]）。
///
/// status **不能像 [`run_raw`] 那样超上限就报错**：改动多是仓库的正常状态，
/// 把整块 Git 功能变成一条报错，比少列几条改动糟得多。所以这条路截断，
/// 而截断了要标 `truncated` —— 界面据此说「还有更多」，
/// 和 `MAX_ENTRIES` 截断走的是同一个出口。
pub(crate) fn status_capped(root: &Path, cap: usize) -> R<Status> {
    let (raw, capped) = run_capped_raw(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
        ],
        cap,
        &[],
    )?;
    // 掐点落在哪儿全看运气，末尾多半是半条路径 —— 丢掉它。
    // 留着就会在改动列表里多出一个看着像真的、其实是半截的文件名
    let raw = if capped { trim_to_last_record(&raw) } else { &raw[..] };
    let mut st = parse_status(raw);
    if capped {
        st.truncated = true;
    }
    expand_untracked_dirs(root, &mut st);
    Ok(st)
}

/// 把折叠出来的未跟踪目录摊成里面的具体文件，追加进 `entries`。
///
/// **为什么不直接用 `--untracked-files=all`**：那样 git 就不再告诉我们哪些
/// 目录是*整个*未跟踪的了。文件树要这个信息给目录本身上色 —— 少了它，
/// 一个全新的目录只剩「里面有东西改了」的冒泡标记，和一个改了一行的
/// 老目录长得一模一样。这里两样都留下：目录名在 `untracked_dirs`，
/// 里面的文件在 `entries`。
///
/// 代价是多起一个子进程，只在真有折叠目录时才起。
fn expand_untracked_dirs(root: &Path, st: &mut Status) {
    if st.untracked_dirs.is_empty() || st.entries.len() >= MAX_ENTRIES {
        return;
    }
    let mut args: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    args.extend(st.untracked_dirs.iter().map(String::as_str));

    // 读不动就算了：目录名已经在 untracked_dirs 里，文件树照样能标色，
    // 只是改动列表少了那几条。为这个把整次 status 判成失败不划算。
    let Ok((raw, capped)) = run_capped_raw(root, &args, MAX_UNTRACKED_BYTES, &[]) else {
        return;
    };
    if capped {
        st.truncated = true;
    }

    for path in split_nul_records(&raw, capped) {
        if st.entries.len() >= MAX_ENTRIES {
            st.truncated = true;
            break;
        }
        st.entries.push(Entry {
            path,
            index: '.',
            work: '?',
            untracked: true,
            conflicted: false,
            orig: None,
        });
    }
    // parse_status 排过一次，但那是在这些文件进来之前
    st.entries.sort_by(|a, b| a.path.cmp(&b.path));
}

/// 问 git：这个仓库里哪些**目录**是被忽略的（相对根，不带末尾 `/`）。
///
/// # 谁要这个，为什么
///
/// 文件树和搜索一直靠一份**名字**名单判「这是不是生成物」
/// （`excludes::GENERATED_DIRS`）。名字判得了 `node_modules`，判不了
/// `dist` 和 `build` —— 那两个是常见的源码目录名（CMake 项目的 `build/`
/// 里放的是构建脚本）。名字只是怀疑，**真正的证据是 git 忽不忽略它**。
/// 见 issue #13。
///
/// # 为什么是 `ls-files` 而不是 `check-ignore`
///
/// `check-ignore` 要先知道问哪些路径，而候选目录散在树里任意深度 ——
/// 那就成了「边走边问」，一个 Gradle 多模块仓库能问出二十来次子进程。
/// `ls-files --directory` 反过来：**一次**把整棵树上被忽略的东西吐出来，
/// 而且整个被忽略的目录会被折叠成一条 `dir/`，正好是我们要的粒度。
///
/// `--exclude-standard` 认的是 `.gitignore` + `.git/info/exclude` + 全局那份，
/// **和 rg 默认认的是同一套** —— 这一条要紧：搜索那边装了 rg 走 rg 的
/// gitignore、没装 rg 走这份名单，两条路必须给同一个答案。
///
/// # 边界
///
/// - 不是 git 仓库、git 不在、读不动 → `Err`。调用方一律退回按名字判，
///   **不能因此把整次搜索判成失败**
/// - 输出设闸（[`MAX_IGNORED_BYTES`]）。一个 `.gitignore` 写得很散的仓库
///   （逐个文件列而不是列目录）能吐出很多条，而我们只要目录那几条 ——
///   超了就当只拿到前面那部分，宁可少跳几个目录，也不能把内存吃穿
/// - 只留**目录**（末尾是 `/` 的那些）。被忽略的单个文件不归这里管：
///   跳过一个文件省不下什么，而漏跳一个目录才是那个「凭空少一块」的问题
pub fn ignored_dirs(root: &Path) -> R<std::collections::BTreeSet<String>> {
    let args = [
        "ls-files",
        "--others",
        "--ignored",
        "--directory",
        "--exclude-standard",
        "-z",
    ];
    let (raw, capped) = run_capped_raw(root, &args, MAX_IGNORED_BYTES, &[])?;
    Ok(split_nul_records(&raw, capped)
        .into_iter()
        .filter_map(|p| p.strip_suffix('/').map(str::to_owned))
        .collect())
}

/// 砍到最后一条**完整**记录为止（`-z` 的记录以 NUL 结尾）。
///
/// 一个 NUL 都没有 = 连一条完整记录都没读到，那就一条都不能要。
fn trim_to_last_record(raw: &[u8]) -> &[u8] {
    match raw.iter().rposition(|&b| b == 0) {
        Some(i) => &raw[..i + 1],
        None => &[],
    }
}

/// 把 `-z` 的输出切成一条条记录。
///
/// `capped` 为真时**末尾那条要丢掉** —— `run_capped_raw` 是按字节掐的，
/// 掐点落在哪儿全看运气，末尾多半是半条路径。留着它，改动列表里就会多出一个
/// **看着像真的、其实是半截的**文件名（`src/OrderServ`），点开报「文件不存在」。
/// 这和差异截断要切回最后一个完整换行是同一条：宁可少一条，不能多一条假的。
pub(crate) fn split_nul_records(raw: &[u8], capped: bool) -> Vec<String> {
    let head = if capped { trim_to_last_record(raw) } else { raw };
    head
        .split(|&b| b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| String::from_utf8_lossy(r).into_owned())
        .collect()
}

/// v2 + `-z` 的记录解析。
///
/// 格式（`git status` 手册 "Porcelain Format Version 2"）：
/// - `# branch.head <name>` / `# branch.ab +N -M` 等表头
/// - `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`        普通变更
/// - `2 <XY> ... <X><score> <path>` + 独立一条 `<origPath>` 改名/复制
/// - `u <XY> ...  <path>`                                   冲突中
/// - `? <path>` / `! <path>`                                未跟踪 / 已忽略
///
/// 关键陷阱：改名条目在 `-z` 下**占两条记录** —— 新路径一条，源路径一条。
/// 按 NUL 切完后必须让解析器有状态地把下一条吃掉，否则源路径会被
/// 当成一条独立的畸形记录。
pub(crate) fn parse_status(raw: &[u8]) -> Status {
    let mut st = Status::default();
    let mut records = raw.split(|&b| b == 0).filter(|r| !r.is_empty());

    while let Some(rec) = records.next() {
        let line = String::from_utf8_lossy(rec);
        let line = line.as_ref();

        if let Some(rest) = line.strip_prefix("# ") {
            parse_branch_header(rest, &mut st);
            continue;
        }

        if st.entries.len() >= MAX_ENTRIES {
            st.truncated = true;
            // 不 break：还得把剩下的表头读完（表头其实在最前面，
            // 但依赖顺序是脆的，扫完更省心）
            continue;
        }

        let mut chars = line.chars();
        let kind = chars.next().unwrap_or(' ');
        match kind {
            '?' => {
                if let Some(p) = line.get(2..) {
                    // 整个未跟踪的目录被 git 折叠成一条 `dir/`，它走另一个口子 ——
                    // 见 Status::untracked_dirs
                    if p.ends_with('/') {
                        st.untracked_dirs.push(p.to_string());
                        continue;
                    }
                    st.entries.push(Entry {
                        path: p.to_string(),
                        index: '.',
                        work: '?',
                        untracked: true,
                        conflicted: false,
                        orig: None,
                    });
                }
            }
            // 已忽略的不进列表：用户要的是「我改了什么」，不是「git 不管什么」
            '!' => {}
            '1' | '2' | 'u' => {
                // 字段以单空格分隔；路径本身可能含空格，所以按固定字段数切
                let field_count = if kind == 'u' { 10 } else if kind == '1' { 8 } else { 9 };
                let Some((meta, path)) = split_fields(line, field_count) else {
                    continue;
                };
                let xy: Vec<char> = meta.get(1).map(|s| s.chars().collect()).unwrap_or_default();
                let (x, y) = (
                    xy.first().copied().unwrap_or('.'),
                    xy.get(1).copied().unwrap_or('.'),
                );
                // 改名条目的源路径是紧随其后的独立记录，必须在这里吃掉
                let orig = if kind == '2' {
                    records
                        .next()
                        .map(|r| String::from_utf8_lossy(r).into_owned())
                } else {
                    None
                };
                st.entries.push(Entry {
                    path: path.to_string(),
                    index: x,
                    work: y,
                    untracked: false,
                    conflicted: kind == 'u',
                    orig,
                });
            }
            _ => {}
        }
    }

    st.entries.sort_by(|a, b| a.path.cmp(&b.path));
    st
}

/// 从 `line` 里切出前 `n` 个空格分隔字段，剩下的整段当作路径。
///
/// 不能用 `splitn(n+1, ' ')` 一把梭 —— 那样返回的最后一段类型不同、
/// 还得再判长度；这里显式一点更好读，也更好在字段数不足时安全退出。
fn split_fields(line: &str, n: usize) -> Option<(Vec<&str>, &str)> {
    let mut fields = Vec::with_capacity(n);
    let mut rest = line;
    for _ in 0..n {
        let idx = rest.find(' ')?;
        fields.push(&rest[..idx]);
        rest = &rest[idx + 1..];
    }
    if rest.is_empty() {
        return None;
    }
    Some((fields, rest))
}

fn parse_branch_header(rest: &str, st: &mut Status) {
    let mut it = rest.splitn(2, ' ');
    let key = it.next().unwrap_or("");
    let val = it.next().unwrap_or("").trim();
    match key {
        "branch.head" => {
            if val == "(detached)" {
                st.detached = true;
            } else {
                st.branch = val.to_string();
            }
        }
        "branch.oid" => {
            // 一个提交都没有时 git 给的是字面量 "(initial)"
            if val == "(initial)" {
                st.unborn = true;
            } else {
                st.head = val[..val.len().min(7)].to_string();
                if st.branch.is_empty() && st.detached {
                    st.branch = format!("({})", st.head);
                }
            }
        }
        "branch.upstream" => st.upstream = val.to_string(),
        "branch.ab" => {
            // 形如 "+3 -1"
            for tok in val.split_whitespace() {
                let (sign, num) = tok.split_at(1);
                let n: u32 = num.parse().unwrap_or(0);
                match sign {
                    "+" => st.ahead = n,
                    "-" => st.behind = n,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// detached HEAD 时 `branch.oid` 可能排在 `branch.head` 前面，
/// 上面的赋值就落空了 —— 补一趟。
fn fill_detached_name(root: &Path, st: &mut Status) {
    if st.detached && st.branch.is_empty() {
        if let Ok(sha) = run(root, &["rev-parse", "--short", "HEAD"]) {
            st.branch = format!("({})", sha.trim());
        }
    }
}

/// 完整状态：解析 + 补齐 detached 名字。命令层用这个。
pub fn status_full(root: impl AsRef<Path>) -> R<Status> {
    let root = root.as_ref();
    let mut st = status(root)?;
    fill_detached_name(root, &mut st);
    if st.branch.is_empty() && st.unborn {
        // 空仓库：HEAD 指向的分支还不存在，但名字是有的
        if let Ok(n) = run(root, &["symbolic-ref", "--short", "HEAD"]) {
            st.branch = n.trim().to_string();
        }
    }
    Ok(st)
}
