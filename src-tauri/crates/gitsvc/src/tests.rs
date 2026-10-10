use super::*;
// 拆模块之后（2026-09-24）这几个解析函数住进了子模块，对测试放宽成 `pub(crate)`
use crate::blame::parse_blame;
use crate::branch::parse_local_changes;
use crate::status::{parse_status, split_nul_records, status_capped};

/// 解析器的输入是字节，用 `\0` 拼真实格式，不走 git
fn rec(parts: &[&str]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p.as_bytes());
        v.push(0);
    }
    v
}

#[test]
fn 表头带出分支与领先落后() {
    let raw = rec(&[
        "# branch.oid abc123def456",
        "# branch.head main",
        "# branch.upstream origin/main",
        "# branch.ab +3 -1",
    ]);
    let st = parse_status(&raw);
    assert_eq!(st.branch, "main");
    assert_eq!(st.head, "abc123d", "head 是 oid 截 7 位（草稿锚点记它）");
    assert_eq!(st.upstream, "origin/main");
    assert_eq!(st.ahead, 3);
    assert_eq!(st.behind, 1);
    assert!(!st.detached);
}

#[test]
fn 普通变更条目的xy与路径() {
    let raw = rec(&[
        "# branch.head main",
        "1 M. N... 100644 100644 100644 aaa bbb src/main.rs",
        "1 .M N... 100644 100644 100644 ccc ddd README.md",
    ]);
    let st = parse_status(&raw);
    assert_eq!(st.entries.len(), 2);
    // 排序后 README 在前
    assert_eq!(st.entries[0].path, "README.md");
    assert_eq!(st.entries[0].index, '.');
    assert_eq!(st.entries[0].work, 'M');
    assert!(st.entries[0].unstaged() && !st.entries[0].staged());
    assert_eq!(st.entries[1].path, "src/main.rs");
    assert!(st.entries[1].staged() && !st.entries[1].unstaged());
}

/// 这是 -z 格式最容易写错的地方：改名占两条记录
#[test]
fn 改名条目要吃掉紧随其后的源路径记录() {
    let raw = rec(&[
        "# branch.head main",
        "2 R. N... 100644 100644 100644 aaa bbb R100 新名字.rs",
        "旧名字.rs",
        "1 .M N... 100644 100644 100644 ccc ddd z.txt",
    ]);
    let st = parse_status(&raw);
    // 源路径不能变成第三条畸形条目
    assert_eq!(st.entries.len(), 2, "源路径被误当成独立条目了");
    let renamed = st.entries.iter().find(|e| e.path == "新名字.rs").unwrap();
    assert_eq!(renamed.orig.as_deref(), Some("旧名字.rs"));
    assert_eq!(renamed.index, 'R');
    assert!(st.entries.iter().any(|e| e.path == "z.txt"));
}

#[test]
fn 带空格的路径不能被切断() {
    let raw = rec(&[
        "# branch.head main",
        "1 .M N... 100644 100644 100644 aaa bbb docs/my notes/a b.md",
        "? 未跟踪 的文件.txt",
    ]);
    let st = parse_status(&raw);
    assert!(st.entries.iter().any(|e| e.path == "docs/my notes/a b.md"));
    let u = st.entries.iter().find(|e| e.untracked).unwrap();
    assert_eq!(u.path, "未跟踪 的文件.txt");
}

#[test]
fn 未跟踪与冲突与忽略() {
    let raw = rec(&[
        "# branch.head main",
        "? new.txt",
        "! ignored.log",
        "u UU N... 100644 100644 100644 100644 aaa bbb ccc conflict.rs",
    ]);
    let st = parse_status(&raw);
    // 已忽略的不进列表
    assert!(!st.entries.iter().any(|e| e.path == "ignored.log"));
    assert!(st.entries.iter().any(|e| e.untracked && e.path == "new.txt"));
    let c = st.entries.iter().find(|e| e.conflicted).unwrap();
    assert_eq!(c.path, "conflict.rs");
}

/// 折叠的未跟踪目录不进 `entries`，只留下目录名给文件树
#[test]
fn 折叠的未跟踪目录走另一个口子() {
    let raw = rec(&["# branch.head main", "? scratch/", "? notes.txt"]);
    let st = parse_status(&raw);
    assert_eq!(st.untracked_dirs, vec!["scratch/".to_string()]);
    assert_eq!(
        st.entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        vec!["notes.txt"],
        "目录不该出现在改动列表里：{:?}",
        st.entries
    );
}

/// 被字节上限掐掉时，末尾那条半截路径必须丢掉。
///
/// 留着它，改动列表里会多出一个**看着像真的、其实是半截的**文件名 ——
/// 而一个说谎的界面比一句「显示不下」危险得多（和差异截断切回换行同一条）。
#[test]
fn 截断的路径列表要丢掉末尾那条半截的() {
    let raw = b"a/one.txt\0b/two.txt\0c/thre".to_vec();

    // 没截断：末尾那条是完整的，三条都要
    assert_eq!(
        split_nul_records(&raw, false),
        vec!["a/one.txt", "b/two.txt", "c/thre"],
    );
    // 截断了：`c/thre` 是半截的
    assert_eq!(split_nul_records(&raw, true), vec!["a/one.txt", "b/two.txt"]);
    // 连一个 NUL 都没有 —— 一条完整记录都没读到，一条都不能要
    assert!(split_nul_records(b"c/thre", true).is_empty());
}

/// 「本地改动挡着切分支」要能认出来并切出文件名。
///
/// 这一条卡的是**界面上有没有按钮**：认出来才给「去提交 / 丢弃这些改动」，
/// 认不出来就退回原样上抛一段英文。
#[test]
fn 认得出本地改动挡着切分支() {
    let tracked = "error: Your local changes to the following files would be overwritten by checkout:\n\tsrc/a.rs\n\tsrc/b 有空格.rs\nPlease commit your changes or stash them before you switch branches.\nAborting\n";
    assert_eq!(
        parse_local_changes(tracked),
        Some(vec!["src/a.rs".to_string(), "src/b 有空格.rs".to_string()]),
    );

    // 未跟踪文件挡路是另一句，格式一样，也要收
    let untracked = "error: The following untracked working tree files would be overwritten by checkout:\n\tnew.txt\nPlease move or remove them before you switch branches.\n";
    assert_eq!(parse_local_changes(untracked), Some(vec!["new.txt".to_string()]));

    // 不是这一类的错误一律不认 —— 装懂比不懂糟
    assert_eq!(parse_local_changes("fatal: invalid reference: nope\n"), None);
    // 认出了句子却一个文件都没切出来，说明格式变了，也不装懂
    assert_eq!(
        parse_local_changes("error: files would be overwritten by checkout:\nAborting\n"),
        None,
    );
}

/// 冲突条目不能既算「已暂存」又算「改动」—— 那会让它在界面上出现三次
#[test]
fn 冲突条目既不算暂存也不算未暂存() {
    let raw = rec(&[
        "# branch.head main",
        "u UU N... 100644 100644 100644 100644 aaa bbb ccc both.rs",
        "1 M. N... 100644 100644 100644 aaa bbb staged.rs",
        "1 .M N... 100644 100644 100644 ccc ddd dirty.rs",
    ]);
    let st = parse_status(&raw);
    let c = st.entries.iter().find(|e| e.conflicted).unwrap();
    assert!(!c.staged(), "冲突条目不该算已暂存");
    assert!(!c.unstaged(), "冲突条目不该算未暂存");
    // 其余两条不受影响
    assert!(st.entries.iter().find(|e| e.path == "staged.rs").unwrap().staged());
    assert!(st.entries.iter().find(|e| e.path == "dirty.rs").unwrap().unstaged());
}

#[test]
fn detached与空仓库的表头() {
    let d = parse_status(&rec(&["# branch.oid a1b2c3d4e5", "# branch.head (detached)"]));
    assert!(d.detached);
    let u = parse_status(&rec(&["# branch.oid (initial)", "# branch.head main"]));
    assert!(u.unborn);
    assert_eq!(u.branch, "main");
}

