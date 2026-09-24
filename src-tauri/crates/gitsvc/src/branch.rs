//! 分支与工作树：列出、切换、新建、删除、改名。

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// 短名，如 `main` 或 `origin/main`
    pub name: String,
    pub sha: String,
    /// 上游分支，没有就是空
    pub upstream: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub when: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    pub sha: String,
    /// 检出的分支短名；游离头指针时为空
    pub branch: String,
    pub detached: bool,
    /// 裸仓库
    pub bare: bool,
    /// 被锁定（`git worktree lock`），不能直接删
    pub locked: bool,
    /// 就是当前打开的这个
    pub current: bool,
}

/// 分支列表（本地 + 远程），一次 `for-each-ref` 搞定。
///
/// 不用 `git branch`：它的输出是给人看的，前缀空格、`*` 标记、颜色都要再剥一层。
/// `for-each-ref` 的 `--format` 是给机器看的，要什么给什么。
pub fn branches(root: impl AsRef<Path>) -> R<Vec<Branch>> {
    let fmt = format!(
        "--format=%(refname:short){US}%(objectname:short){US}%(upstream:short){US}%(HEAD){US}%(committerdate:relative){US}%(contents:subject){US}%(refname)"
    );
    let out = run(
        root.as_ref(),
        &["for-each-ref", &fmt, "refs/heads", "refs/remotes"],
    )?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split(US).collect();
            if f.len() < 7 {
                return None;
            }
            /*
             * `refs/remotes/origin/HEAD` 是个符号引用（指向远程的默认分支），
             * 不是能检出的东西，列出来只会碍事。
             *
             * **必须按全名判断**：git 缩写远程 HEAD 时会把 `/HEAD` 一起吃掉，
             * `refs/remotes/origin/HEAD` 的 `%(refname:short)` 是 **`origin`**，
             * 不是 `origin/HEAD`。所以按短名过滤永远匹配不上，
             * 界面上就会多出一条叫「origin」的假分支，点了必然报错。
             */
            if f[6].ends_with("/HEAD") {
                return None;
            }
            Some(Branch {
                name: f[0].to_string(),
                sha: f[1].to_string(),
                upstream: f[2].to_string(),
                is_head: f[3] == "*",
                is_remote: f[6].starts_with("refs/remotes/"),
                when: f[4].to_string(),
                subject: f[5].to_string(),
            })
        })
        .collect())
}

/// 切分支。
///
/// - `create` 为真：新建并切过去（`switch -c`）
/// - 名字是**远程分支全名**（如 `origin/foo`）时走 `--track`，
///   建一个跟踪它的同名本地分支
///
/// 关于远程分支有个坑：`git switch origin/foo` 会直接失败 ——
/// `fatal: a branch is expected, got remote branch 'origin/foo'`。
/// git 的 DWIM（自动建跟踪分支）只对**短名**生效：本地没有 `foo` 而
/// `origin/foo` 存在时，`git switch foo` 才会自动建。传全名反而不行。
/// 界面上列出来的是全名（要区分 origin/foo 和 upstream/foo），
/// 所以这里得把这层翻译做掉。
///
/// 工作区脏的时候 git 会自己拒绝。**「本地改动会被覆盖」这一类单独分出来**
/// （[`Error::LocalChanges`]，带上挡路的文件名），前端才给得出「去提交 /
/// 丢弃这些改动」两个按钮；其余的错误照旧原样上抛，git 的措辞比我们能写的准。
pub fn switch_branch(root: impl AsRef<Path>, name: &str, create: bool) -> R<String> {
    switch_branch_from(root, name, create, "")
}