#[test]
fn 超过上限要截断而不是撑爆前端() {
    let mut parts: Vec<String> = vec!["# branch.head main".into()];
    for i in 0..(MAX_ENTRIES + 10) {
        parts.push(format!("? f{i}.txt"));
    }
    let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
    let st = parse_status(&rec(&refs));
    assert_eq!(st.entries.len(), MAX_ENTRIES);
    assert!(st.truncated);
}

/// 端到端：真起 git 建个临时仓库跑一遍。
/// 没装 git 的机器上直接跳过，不让 CI 假红。
#[test]
fn 真仓库上的状态与暂存往返() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();

    // 空仓库：discover 能找到根，status 报 unborn
    assert!(discover(&dir).is_some());
    let st = status_full(&dir).unwrap();
    assert!(st.unborn, "刚 init 的仓库应该是 unborn");

    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
    let st = status_full(&dir).unwrap();
    assert!(st.entries.iter().any(|e| e.path == "a.txt" && e.untracked));

    // 空仓库上取消暂存必须走 rm --cached，不能崩
    stage(&dir, &["a.txt".into()]).unwrap();
    assert!(status_full(&dir).unwrap().entries[0].staged());
    unstage(&dir, &["a.txt".into()]).unwrap();
    assert!(status_full(&dir).unwrap().entries[0].untracked, "取消暂存后应变回未跟踪");

    stage(&dir, &["a.txt".into()]).unwrap();
    commit(&dir, "首次提交", false).unwrap();
    let st = status_full(&dir).unwrap();
    assert!(st.entries.is_empty(), "提交后工作区应该是干净的");
    assert!(!st.unborn);
    assert_eq!(st.branch, "main");

    // 改一行，diff 里应该同时有加和减
    std::fs::write(dir.join("a.txt"), "world\n").unwrap();
    let d = diff(&dir, "a.txt", false, false).unwrap().text;
    assert!(d.contains("-hello") && d.contains("+world"), "diff 不对：{d}");

    // 丢弃改动
    discard(&dir, &["a.txt".into()], &[]).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "hello\n");

    let l = log_entries(&dir, 10, false, "").unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].subject, "首次提交");
    assert!(l[0].parents.is_empty(), "首次提交没有父");

    // 带空格和中文的路径要能完整往返。整个目录都是未跟踪时 git 折叠成
    // 一条 "有 空格/"，我们把它摊开：目录名进 untracked_dirs，
    // 里面的文件进 entries。
    std::fs::create_dir_all(dir.join("有 空格")).unwrap();
    std::fs::write(dir.join("有 空格/中 文.md"), "x\n").unwrap();
    let st = status_full(&dir).unwrap();
    assert_eq!(st.untracked_dirs, vec!["有 空格/".to_string()]);
    let f = st
        .entries
        .iter()
        .find(|e| e.path.starts_with("有 空格"))
        .unwrap_or_else(|| panic!("带空格的中文路径没解析对：{:?}", st.entries));
    assert_eq!(f.path, "有 空格/中 文.md", "目录不该出现在改动列表里");
    assert!(f.untracked);

    // 目录里的单个文件被跟踪之后，git 自己就报完整路径，不再折叠
    stage(&dir, &["有 空格/中 文.md".into()]).unwrap();
    let st = status_full(&dir).unwrap();
    assert!(st.untracked_dirs.is_empty());
    let f = st.entries.iter().find(|e| e.path.contains("中 文")).unwrap();
    assert_eq!(f.path, "有 空格/中 文.md");
    commit(&dir, "加个带空格的中文路径", false).unwrap();

    // 改名要能带出源路径
    std::fs::rename(dir.join("a.txt"), dir.join("b.txt")).unwrap();
    stage(&dir, &["a.txt".into(), "b.txt".into()]).unwrap();
    let st = status_full(&dir).unwrap();
    let r = st.entries.iter().find(|e| e.path == "b.txt").unwrap();
    assert_eq!(r.orig.as_deref(), Some("a.txt"), "改名源路径丢了：{r:?}");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// 泳道图的前提：log 必须是拓扑序 —— 任何一条提交的父，都要排在它**后面**。
///
/// 这条测试是冲着 `--topo-order` 去的。默认的提交时间序在「父子提交时间戳
/// 相同」时会把父排到子前面，图就画歪了。造仓库时刻意把所有提交压在同一个
/// 时间戳上，正是为了让默认序必然出错、而拓扑序必然正确。
#[test]
fn 提交历史必须是拓扑序() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-topo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 所有提交同一个时间戳：这样提交时间序完全无法区分先后
    let stamp = "2026-01-01T00:00:00+00:00";
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("GIT_AUTHOR_DATE", stamp)
            .env("GIT_COMMITTER_DATE", stamp)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t.t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t.t")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };

    git(&["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a"), "1").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "base"]);

    // 分出一条支线，各提交一次，再合并回来
    git(&["switch", "-q", "-c", "side"]);
    std::fs::write(dir.join("b"), "1").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "side-1"]);

    git(&["switch", "-q", "main"]);
    std::fs::write(dir.join("c"), "1").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "main-1"]);

    git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);

    let es = log_entries(&dir, 100, true, "").unwrap();
    assert!(es.len() >= 4, "应该有至少 4 条提交，实得 {}", es.len());

    // 核心断言：每条提交的父，位置都必须比它自己靠后
    let pos: std::collections::HashMap<&str, usize> = es
        .iter()
        .enumerate()
        .map(|(i, e)| (e.sha.as_str(), i))
        .collect();
    for (i, e) in es.iter().enumerate() {
        for p in &e.parents {
            if let Some(&j) = pos.get(p.as_str()) {
                assert!(
                    j > i,
                    "拓扑序被破坏：{} 的父 {} 排在了它前面（{i} vs {j}）\n完整顺序：{:?}",
                    e.subject,
                    &p[..7],
                    es.iter().map(|x| &x.subject).collect::<Vec<_>>()
                );
            }
        }
    }

    // 顺带确认合并提交确实带出了两个父，泳道图才有岔路可画
    let merge = es.iter().find(|e| e.subject == "merge side").unwrap();
    assert_eq!(merge.parents.len(), 2, "合并提交该有两个父");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// 未跟踪文件必须整份显示成新增 —— 这是 VS Code / IDEA 的一致行为，
/// 也是唯一有意义的显示：它没有「旧版本」可比，左栏本来就该是空的。
#[test]
fn 未跟踪文件的差异是整份新增() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-untracked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs/tasks")).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("docs/tasks/old.md"), "old\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    run(&dir, &["commit", "-qm", "base"]).unwrap();

    // 目录本身已被跟踪，所以新文件会以完整路径出现，不会被折叠成 "docs/tasks/"
    let rel = "docs/tasks/2026-08-27-new.md";
    std::fs::write(dir.join(rel), "# 标题\n\n第一行\n第二行\n").unwrap();

    let st = status_full(&dir).unwrap();
    let e = st.entries.iter().find(|e| e.path == rel).unwrap();
    assert!(e.untracked, "应该是一条未跟踪的文件条目：{e:?}");

    let d = diff(&dir, rel, false, true).unwrap();
    assert!(!d.truncated, "这么小的文件不该触发截断");
    let d = d.text;
    assert!(!d.trim().is_empty(), "未跟踪文件的差异不能是空的");
    assert!(d.contains("new file mode"), "应标成新增文件：{d}");
    assert!(
        d.contains("+# 标题") && d.contains("+第一行") && d.contains("+第二行"),
        "整份内容都该是新增行：{d}"
    );
    assert!(
        !d.lines().any(|l| l.starts_with('-') && !l.starts_with("---")),
        "新增文件不该有删除行：{d}"
    );

    // 空的未跟踪文件：git 退出码 0、没有输出。这是合法情形，不能报错
    let empty_rel = "docs/tasks/empty.md";
    std::fs::write(dir.join(empty_rel), "").unwrap();
    assert!(
        diff(&dir, empty_rel, false, true).is_ok(),
        "空的未跟踪文件不该报错"
    );

    // 被折叠的未跟踪目录：目录名只出现在 untracked_dirs 里，
    // 改动列表拿到的是里面的文件。真给 diff 传一个目录路径也不能炸 ——
    // 没有单文件差异可言，返回空串而不是报错。
    std::fs::create_dir_all(dir.join("brand-new/sub")).unwrap();
    std::fs::write(dir.join("brand-new/a.txt"), "x\n").unwrap();
    std::fs::write(dir.join("brand-new/sub/b.txt"), "y\n").unwrap();
    let st = status_full(&dir).unwrap();
    assert_eq!(st.untracked_dirs, vec!["brand-new/".to_string()]);
    let inside: Vec<&str> = st
        .entries
        .iter()
        .filter(|e| e.path.starts_with("brand-new/"))
        .map(|e| e.path.as_str())
        .collect();
    assert_eq!(
        inside,
        vec!["brand-new/a.txt", "brand-new/sub/b.txt"],
        "折叠的目录要摊成里面的每个文件：{:?}",
        st.entries
    );
    assert_eq!(diff(&dir, "brand-new/", false, true).unwrap(), Diff::default());

    std::fs::remove_dir_all(&dir).unwrap();
}

/// 真仓库上跑一次「被本地改动挡住的切分支」。
///
/// 上面那条纯函数测试卡的是**解析**，这条卡的是**接线** ——
/// `switch_branch` 有五个 `run(...)` 的返回点，少包一个 classify 的表现是
/// 「大部分时候给按钮，某个分支上突然给英文报错」，而纯函数测试看不见这个。
#[test]
fn 切分支被本地改动挡住时要分类而不是原样上抛() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "gitsvc-blocked-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();

    std::fs::write(dir.join("a.txt"), "main 这边的内容\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "base", false).unwrap();

    // other 分支上把同一个文件改掉并提交
    switch_branch(&dir, "other", true).unwrap();
    std::fs::write(dir.join("a.txt"), "other 这边的内容\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "other 改了 a.txt", false).unwrap();
    switch_branch(&dir, "main", false).unwrap();

    // 回到 main，在工作区里改同一个文件但不提交 —— 这时切 other 必被拒
    std::fs::write(dir.join("a.txt"), "没提交的改动\n").unwrap();
    match switch_branch(&dir, "other", false) {
        Err(Error::LocalChanges { files, raw }) => {
            assert_eq!(files, vec!["a.txt".to_string()], "挡路的文件没切对");
            assert!(!raw.is_empty(), "原话要留着，界面上「看 git 的原话」要用");
        }
        other => panic!("应该分类成 LocalChanges，实际是：{other:?}"),
    }

    // 改动提交掉之后同一次切换要成功 —— 证明上面那条不是「永远失败」
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "提交掉挡路的改动", false).unwrap();
    // 现在两边都改过同一个文件，切过去会是快进不了的普通切换（git 允许）
    switch_branch(&dir, "other", false).expect("提交之后应该切得过去");

    std::fs::remove_dir_all(&dir).ok();
}