/// 同 [`switch_branch`]，`create` 时可以指定起点 `from`（`switch -c name from`）。
/// 起点留空 = 当前 HEAD。分支菜单里「从 X 新建分支」走这条 —— 不先切到 X 再 `-c`，
/// 那是两次检出，中间那次会把工作区翻一遍。
pub fn switch_branch_from(root: impl AsRef<Path>, name: &str, create: bool, from: &str) -> R<String> {
    let name = name.trim();
    let from = from.trim();
    if name.is_empty() {
        return Err(Error::Git("分支名不能为空".into()));
    }
    let root = root.as_ref();
    /// 所有出口都要过这一遍分类 —— 这个函数有五个 `run(...)` 的返回点，
    /// 少包一个的表现就是「大部分时候给按钮，某个分支上突然给英文报错」。
    fn classify(r: R<String>) -> R<String> {
        match r {
            Err(Error::Git(msg)) => match parse_local_changes(&msg) {
                Some(files) => Err(Error::LocalChanges { files, raw: msg }),
                None => Err(Error::Git(msg)),
            },
            other => other,
        }
    }
    if create {
        let mut args = vec!["switch", "-c", name];
        if !from.is_empty() {
            args.push(from);
        }
        return classify(run(root, &args));
    }

    let exists = |r: &str| {
        run(root, &["rev-parse", "--verify", "--quiet", r])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    };

    // 本地就有同名分支：直接切，最常见的情形
    if exists(&format!("refs/heads/{name}")) {
        return classify(run(root, &["switch", name]));
    }
    // 是个远程分支：建跟踪分支切过去
    if exists(&format!("refs/remotes/{name}")) {
        let short = name.split_once('/').map(|(_, b)| b).unwrap_or(name);
        // 本地已经有同名短分支了（跟踪的可能是别的远程），就切到那个，
        // 别再建一个重名的
        if exists(&format!("refs/heads/{short}")) {
            return classify(run(root, &["switch", short]));
        }
        return classify(run(root, &["switch", "--track", name]));
    }
    // 既不是本地也不是远程分支：tag 或 sha，只能游离检出（issue #33 ⑬）。
    // 不带 `--detach` 的话 `git switch <sha>` 直接拒绝（"a branch is expected"），
    // 而 `checkout <sha>` 又不会把本地改动挡路的报错分成那一类
    classify(run(root, &["switch", "--detach", name]))
}

/// 从 git 的 stderr 里认出「本地改动挡着切分支」，并把挡路的文件名切出来。
///
/// git 的原话长这样（`LC_ALL=C` 保证是英文，见 `git_cmd`）：
///
/// ```text
/// error: Your local changes to the following files would be overwritten by checkout:
///         src/a.rs
///         src/b.rs
/// Please commit your changes or stash them before you switch branches.
/// Aborting
/// ```
///
/// 未跟踪文件挡路时是另一句（"The following untracked working tree files
/// would be overwritten by checkout"），**格式一样**，所以按「would be
/// overwritten by」这一句认，两种都收。
///
/// 认不出来就返回 `None` —— 那时照旧把原话上抛，它的措辞比我们能写的准。
pub(crate) fn parse_local_changes(stderr: &str) -> Option<Vec<String>> {
    let mut lines = stderr.lines();
    lines.find(|l| l.contains("would be overwritten by"))?;
    // 文件名是缩进的，一行一个；碰到第一行不缩进的（"Please commit …"）就到头了
    let files: Vec<String> = lines
        .take_while(|l| l.starts_with('\t') || l.starts_with("  "))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    // 认出了句子却一个文件都没切出来，说明格式和预期不一样 —— 那就别装懂
    if files.is_empty() {
        return None;
    }
    Some(files)
}