/// 大文件的差异必须被掐在上限内，而且要如实说自己被截断了。
///
/// 这条挡的是一个实测出来的内存问题：一个 30MB 的新增文件，`git diff`
/// 原样吐 30MB，过一趟 JSON IPC 再在前端解析成行对象，堆占用涨到 126MB ——
/// 而界面最多只渲染 3000 行。
#[test]
fn 大差异要截断且如实上报() {
    let dir = std::env::temp_dir().join(format!("gitsvc-bigdiff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();

    // 造一份稳超 1MB 的未跟踪文件
    let mut body = String::new();
    while body.len() < MAX_DIFF_BYTES * 3 {
        body.push_str("这一行有点长，重复很多遍就能把差异撑过上限 0123456789\n");
    }
    std::fs::write(dir.join("big.txt"), &body).unwrap();

    let d = diff(&dir, "big.txt", false, true).unwrap();
    assert!(d.truncated, "超过上限的差异必须标成截断");
    assert!(
        d.text.len() <= MAX_DIFF_BYTES,
        "截断后不该还超上限：{} > {MAX_DIFF_BYTES}",
        d.text.len()
    );
    // 切在半行上，前端会把残行当成一条真改动显示出来
    assert!(d.text.ends_with('\n'), "必须切在完整行的边界上");
    assert!(d.text.contains("new file mode"), "开头那段该原样保留");

    // 小文件走同一条路径，不能被误报成截断
    std::fs::write(dir.join("small.txt"), "一行\n").unwrap();
    let s = diff(&dir, "small.txt", false, true).unwrap();
    assert!(!s.truncated, "小文件不该报截断");
    assert!(s.text.contains("+一行"));

    std::fs::remove_dir_all(&dir).unwrap();
}

/// 远程分支要能检出。
///
/// `git switch origin/foo` 会直接失败，必须翻译成 `--track origin/foo`。
/// 这条造一个真的「远程」（用本地目录当 remote），走完整流程。
#[test]
fn 检出远程分支要建跟踪分支() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let base = std::env::temp_dir().join(format!("gitsvc-remote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let origin = base.join("origin");
    let clone = base.join("clone");
    std::fs::create_dir_all(&origin).unwrap();

    let cfg = |d: &Path| {
        run(d, &["config", "user.email", "t@t.t"]).unwrap();
        run(d, &["config", "user.name", "t"]).unwrap();
    };
    run(&origin, &["init", "-q", "-b", "main"]).unwrap();
    cfg(&origin);
    std::fs::write(origin.join("a.txt"), "1").unwrap();
    run(&origin, &["add", "-A"]).unwrap();
    run(&origin, &["commit", "-qm", "base"]).unwrap();
    // 在 origin 上再造一条分支
    run(&origin, &["switch", "-q", "-c", "feature/x"]).unwrap();
    std::fs::write(origin.join("b.txt"), "2").unwrap();
    run(&origin, &["add", "-A"]).unwrap();
    run(&origin, &["commit", "-qm", "feature"]).unwrap();
    run(&origin, &["switch", "-q", "main"]).unwrap();

    run(
        &base,
        &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()],
    )
    .unwrap();
    cfg(&clone);

    // 克隆之后本地只有 main，feature/x 只存在于 origin/ 下
    let bs = branches(&clone).unwrap();
    assert!(
        bs.iter().any(|b| b.name == "origin/feature/x" && b.is_remote),
        "没列出远程分支：{:?}",
        bs.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
    // refs/remotes/origin/HEAD 的短名就是 "origin"，它不是分支，不能出现在列表里
    assert!(
        !bs.iter().any(|b| b.name == "origin"),
        "远程 HEAD 混进分支列表了：{:?}",
        bs.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
    assert!(
        !bs.iter().any(|b| b.name == "feature/x" && !b.is_remote),
        "本地不该已经有 feature/x"
    );

    // 关键：传全名也必须能切过去
    switch_branch(&clone, "origin/feature/x", false)
        .unwrap_or_else(|e| panic!("检出远程分支失败：{e}"));
    let st = status_full(&clone).unwrap();
    assert_eq!(st.branch, "feature/x", "应该切到了本地跟踪分支");
    assert_eq!(st.upstream, "origin/feature/x", "上游没设对");
    assert!(clone.join("b.txt").exists(), "工作区内容没跟着切过来");

    // 再切回去，然后用全名切第二次 —— 这次本地已有同名分支，不该重复新建
    switch_branch(&clone, "main", false).unwrap();
    switch_branch(&clone, "origin/feature/x", false).unwrap();
    assert_eq!(status_full(&clone).unwrap().branch, "feature/x");

    std::fs::remove_dir_all(&base).unwrap();
}

/// 不是仓库的目录必须安静地返回 None，不能报错
#[test]
fn 非仓库目录返回none() {
    let dir = std::env::temp_dir().join(format!("gitsvc-norepo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // 临时目录本身可能落在某个仓库里（少见但可能），只在确实不在仓库时断言
    if discover(std::env::temp_dir()).is_none() {
        assert!(discover(&dir).is_none());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/*
 * **跑过的 git 要真的落进控制台，而且 argv 里要带加固参数**（issue #29）。
 *
 * 这条是端到端的：走真正的 `run_capped_raw`，不是直接喂 `console::record`。
 * 上面那些单元测试测的是环本身，测不出「有没有接上」—— 把
 * `console::record(...)` 那一行从 `run_capped_raw` 里删掉，它们照样全绿。
 *
 * 判据里最要紧的是 **argv 含 `core.fsmonitor=`**：issue #29 的第二条缺口
 * 就是「界面上看不到跑的是什么」，而看得到的那份必须是**完整的**argv。
 * 只拼调用方传进来的 args 是不够的 —— 加固参数恰恰是最可能把一个
 * 正常仓库弄坏的东西，而它不在那份里。
 *
 * # **不许 `console::clear()`，也不许假设顺序**
 *
 * 那个环是全局的，而 `cargo test` 是并行的 —— 别的测试同时也在跑 git，
 * 同时也在往里写。第一版写的是「clear 之后断言 `entries()[0]` 是我的」，
 * 本地十次全绿，**到 CI 上红了**：另一条测试的 `git init` 抢在了前面。
 * （`console.rs` 里那几条单元测试专门为此抽了一个本地 `Console` 实例，
 * 而我在这条端到端测试上又踩了同一个坑。）
 *
 * 现在靠 `cwd` 认领自己那几条 —— `tmpdir` 的名字带 pid，是唯一的。
 * 而且**一条 `clear()` 都不留**：它会把别的测试写进去的东西一起抹掉，
 * 那是在给整个套件埋雷。
 *
 * 顺带记一条教训：**并行测试跑绿一次，不等于它不依赖顺序。**
 */
#[test]
fn 跑过的_git_要落进控制台并带上加固参数() {
    let dir = tmpdir("console");
    let 我的目录 = dir.to_string_lossy().into_owned();
    // 这个测试里所有的断言都靠这个闭包认领自己那几条，不靠顺序
    let 我的 = |argv_有: &str| {
        // 走界面在用的那个口（按仓库看，多窗口第 4 步）—— 全局的 `entries()` 审查时当死代码删了
        console::entries_under(&dir)
            .into_iter()
            .find(|e| e.cwd == 我的目录 && e.argv.iter().any(|a| a == argv_有))
            .unwrap_or_else(|| panic!("控制台里找不到 `{argv_有}` 那条"))
    };

    run(&dir, &["init", "-q", "-b", "main"]).unwrap();

    /*
     * **紧跟着放一个诱饵**：在另一个目录里也跑一次 `git init`。
     *
     * 这一下是在本进程内复现 CI 上那次失败 —— 别的测试并行跑着同样的
     * 命令，环里于是有两条 `init`。`entries()` 是**最新在前**，
     * 所以诱饵必须排在自己这条**后面**才顶得上第一位；
     * 放前面的话按顺序取照样能蒙对，这个诱饵就白放了（第一版就是这么写的，
     * 把认领改回「只看 argv」它照样绿）。现在取的是 `entries_under(我的目录)`，
     * 诱饵在别的目录里，顺带验了「按仓库看」真的把别的仓库滤掉了。
     */
    let 诱饵 = tmpdir("console-decoy");
    run(&诱饵, &["init", "-q", "-b", "main"]).unwrap();

    let 那条 = 我的("init");
    assert_eq!(那条.cwd, 我的目录, "认领错了 —— 拿到的是诱饵那条");
    assert_eq!(那条.code, Some(0));
    assert!(!那条.failed());
    assert_eq!(那条.argv[0], "git");
    assert!(
        那条.argv.iter().any(|a| a.starts_with("core.fsmonitor=")),
        "argv 里没有加固参数，记的是调用方那份而不是真正跑的那份：{:?}",
        那条.argv
    );

    // 失败的那条也要在，而且带着 git 的原话
    let _ = run(&dir, &["rev-parse", "没有这个引用"]);
    let 坏的 = 我的("没有这个引用");
    assert!(坏的.failed(), "失败的命令在控制台里显示成功了：{坏的:?}");
    assert!(
        !坏的.err.is_empty(),
        "失败了却没留下 git 的原话 —— 那正是这个控制台存在的理由"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&诱饵);
}

/// 一个干净的临时目录。名字带 pid —— 失败时不清理，
/// 而残留目录会让下一次 `git init` 撞上（remote 那边踩过这个坑）
fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gitsvc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 造一个「配置里挂着一条可执行脚本」的仓库。
///
/// 返回 `(仓库路径, 痕迹文件)` —— 脚本一旦被 git 执行，痕迹文件就会出现。
/// 痕迹刻意放在**仓库外面**：放里面的话它自己会变成一个未跟踪文件，
/// 后面几步的差异就跟着变了。
fn trapped_repo(name: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("gitsvc-trap-{name}-{}", std::process::id()));
    let marker = std::env::temp_dir().join(format!("gitsvc-trap-{name}-{}.痕迹", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&marker);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();

    let hook = dir.join("hook.sh");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, marker)
}

/// **打开一个别人给的仓库，不许因此执行仓库自带的脚本。**
///
/// `core.fsmonitor` 是这一族里最凶的一条：它由 `git status` 触发，
/// 而 lite-ide 在项目根一变就自动跑 status（App.svelte 里那条 effect），
/// 用户一次都不用点；会话恢复还让它每次启动都再跑一遍。
///
/// 这条测试是照着真的能打中的形状写的 —— 先在裸 git 上验证过它确实
/// 会被执行，再加的 `-c core.fsmonitor=`。
#[test]
fn 仓库自带的_fsmonitor_不许被执行() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let (dir, marker) = trapped_repo("fsmonitor");
    let hook = dir.join("hook.sh");
    run(&dir, &["config", "core.fsmonitor", hook.to_str().unwrap()]).unwrap();
    std::fs::write(dir.join("a.txt"), "x\n").unwrap();

    let _ = status(&dir);

    assert!(
        !marker.exists(),
        "git status 执行了仓库 .git/config 里挂的脚本 —— 打开一个别人的目录就等于让他在这台机器上跑代码"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&marker).ok();
}

/// issue #17 的第二个口子：`remote.<名字>.url = ext::<命令>` ——
/// fetch / push 时 git 会把那条命令当传输层**跑起来**。
///
/// **测试里必须自己先把协议放开**（`protocol.ext.allow = always`）。
/// git 2.50 默认就拒绝 ext:，不放开的话这条断言永远绿 —— 它验的会是
/// git 的默认值，而不是我们的加固。而放开这件事**恶意仓库自己就能做**，
/// 因为 `protocol.ext.allow` 可以写在仓库的 `.git/config` 里：
/// 实测不带加固时，一条 `git fetch` 就执行了仓库指定的脚本。
#[test]
fn 仓库自带的_ext_传输不许被执行() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let (dir, marker) = trapped_repo("ext");
    let hook = dir.join("hook.sh");
    // 仓库自己放开这个协议 —— 这一句正是加固要压过去的东西
    run(&dir, &["config", "protocol.ext.allow", "always"]).unwrap();
    run(&dir, &[
        "config",
        "remote.evil.url",
        &format!("ext::{}", hook.display()),
    ])
    .unwrap();

    // 走真实的拉取路径（`remote.rs` 也经过 `git_cmd`），不是通用的 run
    let cancel: crate::remote::Cancel = Default::default();
    let _ = crate::remote::fetch(&dir, "evil", &cancel, &mut |_| {});

    assert!(
        !marker.exists(),
        "fetch 执行了仓库 .git/config 里挂的脚本 —— 点一下「拉取」就等于让仓库的作者在这台机器上跑代码"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&marker).ok();
}

/// issue #15 缺口 2：提交失败要说人话。
///
/// 两档各验一次。**第三档（认不出来的）故意原样透出去** ——
/// 猜错的分类比不分类更害人。
#[test]
fn 提交失败要分档说人话() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = tmpdir("commit-classify");
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();

    // ① 暂存区空的
    match commit(&dir, "空提交", false) {
        Err(Error::NothingStaged { .. }) => {}
        other => panic!("该分成 NothingStaged，实际是：{other:?}"),
    }
    let msg = format!("{}", commit(&dir, "空提交", false).unwrap_err());
    assert!(msg.contains("暂存区是空的"), "说的还是 git 的原话：{msg}");
    assert!(msg.contains("全部暂存"), "没给出下一步该点哪儿：{msg}");

    // ② pre-commit 钩子拒绝
    std::fs::write(dir.join("a.txt"), "x\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    let hook = dir.join(".git/hooks/pre-commit");
    // 20 行噪声 + 最后一句真话 —— 验「只留最后几行」确实把真话留下了
    let mut sh = String::from("#!/bin/sh\n");
    for i in 1..=20 {
        sh.push_str(&format!("echo '通过检查 {i}' >&2\n"));
    }
    sh.push_str("echo 'lint: a.txt 第 3 行有问题' >&2\nexit 1\n");
    std::fs::write(&hook, sh).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let e = commit(&dir, "会被拒", false).unwrap_err();
    match &e {
        Error::HookRejected { .. } => {}
        other => panic!("该分成 HookRejected，实际是：{other:?}"),
    }
    let msg = format!("{e}");
    assert!(msg.contains("提交被钩子拒绝"), "没说清是谁拒的：{msg}");
    assert!(msg.contains("改动都还在"), "没告诉人代码还在：{msg}");
    assert!(
        msg.contains("lint: a.txt 第 3 行有问题"),
        "最后那句真话被截没了：{msg}"
    );
    assert!(msg.contains("前面还有"), "截断了却没说截了多少：{msg}");
    assert!(!msg.contains("通过检查 1\n"), "前面的噪声没被截掉：{msg}");
    // 完整的那份不能丢
    assert!(e.raw().contains("通过检查 1"), "raw() 里也没有完整输出");

    // ③ **暂存区空 + 会说话的钩子** —— grok review 逮到的那条。
    // husky / lint-staged 成功时也往 stderr 打招呼，只看 stderr 就会
    // 把「一个文件都没勾」报成「钩子拒绝了」，而钩子明明通过了
    let dir3 = tmpdir("commit-classify-3");
    run(&dir3, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir3, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir3, &["config", "user.name", "t"]).unwrap();
    let hook3 = dir3.join(".git/hooks/pre-commit");
    std::fs::write(&hook3, "#!/bin/sh\necho 'husky > pre-commit' >&2\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook3, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    match commit(&dir3, "没勾文件", false) {
        Err(Error::NothingStaged { .. }) => {}
        other => panic!("会说话的钩子把「暂存区是空的」盖住了：{other:?}"),
    }

    // ④ **钩子先打招呼，git 随后失败** —— 那句 error: 在第二行，
    // 只看第一行会漏，于是又赖到钩子头上
    std::fs::write(dir3.join("b.txt"), "y\n").unwrap();
    run(&dir3, &["add", "-A"]).unwrap();
    run(&dir3, &["config", "commit.gpgsign", "true"]).unwrap();
    run(&dir3, &["config", "gpg.program", "/nonexistent-gpg"]).unwrap();
    match commit(&dir3, "签不了名", false) {
        Err(Error::Git(_)) => {}
        other => panic!("git 自己的 error: 在第二行时被赖到钩子头上：{other:?}"),
    }
    std::fs::remove_dir_all(&dir3).ok();

    // ⑥ **core.hooksPath（husky 的做法）** —— 真钩子在 .husky/，
    // 盯着 .git/hooks/ 就等于看错了地方，这一档会漏
    let dir6 = tmpdir("commit-classify-6");
    run(&dir6, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir6, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir6, &["config", "user.name", "t"]).unwrap();
    std::fs::create_dir_all(dir6.join(".husky")).unwrap();
    run(&dir6, &["config", "core.hooksPath", ".husky"]).unwrap();
    let h6 = dir6.join(".husky/pre-commit");
    std::fs::write(&h6, "#!/bin/sh\necho 'lint 没过' >&2\nexit 1\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&h6, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(dir6.join("c.txt"), "z\n").unwrap();
    run(&dir6, &["add", "-A"]).unwrap();
    match commit(&dir6, "会被 husky 拒", false) {
        Err(Error::HookRejected { output }) => {
            assert!(output.contains("lint 没过"), "钩子的话没带出来：{output}")
        }
        other => panic!("core.hooksPath 下的钩子没认出来：{other:?}"),
    }
    std::fs::remove_dir_all(&dir6).ok();

    // ⑦ **工作树** —— .git 是文件不是目录，朴素拼 .git/hooks/ 那条路径
    // 根本不存在，于是工作树里这一档永远进不去
    let dir7 = tmpdir("commit-classify-7");
    run(&dir7, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir7, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir7, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir7.join("a.txt"), "x\n").unwrap();
    run(&dir7, &["add", "-A"]).unwrap();
    commit(&dir7, "首次提交", false).unwrap();
    let h7 = dir7.join(".git/hooks/pre-commit");
    std::fs::write(&h7, "#!/bin/sh\necho '工作树里也该认出来' >&2\nexit 1\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&h7, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let wt = dir7.join("../wt-classify-7");
    run(&dir7, &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wtb"]).unwrap();
    assert!(wt.join(".git").is_file(), "工作树的 .git 该是文件");
    std::fs::write(wt.join("b.txt"), "y\n").unwrap();
    run(&wt, &["add", "-A"]).unwrap();
    match commit(&wt, "工作树里提交", false) {
        Err(Error::HookRejected { output }) => {
            assert!(output.contains("工作树里也该认出来"), "钩子的话没带出来：{output}")
        }
        other => panic!("工作树里的钩子没认出来：{other:?}"),
    }
    std::fs::remove_dir_all(&wt).ok();
    std::fs::remove_dir_all(&dir7).ok();

    // ⑤ **钩子在，但失败原因跟钩子无关** —— 不许赖到钩子头上。
    // `--amend` 在还没有提交的仓库上报 `fatal: You have nothing to amend.`，
    // 形状（stdout 空、stderr 有话）和钩子拒绝一模一样
    let dir2 = tmpdir("commit-classify-2");
    run(&dir2, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir2, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir2, &["config", "user.name", "t"]).unwrap();
    let hook2 = dir2.join(".git/hooks/pre-commit");
    std::fs::write(&hook2, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook2, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    match commit(&dir2, "改上一条", true) {
        Err(Error::Git(m)) => assert!(m.contains("nothing to amend"), "原话没留住：{m}"),
        other => panic!("git 自己的 fatal 被赖到钩子头上了：{other:?}"),
    }
    std::fs::remove_dir_all(&dir2).ok();

    std::fs::remove_dir_all(&dir).ok();
}

/// issue #17 的第一个口子：`filter.<名字>.smudge` —— **检出时**跑。
///
/// 这一条不能像别的加固那样一句 `-c` 压过去：驱动名是任意的，
/// 点不着的键关不掉。所以 `git_cmd` 会先查一次仓库**自己带的**驱动名
/// （`--local`，用户全局那份里的 git-lfs 不碰），再逐个关。
#[test]
fn 仓库自带的_filter_不许被执行() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let (dir, marker) = trapped_repo("filter");
    let hook = dir.join("hook.sh");

    std::fs::write(dir.join("a.txt"), "内容\n").unwrap();
    // 一句 .gitattributes 就把驱动挂到这个文件上
    std::fs::write(dir.join(".gitattributes"), "a.txt filter=随便什么名字\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次提交", false).unwrap();

    // **配置要在建仓库之后写** —— 这正好也验到了缓存的 key 带 mtime：
    // 只按路径缓存的话，这次写入之后那份空名单还会被用上，测试就假绿了
    run(&dir, &[
        "config",
        "filter.随便什么名字.smudge",
        hook.to_str().unwrap(),
    ])
    .unwrap();

    // 触发检出：把文件删掉再让 git 写回来
    std::fs::remove_file(dir.join("a.txt")).unwrap();
    let _ = run(&dir, &["checkout", "--", "a.txt"]);

    assert!(
        !marker.exists(),
        "检出执行了仓库 .git/config 里挂的 smudge 脚本 —— 切一下分支就等于让仓库的作者在这台机器上跑代码"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&marker).ok();
}

/// 同一族的另一半：看差异不许执行仓库自带的脚本。
///
/// 四条产生差异的路要一起验 —— `--no-ext-diff` 当初**只写在其中两条上**，
/// 而 `--no-textconv` 一条都没有。挨个点一遍才发现漏了哪几个。
#[test]
fn 仓库自带的_diff_驱动不许被执行() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let (dir, marker) = trapped_repo("diff");
    let hook = dir.join("hook.sh");
    let hook_s = hook.to_str().unwrap().to_string();
    run(&dir, &["config", "diff.external", &hook_s]).unwrap();
    run(&dir, &["config", "diff.ev.textconv", &hook_s]).unwrap();
    // 一句 .gitattributes 就够把 textconv 挂上去。
    // **逐个文件写，不写 `*`** —— 写 `*` 时实测第一步打不中（git 对通配
    // 属性的处理和显式路径不一样），于是那一步会变成一条永远绿的断言
    std::fs::write(dir.join(".gitattributes"), "a.txt diff=ev\nb.txt diff=ev\n").unwrap();

    std::fs::write(dir.join("a.txt"), "第一版\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次提交", false).unwrap();
    let first = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
    std::fs::write(dir.join("a.txt"), "第二版\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "第二次提交", false).unwrap();
    let second = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // ① 工作区里已跟踪文件的差异
    std::fs::write(dir.join("a.txt"), "改了一行\n").unwrap();
    let _ = diff(&dir, "a.txt", false, false);
    assert!(!marker.exists(), "已跟踪文件的差异执行了仓库自带的脚本（textconv 那条路）");

    // ② 未跟踪文件走的是 diff --no-index，另一条路
    std::fs::write(dir.join("b.txt"), "全新的\n").unwrap();
    let _ = diff(&dir, "b.txt", false, true);
    assert!(!marker.exists(), "未跟踪文件的 --no-index 差异执行了仓库自带的脚本");

    // ③ 历史提交的差异（有父提交，走主路径）
    let _ = commit_diff(&dir, &second, "a.txt");
    assert!(!marker.exists(), "历史提交的差异执行了仓库自带的脚本");

    // ④ 首次提交没有父，`sha^!` 会失败而回退到 git show —— 那条当初完全没设防
    let _ = commit_diff(&dir, &first, "a.txt");
    assert!(!marker.exists(), "首次提交的 show 回退执行了仓库自带的脚本");

    // ⑤ **忘了 DIFF_SAFE 的新入口必须当场炸，不能悄悄拿到一份空差异。**
    //
    // `-c diff.external=` 让 git 去执行一条空命令，于是这种调用直接失败。
    // 这是有意的 fail-closed，理由见 HARDENING 的注释。
    // 2026-09-07 拿真仓库验收时正是从这条错误消息上发现这个行为的。
    let e = run_raw(&dir, &["diff", "--", "a.txt"]).expect_err("忘了 DIFF_SAFE 就该失败");
    assert!(
        format!("{e}").contains("cannot run"),
        "失败的理由要能看出是外部 diff 驱动，实得：{e}"
    );
    assert!(!marker.exists(), "忘了 DIFF_SAFE 竟然把仓库自带的脚本跑了");

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&marker).ok();
}

/// **输出本该有界的命令，超上限要报错，不能闷头收下。**
///
/// `run_raw` 原来用 `.output()` —— 把整份 stdout 全缓冲进内存，没有任何上限。
/// 这是 AGENTS.md 那条「跑子进程读它 stdout，先问一句有没有上限」
/// 第三次漏在同一个形状上。
///
/// 上限可注入正是为了这条测试：真造一个 4MB 输出的仓库不现实，
/// 而不测的话，把这道闸删掉所有测试照样绿。
#[test]
fn 输出超过上限的命令要报错而不是给一份半截的() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = tmpdir("cap-run");
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("a.txt"), "x\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次提交", false).unwrap();

    // ① 接线：走真正的 `run_raw`（用真正的 MAX_STDOUT_BYTES）。
    //
    // 这一段是有代价的 —— 要真造一份 5MB 的输出。**但省不掉**：
    // 只测可注入上限的那个版本，把 `run_raw` 改回 `.output()` 之后
    // 测试照样绿（试过），那就成了一条测不到接线的断言。
    let big = "这一行是用来把输出撑到 4MB 以上的噪声\n".repeat(100_000);
    assert!(big.len() > MAX_STDOUT_BYTES, "造出来的得比上限大");
    std::fs::write(dir.join("big.txt"), &big).unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "一个大文件", false).unwrap();

    let e = run_raw(&dir, &["show", "HEAD:big.txt"]).expect_err("超上限必须报错");
    let msg = format!("{e}");
    assert!(
        msg.contains("输出超过"),
        "报错要说清是被上限拦下的，实得：{msg}"
    );

    // ② 上限本身：可注入的版本，两边界各验一次
    let args = ["for-each-ref", "--format=%(refname)"];
    assert!(
        run_raw_capped(&dir, &args, 16 << 10).is_ok(),
        "正常大小不该被拦"
    );
    assert!(
        run_raw_capped(&dir, &args, 4).is_err(),
        "4 字节的上限必须拦下分支列表"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// **status 的输出被掐断时：标 `truncated`，而且不许留下半截路径。**
///
/// status 不能像别的命令那样超上限就报错 —— 改动多是仓库的正常状态，
/// 把整块 Git 功能变成一条报错，比少列几条改动糟得多。所以它截断，
/// 而截断必须说出来，且末尾那条半截记录必须丢掉：
/// 留着就是改动列表里一个**看着像真的、其实点不开**的文件名。
#[test]
fn status_被掐断时要标出来且不留半截路径() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = tmpdir("cap-status");
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    // 名字取长一点，好让「掐在半条路径上」真的发生
    let names: Vec<String> = (0..40)
        .map(|i| format!("一个名字相当长的未跟踪文件-{i:02}.txt"))
        .collect();
    for n in &names {
        std::fs::write(dir.join(n), "x\n").unwrap();
    }

    let full = status_capped(&dir, MAX_STDOUT_BYTES).unwrap();
    assert!(!full.truncated, "这点输出不该被截断");
    assert_eq!(full.entries.len(), 40);

    let cut = status_capped(&dir, 512).unwrap();
    assert!(cut.truncated, "掐断了必须标 truncated");
    assert!(cut.entries.len() < 40, "掐断了条目就该变少");
    for e in &cut.entries {
        assert!(
            names.contains(&e.path),
            "留下了一条半截路径：{:?} —— 它在改动列表里点不开",
            e.path
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// **钩子话多，不能把提交挂住。**
///
/// 这条是并发排空 stderr（原来的 `drain_stderr`，2026-10-10 收进 `procutil`）存在的全部理由，而且是**真的挂过**：
/// 第一版把 `run_drained`（现在的 `drain_both`）写成「先把 stdout 读完，再顺序读 stderr」，
/// 跑这条测试时 `git commit` 和测试进程互相等着，最后是手动 kill 掉的。
///
/// 根因是一个反直觉的事实：**git 2.50 把 pre-commit 钩子的 stdout 转到了
/// stderr** —— 实测一个 200 行的钩子，git 的 stdout 只有 89 字节，
/// stderr 有 2892 字节。于是钩子一话多就写满 stderr 那几十 KB 缓冲，
/// 卡在写上；而我们在等 stdout 的 EOF，那个 EOF 要等它退出才来。
///
/// 界面上的表现是「点了提交，然后什么都不再发生」。
///
/// 卡 20 秒：修好之后实测不到 1 秒，挂住的那一版会走满 20 秒。
#[test]
fn 钩子话多不能把提交挂住() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir("noisy-hook");
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();

    // 5000 行约 150KB，稳稳超过管道那几十 KB 的缓冲
    let hook = dir.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nfor i in $(seq 1 5000); do echo \"eslint: 一切正常，这行纯属噪声 $i\"; done\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    std::fs::write(dir.join("a.txt"), "x\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let d = dir.clone();
    std::thread::spawn(move || {
        let _ = tx.send(commit(&d, "钩子话很多", false).is_ok());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(20)) {
        Ok(ok) => assert!(ok, "钩子退出码是 0，提交不该失败"),
        Err(_) => panic!("提交挂住了 —— stderr 没有被并发排空（见 procutil::spawn）"),
    }
    assert!(
        status_full(&dir).unwrap().entries.is_empty(),
        "提交应该真的发生了，工作区该是干净的"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// `drain_both` 的另一半：**stdout 超上限时留下的字节要被管住，但不算失败。**
///
/// 和 `run_raw` 那条相反 —— 那边超上限是报错（输出本该有界），
/// 这边是截断（钩子话多是正常的，报错等于把成功的提交说成失败）。
/// 所以这里断言的是 `ok` 仍为真、`truncated` 标上了、留下的字节不超 cap。
#[test]
fn drain_both_超上限只截断不报错() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = tmpdir("drain-cap");
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("a.txt"), "x\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次提交", false).unwrap();

    let args = ["for-each-ref", "--format=%(refname)"];
    let (out, truncated, _, ok) = drain_both(&dir, &args, 4).unwrap();
    assert!(ok, "截断不是失败：退出码是 0 就得报成功");
    assert!(truncated, "输出比 4 字节长，应该报截断");
    assert!(out.len() <= 4, "留下的字节要被上限管住，实得 {}", out.len());

    let (out, truncated, _, ok) = drain_both(&dir, &args, 16 << 10).unwrap();
    assert!(ok);
    assert!(!truncated, "正常大小不该报截断");
    assert!(String::from_utf8_lossy(&out).contains("refs/heads/main"));
    std::fs::remove_dir_all(&dir).ok();
}
/// HEAD 里有的给内容、没有的给 None、改了工作区不影响基线
#[test]
fn head_text_给的是提交里那份() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-head-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    // 还没有提交：HEAD 都没有，必须是 None 而不是 Err
    std::fs::write(dir.join("a.txt"), "第一版\n").unwrap();
    assert!(matches!(head_text(&dir, "a.txt"), Ok(None)), "没有 HEAD 时该是 None");
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次提交", false).unwrap();
    std::fs::write(dir.join("a.txt"), "改了\n").unwrap();
    let got = head_text(&dir, "a.txt").unwrap().expect("HEAD 里有 a.txt");
    assert_eq!(got.text, "第一版\n", "要的是 HEAD 那份，不是工作区那份");
    assert!(!got.truncated);
    assert!(matches!(head_text(&dir, "没有的.txt"), Ok(None)), "HEAD 里没有的文件该是 None");
    std::fs::remove_dir_all(&dir).ok();
}

/// push 收得进、list 看得见、pop 放得回；没改动时 push 要报错而不是假成功
#[test]
fn stash_一来一回() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-stash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("a.txt"), "第一版\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次提交", false).unwrap();

    assert!(stash_list(&dir).unwrap().is_empty(), "一开始没有 stash");
    assert!(stash_push(&dir).is_err(), "没改动时 push 要报错，git 自己这时退出码是 0");

    std::fs::write(dir.join("a.txt"), "改了\n").unwrap();
    stash_push(&dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "第一版\n", "push 之后工作区回到 HEAD");
    let list = stash_list(&dir).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].index, 0);
    assert!(list[0].message.contains("首次提交"), "标题该带上一条提交：{}", list[0].message);

    stash_pop(&dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "改了\n", "pop 之后改动回来了");
    assert!(stash_list(&dir).unwrap().is_empty(), "pop 之后 stash 该没了");
    std::fs::remove_dir_all(&dir).ok();
}

/// 提交历史里「检出到此提交」走的路：sha 不是分支，得游离检出
#[test]
fn switch_到_sha_是游离检出() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-detach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("a.txt"), "1\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "一", false).unwrap();
    let first = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
    std::fs::write(dir.join("a.txt"), "2\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "二", false).unwrap();

    switch_branch(&dir, &first, false).expect("切到 sha 该成功");
    assert_eq!(run(&dir, &["rev-parse", "HEAD"]).unwrap().trim(), first, "HEAD 该在第一次提交上");
    assert!(status_full(&dir).unwrap().detached, "该是游离状态");
    std::fs::remove_dir_all(&dir).ok();
}

/// 删分支：没合并的先被 `-d` 拦下来并分成 NotMerged，force 才真删；改名要能改
#[test]
fn 分支删除和改名() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-brdel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("a.txt"), "1\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "一", false).unwrap();
    // feat 上多一个 main 没有的提交
    switch_branch(&dir, "feat", true).unwrap();
    std::fs::write(dir.join("a.txt"), "2\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "二", false).unwrap();
    switch_branch(&dir, "main", false).unwrap();

    match branch_delete(&dir, "feat", false) {
        Err(Error::NotMerged { raw }) => assert!(raw.contains("not fully merged"), "raw 该是 git 的原话：{raw}"),
        other => panic!("没合并的分支该分成 NotMerged，实际是：{other:?}"),
    }
    assert!(branches(&dir).unwrap().iter().any(|b| b.name == "feat"), "被拦下来时分支还得在");

    branch_rename(&dir, "feat", "feat2").unwrap();
    let names: Vec<String> = branches(&dir).unwrap().into_iter().map(|b| b.name).collect();
    assert!(names.contains(&"feat2".to_string()) && !names.contains(&"feat".to_string()), "改名后：{names:?}");
    assert!(branch_rename(&dir, "feat2", "main").is_err(), "目标已存在要报错，不能盖掉");

    branch_delete(&dir, "feat2", true).unwrap();
    assert!(!branches(&dir).unwrap().iter().any(|b| b.name == "feat2"), "force 之后该没了");
    assert!(branch_delete(&dir, "main", false).is_err(), "当前分支删不掉");
    std::fs::remove_dir_all(&dir).ok();
}

/// 解析器：同一提交的连续行合成一段，不连续的不合；未提交的行是全零 sha
#[test]
fn blame_按连续行合段() {
    let a = "a".repeat(40);
    let b = "b".repeat(40);
    let z = "0".repeat(40);
    let text = format!(
        "{a} 1 1 2\nauthor 张三\nauthor-time 100\nsummary 一\n\t行1\n\
         {a} 2 2\nauthor 张三\nauthor-time 100\nsummary 一\n\t行2\n\
         {b} 1 3 1\nauthor 李四\nauthor-time 200\nsummary 二\n\t行3\n\
         {a} 3 4 1\nauthor 张三\nauthor-time 100\nsummary 一\n\t行4\n\
         {z} 5 5 1\nauthor Not Committed Yet\nauthor-time 0\nsummary Version of x\n\t行5\n"
    );
    let h = parse_blame(&text);
    assert_eq!(h.len(), 4, "1-2 一段、3 一段、4 一段（和 1-2 同提交但不连续）、5 一段：{h:?}");
    assert_eq!((h[0].start, h[0].count, h[0].author.as_str()), (1, 2, "张三"));
    assert_eq!((h[1].start, h[1].count, h[1].short.as_str()), (3, 1, "bbbbbbb"));
    assert_eq!((h[2].start, h[2].count), (4, 1));
    assert!(h[3].sha.chars().all(|c| c == '0'), "未提交的行 sha 全零");
    assert_eq!(h[0].time, 100);
    assert_eq!(h[0].summary, "一");
}

/// 真仓库跑一遍：两次提交各改一行，段数和作者都对
#[test]
fn blame_真仓库() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-blame-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "甲"]).unwrap();
    std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次", false).unwrap();
    run(&dir, &["config", "user.name", "乙"]).unwrap();
    std::fs::write(dir.join("a.txt"), "1\n二\n3\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "改第二行", false).unwrap();
    std::fs::write(dir.join("a.txt"), "1\n二\n3\n4\n").unwrap();

    let (h, truncated) = blame(&dir, "a.txt").unwrap();
    assert!(!truncated);
    let who: Vec<(u32, u32, &str)> = h.iter().map(|x| (x.start, x.count, x.author.as_str())).collect();
    assert_eq!(who, vec![(1, 1, "甲"), (2, 1, "乙"), (3, 1, "甲"), (4, 1, "Not Committed Yet")], "{h:?}");
    assert!(h[3].sha.chars().all(|c| c == '0'));
    std::fs::remove_dir_all(&dir).ok();
}

/// 按块暂存：两处改动只暂存一处，暂存区里有它、工作区两处都在；再 -R 撤掉
#[test]
fn apply_cached_只暂存一块() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-hunk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    let base: String = (1..=20).map(|i| format!("l{i}\n")).collect();
    std::fs::write(dir.join("a.txt"), &base).unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次", false).unwrap();
    // 第 2 行和第 18 行各改一处：隔得够远，git 会拆成两个 hunk
    let changed = base.replace("l2\n", "L2\n").replace("l18\n", "L18\n");
    std::fs::write(dir.join("a.txt"), &changed).unwrap();
    let d = diff(&dir, "a.txt", false, false).unwrap().text;
    assert_eq!(d.matches("\n@@").count(), 2, "该有两块：{d}");
    // 只取第一块（前端 splitHunks 做的事，这里手工拼）
    let head_end = d.find("\n@@").unwrap() + 1;
    let second = d[head_end..].find("\n@@").map(|i| head_end + i + 1).unwrap();
    let header: String = d[..head_end].lines().filter(|l| !l.starts_with("index ")).map(|l| format!("{l}\n")).collect();
    let patch = format!("{header}{}", &d[head_end..second]);

    apply_cached(&dir, &patch, false).expect("暂存第一块");
    let staged = diff(&dir, "a.txt", true, false).unwrap().text;
    assert!(staged.contains("+L2") && !staged.contains("+L18"), "暂存区只该有第 2 行那块：{staged}");
    let work = diff(&dir, "a.txt", false, false).unwrap().text;
    assert!(work.contains("+L18") && !work.contains("+L2"), "工作区相对暂存区只剩第 18 行那块：{work}");

    apply_cached(&dir, &patch, true).expect("撤掉那一块");
    let staged2 = diff(&dir, "a.txt", true, false).unwrap().text;
    assert!(staged2.trim().is_empty(), "-R 之后暂存区该干净：{staged2}");
    std::fs::remove_dir_all(&dir).ok();
}