/// 工作树列表。`--porcelain` 的记录以空行分隔，每行是 `键 值`。
pub fn worktrees(root: impl AsRef<Path>) -> R<Vec<Worktree>> {
    let root = root.as_ref();
    let out = run(root, &["worktree", "list", "--porcelain"])?;
    let here = root.to_string_lossy().into_owned();
    let mut list = Vec::new();
    let mut cur: Option<Worktree> = None;

    let flush = |cur: &mut Option<Worktree>, list: &mut Vec<Worktree>| {
        if let Some(w) = cur.take() {
            list.push(w);
        }
    };

    for line in out.lines() {
        if line.is_empty() {
            flush(&mut cur, &mut list);
            continue;
        }
        let (key, val) = match line.split_once(' ') {
            Some((k, v)) => (k, v),
            None => (line, ""),
        };
        match key {
            "worktree" => {
                flush(&mut cur, &mut list);
                cur = Some(Worktree {
                    current: val == here,
                    path: val.to_string(),
                    sha: String::new(),
                    branch: String::new(),
                    detached: false,
                    bare: false,
                    locked: false,
                });
            }
            "HEAD" => {
                if let Some(w) = cur.as_mut() {
                    w.sha = val.chars().take(7).collect();
                }
            }
            "branch" => {
                if let Some(w) = cur.as_mut() {
                    w.branch = val.trim_start_matches("refs/heads/").to_string();
                }
            }
            "detached" => {
                if let Some(w) = cur.as_mut() {
                    w.detached = true;
                }
            }
            "bare" => {
                if let Some(w) = cur.as_mut() {
                    w.bare = true;
                }
            }
            "locked" => {
                if let Some(w) = cur.as_mut() {
                    w.locked = true;
                }
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut list);
    Ok(list)
}

/// 新建工作树。
///
/// `branch` 已存在就检出它，不存在就顺带新建（`-b`）—— 这个判断放在这里而不是
/// 前端：它是「git 里分支存不存在」的业务判断，而且省掉一次 IPC 往返。
/// 用户在意的只是「我要一个跑着这个分支的目录」，不该关心加不加 `-b`。
///
/// 返回新工作树的绝对路径，调用方可以直接把它当项目根打开。
pub fn worktree_add(root: impl AsRef<Path>, path: &str, branch: &str) -> R<String> {
    if path.trim().is_empty() {
        return Err(Error::Git("工作树路径不能为空".into()));
    }
    let root_p = root.as_ref();
    let refspec = format!("refs/heads/{branch}");
    let exists = !branch.is_empty()
        && run(root_p, &["rev-parse", "--verify", "--quiet", &refspec])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    let mut args: Vec<&str> = vec!["worktree", "add"];
    if branch.is_empty() {
        args.push(path);
    } else if exists {
        args.push(path);
        args.push(branch);
    } else {
        args.push("-b");
        args.push(branch);
        args.push(path);
    }
    run(root.as_ref(), &args)?;
    // git 接受相对路径，但前端要的是能直接打开的绝对路径
    let p = Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.as_ref().join(p)
    };
    Ok(std::fs::canonicalize(&abs)
        .unwrap_or(abs)
        .to_string_lossy()
        .into_owned())
}

/// 移除工作树。**会删掉那个目录**，调用方必须先让用户确认。
///
/// `force` 对应 `--force`：里面有未提交改动时 git 会拒绝，除非强制。
pub fn worktree_remove(root: impl AsRef<Path>, path: &str, force: bool) -> R<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path);
    run(root.as_ref(), &args).map(|_| ())
}

/// 删本地分支。
///
/// 默认走 `-d`：分支上有没合进别处的提交时 git 会拒绝，分成 [`Error::NotMerged`]
/// 上抛，界面上再给「仍然删除」那条路（`force` = `-D`）。不一上来就 `-D`：
/// 删分支本身不可逆（reflog 能捞但那是命令行的事），能让 git 先拦一道就让它拦。
///
/// 当前分支删不掉是 git 自己的规矩，那句报错照原样透出去。
pub fn branch_delete(root: impl AsRef<Path>, name: &str, force: bool) -> R<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Git("分支名不能为空".into()));
    }
    // `--` 之后才是分支名：一个叫 `-D` 的分支名不该变成开关
    let args = [
        "branch",
        if force { "-D" } else { "-d" },
        "--",
        name,
    ];
    match run(root.as_ref(), &args) {
        Ok(_) => Ok(()),
        Err(Error::Git(msg)) if msg.contains("not fully merged") => {
            Err(Error::NotMerged { raw: msg })
        }
        Err(e) => Err(e),
    }
}

/// 重命名本地分支（`branch -m`）。上游跟踪配置 git 会一起搬，不用管。
///
/// 不用 `-M`：目标名已存在时该报错，而不是把那条分支悄悄盖掉 ——
/// 和 fsservice 里「rename 默认覆盖目标」是同一条戒心。
pub fn branch_rename(root: impl AsRef<Path>, old: &str, new: &str) -> R<()> {
    let (old, new) = (old.trim(), new.trim());
    if old.is_empty() || new.is_empty() {
        return Err(Error::Git("分支名不能为空".into()));
    }
    run(root.as_ref(), &["branch", "-m", "--", old, new]).map(|_| ())
}