/// 撤销一块（issue #38）：两处改动只撤第 2 行那块，盘上第 2 行回去、第 18 行还在；
/// 暂存区一个字不动 —— 它动的是工作区，不带 `--index`
#[test]
fn apply_worktree_只撤一块() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-revert-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    let base: String = (1..=20).map(|i| format!("l{i}\n")).collect();
    std::fs::write(dir.join("a.txt"), &base).unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次", false).unwrap();
    let changed = base.replace("l2\n", "L2\n").replace("l18\n", "L18\n");
    std::fs::write(dir.join("a.txt"), &changed).unwrap();
    // 先把第 18 行那块暂存起来：验「撤工作区那块不碰暂存区」
    let d = diff(&dir, "a.txt", false, false).unwrap().text;
    let head_end = d.find("\n@@").unwrap() + 1;
    let second = d[head_end..].find("\n@@").map(|i| head_end + i + 1).unwrap();
    let header: String = d[..head_end].lines().filter(|l| !l.starts_with("index ")).map(|l| format!("{l}\n")).collect();
    let first = format!("{header}{}", &d[head_end..second]);
    let second_patch = format!("{header}{}", &d[second..]);
    apply_cached(&dir, &second_patch, false).expect("暂存第 18 行那块");

    apply_worktree(&dir, &first, true).expect("撤掉第 2 行那块");
    let disk = std::fs::read_to_string(dir.join("a.txt")).unwrap();
    assert!(disk.contains("\nl2\n") && !disk.contains("L2"), "第 2 行该回去：{disk}");
    assert!(disk.contains("L18"), "第 18 行那块不该被碰：{disk}");
    let staged = diff(&dir, "a.txt", true, false).unwrap().text;
    assert!(staged.contains("+L18"), "暂存区里第 18 行那块还得在：{staged}");
    let work = diff(&dir, "a.txt", false, false).unwrap().text;
    assert!(work.trim().is_empty(), "工作区相对暂存区该干净了：{work}");
    std::fs::remove_dir_all(&dir).ok();
}

/// 「和本地比较」（issue #39）：从那次提交到现在，中间的提交和未提交的都算
#[test]
fn commit_vs_worktree_算的是到现在() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-vslocal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次", false).unwrap();
    let first = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "第二次", false).unwrap();
    std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();

    let second = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();

    let d = commit_vs_worktree(&dir, &first, "a.txt").unwrap().text;
    assert!(d.contains("+two") && d.contains("+three"), "第二次提交的和没提交的都该在：{d}");
    let d2 = commit_vs_worktree(&dir, &second, "a.txt").unwrap().text;
    assert!(!d2.contains("+two") && d2.contains("+three"), "从第二次算起只剩没提交的：{d2}");
    // 对照：commit_diff 答的是「那次提交改了什么」，第二次提交只有 two
    let own = commit_diff(&dir, &second, "a.txt").unwrap().text;
    assert!(own.contains("+two") && !own.contains("+three"), "commit_diff 只该有那次提交的：{own}");
    // 根提交：工作区脏着也只能看到首次那份（`^!` 对根提交是「和工作区比」，得先判父）
    let root_own = commit_diff(&dir, &first, "a.txt").unwrap().text;
    assert!(root_own.contains("+one") && !root_own.contains("+two") && !root_own.contains("+three"), "根提交的差异只该有 one：{root_own}");
    std::fs::remove_dir_all(&dir).ok();
}

/// cherry-pick（issue #39）：干净的搬过来；撞冲突时报错、盘上带标记、status 有冲突条目、不回滚
#[test]
fn cherry_pick_干净与冲突() {
    if !available() {
        eprintln!("跳过：机器上没有 git");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gitsvc-cherry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run(&dir, &["init", "-q", "-b", "main"]).unwrap();
    run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
    run(&dir, &["config", "user.name", "t"]).unwrap();
    std::fs::write(dir.join("a.txt"), "base\n").unwrap();
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "首次", false).unwrap();
    // 分支 feat：一条只动 b.txt 的（干净）、一条动 a.txt 的（会冲突）
    switch_branch(&dir, "feat", true).unwrap();
    std::fs::write(dir.join("b.txt"), "b\nfeat\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "feat 改 b", false).unwrap();
    let clean = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
    std::fs::write(dir.join("a.txt"), "feat 的 a\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "feat 改 a", false).unwrap();
    let conflicting = run(&dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
    // 回 main，让 a.txt 和 feat 分叉
    switch_branch(&dir, "main", false).unwrap();
    std::fs::write(dir.join("a.txt"), "main 的 a\n").unwrap();
    run(&dir, &["add", "-A"]).unwrap();
    commit(&dir, "main 改 a", false).unwrap();

    cherry_pick(&dir, &clean).expect("干净的 cherry-pick 该成功");
    assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b\nfeat\n", "改动搬过来了");
    let subj = run(&dir, &["log", "-1", "--format=%s"]).unwrap();
    assert_eq!(subj.trim(), "feat 改 b", "提交信息原样带过来");

    let err = cherry_pick(&dir, &conflicting).expect_err("撞冲突该报错");
    assert!(format!("{err}").contains("conflict"), "报的该是冲突：{err}");
    let a = std::fs::read_to_string(dir.join("a.txt")).unwrap();
    assert!(a.contains("<<<<<<<"), "盘上该带冲突标记，不回滚：{a}");
    let st = status_full(&dir).unwrap();
    assert!(st.entries.iter().any(|e| e.path == "a.txt" && e.conflicted), "status 里该有冲突条目：{:?}", st.entries);
    std::fs::remove_dir_all(&dir).ok();
}
