//! 项目文件索引与全局内容搜索。
//!
//! 分工：
//! - **文件名搜索**（⌘P / 随处搜索的文件范围）：这里只负责把路径列出来，
//!   模糊匹配放在前端做 —— 每敲一个字符都往 Rust 跑一趟的话，
//!   IPC 往返会让输入发木。几万条路径传过去也就几 MB。
//! - **内容搜索**：优先起 `rg` 子进程（多文件遍历正是它的主场，也是架构原本的规划），
//!   机器上没有 rg 就回落到进程内实现，功能不打折。

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// 不进哪些目录 —— **一次搜索开始之前算好一份，两条实现路径共用**。
///
/// # 为什么不再是一个 `fn skip_dir(name)`
///
/// 原来就是那样：名字在 `excludes` 的名单里就跳。名字判得了 `node_modules`，
/// 判不了 `dist` 和 `build` —— 那两个是常见的源码目录名（CMake 项目的
/// `build/` 里放的是构建脚本）。于是一个真叫 `build/` 的源码目录
/// **⌘P 搜不到、⇧⌘F 也搜不到**，issue #13 记的正是这个。
///
/// **名字只是怀疑，git 忽不忽略它才是证据。** 所以有争议的那几个名字
/// （[`excludes::CONTESTED_DIRS`]）要拿 git 的答案对一遍。
///
/// # 为什么是「一次算好」而不是「边走边问」
///
/// 候选目录散在树里任意深度，边走边问就是一个 Gradle 多模块仓库问出
/// 二十来次子进程。调用方在开始之前起**一次** `gitsvc::ignored_dirs`，
/// 结果传进来 —— 这个 crate 因此不认识 git，也不用依赖 gitsvc
/// （「搜索依赖 git」和「搜索依赖文件树」是同一种错箭头）。
///
/// # 两条路必须拿同一份
///
/// 装了 rg 走 rg、没装走内置实现，**结果不能不一样** —— 这是这个模块的前提。
/// 所以 `grep_rg` 传给 rg 的 `--glob !` 和内置 `walk` 用的判据都从这里出，
/// 一个字都不各写各的。
#[derive(Debug, Clone, Default)]
pub struct Skip {
    /// git 说被忽略的目录（相对 root，不带末尾 `/`）。
    /// `None` = **问不到 git**（不是仓库、git 不在、读不动）
    ignored: Option<std::collections::BTreeSet<String>>,
}

impl Skip {
    /// 没有 git 信息时的退路：**有争议的名字也一律跳**。
    ///
    /// 这条退路是有意保守的。没有证据的时候，「多列出一个 node_modules」
    /// 和「少列一个源码目录」两种错里，后者是没有提示的那个 —— 但那是
    /// **文件树**的账（树现在照列、只压暗）。搜索这边反过来：一个非 git
    /// 目录里有 20 万个 `node_modules` 文件，索引撑爆了连搜都搜不了。
    /// 所以树宽松、搜索保守，两边的代价本来就不对称。
    pub fn by_name() -> Self {
        Self { ignored: None }
    }

    /// 拿到 git 的答案。有争议的名字按这份判，确定的仍然直接跳。
    pub fn with_git(ignored: std::collections::BTreeSet<String>) -> Self {
        Self {
            ignored: Some(ignored),
        }
    }

    /// 有争议的那几个名字里，**这一次真要跳的**那些 —— 传给 rg 的 `--glob`。
    ///
    /// 确定是生成物的那几个不在这里（调用方按名字全局排除，不需要路径）。
    ///
    /// 问不到 git 时退回按名字：和 [`Self::skips`] 的那条退路必须一致，
    /// 否则装了 rg 和没装 rg 的结果又分岔了。
    pub fn rg_globs(&self) -> Vec<String> {
        match &self.ignored {
            Some(set) => set
                .iter()
                .filter(|rel| {
                    let name = rel.rsplit('/').next().unwrap_or(rel);
                    excludes::is_contested_dir(name)
                })
                .map(|rel| format!("!{rel}/**"))
                .collect(),
            None => excludes::CONTESTED_DIRS
                .iter()
                .map(|d| format!("!**/{d}/**"))
                .collect(),
        }
    }

    /// 这个目录跳不跳。`rel` 是相对项目根的路径（不带前后 `/`）。
    pub fn skips(&self, rel: &str, name: &str) -> bool {
        // 点目录一律不进（搜索侧的老规矩，树那边不适用）
        if excludes::GENERATED_DOT_DIRS.contains(&name) {
            return true;
        }
        if excludes::is_certain_generated_dir(name) {
            return true;
        }
        if !excludes::is_contested_dir(name) {
            return false;
        }
        match &self.ignored {
            // git 说它被忽略 = 证据确凿，跳
            Some(set) => set.contains(rel),
            // 问不到 git，退回按名字
            None => true,
        }
    }
}

/// 索引上限。超过这个数就停 —— 再多前端也没法有意义地展示。
pub const MAX_FILES: usize = 50_000;
/// 递归深度上限，防软链环或者病态目录结构
const MAX_DEPTH: usize = 24;

/// 递归列出项目里的文件（相对路径）。
pub fn list_files(root: impl AsRef<Path>, skip: &Skip) -> io::Result<Vec<String>> {
    Ok(list_files_and_symlinks(root, skip)?.0)
}

/// 同 [`list_files`]，另外把其中的**软链文件**单独挑一份出来。
///
/// 为什么要这一份：rg 没有「只跟软链文件」这一档 —— 默认整个跳过 symlink，
/// 而 `--follow` 会连软链**目录**一起跟进（实测 ripgrep 15.2）。内置实现
/// 只收软链文件，两条路的结果又必须一致，所以只能把软链文件挑出来
/// 显式喂给 rg。
///
/// 对 [`list_files`]（⌘P 那条路）来说这一份是顺路捎带的，不多花钱；
/// 但 `grep_rg` 自己不遍历（遍历是 rg 干的），它调这个函数就是**实打实
/// 多走一趟目录树** —— 那笔账记在调用点上。
///
/// 没有软链时第二份是空表，rg 的参数和以前一模一样。
pub fn list_files_and_symlinks(
    root: impl AsRef<Path>,
    skip: &Skip,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let root = root.as_ref();
    let mut out = Vec::new();
    let mut syms = Vec::new();
    walk(root, root, 0, skip, &mut out, &mut syms);
    out.sort();
    syms.sort();
    Ok((out, syms))
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    skip: &Skip,
    out: &mut Vec<String>,
    syms: &mut Vec<String>,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs = Vec::new();
    for ent in rd.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && name != ".env" {
            continue;
        }
        /*
         * symlink：**只跳目录，不跳文件**（issue #19）。
         *
         * 原来这里是「一律不跟进」，理由写的是「跟进就可能绕进环里」——
         * 那条理由只对软链**目录**成立。一个软链文件绕不进环里，
         * 而跳掉它的后果是：文件树里看得见、点得开、存得进去的文件，
         * 在 ⌘P 和 ⇧⌘F 里搜不到。
         * `~/.zshrc -> dotfiles/zshrc` 这种用法正是这个应用的典型场景。
         */
        let Ok(ft) = ent.file_type() else { continue };
        let is_dir = if ft.is_symlink() {
            // 跟随链接看真身是不是目录。软链本来就少，这次 stat 可以忽略
            match std::fs::metadata(ent.path()) {
                Ok(m) => m.is_dir(),
                // 断链：真身已经不在，收进来点开也是错
                Err(_) => continue,
            }
        } else {
            ft.is_dir()
        };
        if is_dir {
            // 软链目录仍然不跟进 —— 环的风险只在这一侧，判据没变
            if ft.is_symlink() {
                continue;
            }
            /*
             * 判据要的是**相对项目根的路径**，不只是名字 —— git 的答案
             * 是按路径给的（`moduleA/build` 被忽略不代表 `moduleB/build` 也是）。
             * 取不到相对路径（理论上不会，`ent` 就在 `root` 底下）时按空串走，
             * 那样有争议的名字会落到「git 说没忽略」→ 不跳，宁可多搜一点。
             */
            let rel = ent
                .path()
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !skip.skips(&rel, &name) {
                subdirs.push(ent.path());
            }
        } else if let Ok(rel) = ent.path().strip_prefix(root) {
            let rel = rel.to_string_lossy().into_owned();
            if ft.is_symlink() {
                syms.push(rel.clone());
            }
            out.push(rel);
        }
    }
    for d in subdirs {
        walk(root, &d, depth + 1, skip, out, syms);
    }
}

/// 一条内容命中。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 相对项目根的路径
    pub path: String,
    /// 1-based 行号，与编辑器显示一致
    pub line: u64,
    /// 该行内容（已截断到合理长度）
    pub text: String,
}

/// 单条命中里最多带回多少字符 —— 压缩包里的超长行会把结果面板撑垮
const MAX_HIT_LEN: usize = 400;

/// 全局内容搜索。有 rg 用 rg，没有就用进程内实现。
pub fn grep(
    root: impl AsRef<Path>,
    pattern: &str,
    limit: usize,
    skip: &Skip,
) -> io::Result<Vec<Hit>> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    match grep_rg(root.as_ref(), pattern, limit, skip) {
        Ok(hits) => Ok(hits),
        // rg 不在、版本不对、输出格式变了 —— 一律回落，不让搜索功能整个瘫掉
        Err(_) => grep_builtin(root.as_ref(), pattern, limit, skip),
    }
}

/// rg 是否可用，供界面显示当前走的哪条路
pub fn ripgrep_available() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/**
 * rg 输出的读取上限。
 *
 * 命中够数就掐掉进程，这一条只是兜底：万一 rg 吐出的一「行」大到离谱
 * （比如一个没有换行符的压缩文件），不设闸就会把它整份读进内存。
 */
const MAX_RG_BYTES: u64 = 8 << 20;

/*
 * **边读边解析，够数就掐掉 rg。**
 *
 * 原来是 `.output()` —— 先把 rg 的**全部** stdout 缓冲进内存，再截成 60 条。
 * 实测在本仓库（已排除 node_modules/target/.git）搜一个 `e`：
 * 5,663,558 字节换 60 条命中。大仓库上按倍数放大。
 *
 * 这正是 AGENTS.md 自己那条规矩漏掉的一处：「新加任何跑子进程读它 stdout
 * 的功能，先问一句：这东西的输出有上限吗」。gitsvc 有 MAX_DIFF_BYTES，
 * searchsvc 一直没有。
 */
fn grep_rg(root: &Path, pattern: &str, limit: usize, skip: &Skip) -> io::Result<Vec<Hit>> {
    use std::io::{BufRead, BufReader, Read};

    let mut cmd = Command::new("rg");
    cmd.args([
        "--json",
        "--line-number",
        "--no-heading",
        "--smart-case",
        "--max-filesize",
        "8M",
    ]);
    // **跟内置实现跳同一批目录。**
    //
    // rg 靠 `.gitignore` 跳过 node_modules 之类，但项目不一定是 git 仓库、
    // 也不一定有 `.gitignore` —— 那样「装了 rg」和「没装 rg」搜出来的结果
    // 就不一样了，这是不能接受的。所以照旧显式传 `--glob !`。
    //
    // 但**不能再一律按名字传**（issue #13）：那正是一个源码 `build/`
    // 搜不到的原因 —— rg 自己本来会搜它（`.gitignore` 里没有它），
    // 是我们那句排除通配把它挡掉的。
    //
    // 现在分两种传：确定是生成物的按名字全局排除；有争议的**按 git 给的
    // 具体路径逐条排除**，git 没说忽略的就不传，交给 rg 去搜。
    //
    // （这一段用行注释不用块注释：通配符里的 `**` 紧跟 `/` 会把块注释
    // 提前闭合，Rust 报的是「found doc comment」，第一眼看不出是注释的事。）
    for d in excludes::CERTAIN_GENERATED_DIRS
        .iter()
        .chain(excludes::GENERATED_DOT_DIRS.iter())
    {
        cmd.arg("--glob").arg(format!("!**/{d}/**"));
    }
    for g in skip.rg_globs() {
        cmd.arg("--glob").arg(g);
    }
    /*
     * `--` 之后才是路径。
     *
     * 这里是**防御性的，不是在补一个已知漏洞**：root 一路来自 `probe_path`，
     * 永远是绝对路径，开头是 `/` 而不是 `-`。之所以还是加上，是因为
     * 「路径前一律加 --」是本仓库对所有子进程调用的统一纪律，
     * 例外一多，下次真有人传相对路径进来时就没人记得这回事了。
     * （试着为它写过一条测试，发现测不出来 —— 绝对路径根本触发不了，
     * 而不会失败的测试比没有测试更糟，所以只留这段注释。）
     */
    /*
     * 软链文件要显式列给 rg。
     *
     * rg 没有「只跟软链文件」这一档：默认整个跳过 symlink，`--follow` 又会
     * 连软链**目录**一起跟进（实测 ripgrep 15.2：加了 `--follow` 之后
     * `linkdir/target.txt` 也进了结果）。而内置实现只收软链文件。
     *
     * **代价是多走一趟目录树** —— 遍历本来是 rg 在干的，这一趟是额外的。
     * 认这笔账，是因为另一头更糟：装了 rg 的人在 ⇧⌘F 里搜不到软链文件，
     * 没装 rg 的人反而搜得到，而「装没装 rg 结果要一样」是这个模块的前提
     * （见 `两条实现路径结果必须一致`）。
     */
    let syms = list_files_and_symlinks(root, skip)
        .map(|(_, s)| s)
        .unwrap_or_default();

    cmd.args(["-e", pattern]).arg("--").arg(root);
    for s in &syms {
        cmd.arg(root.join(s));
    }

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // rg 一般原样保留传入的前缀，但遇到软链时可能吐出解析后的真实路径
    // （macOS 上 /var → /private/var）。两个前缀都试，否则结果里会混进绝对路径。
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    let mut hits = Vec::new();
    let mut 掐掉了 = false;
    {
        let stdout = child.stdout.take().expect("stdout 已 piped");
        let mut reader = BufReader::new(stdout.take(MAX_RG_BYTES));
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            if let Some(h) = parse_rg_line(&line, root, &canon) {
                hits.push(h);
                if hits.len() >= limit {
                    掐掉了 = true;
                    break;
                }
            }
        }
    }
    // 够了就别让 rg 为没人要看的命中继续遍历整个仓库
    if 掐掉了 {
        let _ = child.kill();
    }

    // stderr 也要限量读：管道写满时 rg 会阻塞，而我们已经不读 stdout 了
    let mut err = Vec::new();
    if let Some(stderr) = child.stderr.as_mut() {
        let _ = stderr.take(8 << 10).read_to_end(&mut err);
    }
    let status = child.wait()?;

    // 被我们掐掉的进程退出码没有意义，不能当成失败。
    // 没掐的情况下：0 = 有命中，1 = 无命中（正常），>=2 才是真出错
    if !掐掉了 && status.code().is_some_and(|c| c >= 2) {
        return Err(io::Error::other(format!(
            "rg 执行失败：{}",
            String::from_utf8_lossy(&err).trim()
        )));
    }
    Ok(hits)
}

/// 解析 rg `--json` 的一行。不是命中、或者解不出来都返回 None。
fn parse_rg_line(line: &[u8], root: &Path, canon: &Path) -> Option<Hit> {
    if line.is_empty() {
        return None;
    }
    let v = serde_json::from_slice::<serde_json::Value>(line).ok()?;
    if v["type"] != "match" {
        return None;
    }
    let d = &v["data"];
    Some(Hit {
        path: rel(root, canon, d["path"]["text"].as_str()?),
        line: d["line_number"].as_u64()?,
        text: clip(d["lines"]["text"].as_str()?.trim_end()),
    })
}

/// 进程内回落实现：遍历索引到的文件逐个扫。
fn grep_builtin(root: &Path, pattern: &str, limit: usize, skip: &Skip) -> io::Result<Vec<Hit>> {
    let files = list_files(root, skip)?;
    let needle = pattern.as_bytes();
    let finder = memchr::memmem::Finder::new(needle);
    let mut hits = Vec::new();

    for rel_path in files {
        if hits.len() >= limit {
            break;
        }
        let full = root.join(&rel_path);
        let Ok(meta) = std::fs::metadata(&full) else {
            continue;
        };
        // 与 rg 的 --max-filesize 对齐，别把 GB 级日志卷进来
        if meta.len() > 8 << 20 {
            continue;
        }
        let Ok(bytes) = std::fs::read(&full) else {
            continue;
        };
        // NUL 字节 = 二进制，跳过
        if memchr::memchr(0, &bytes[..bytes.len().min(4096)]).is_some() {
            continue;
        }
        let mut start = 0usize;
        for (idx, nl) in memchr::memchr_iter(b'\n', &bytes).enumerate() {
            if finder.find(&bytes[start..nl]).is_some() {
                hits.push(Hit {
                    path: rel_path.clone(),
                    // 行号 1-based，与编辑器显示一致
                    line: idx as u64 + 1,
                    text: clip(String::from_utf8_lossy(&bytes[start..nl]).trim_end()),
                });
                if hits.len() >= limit {
                    break;
                }
            }
            start = nl + 1;
        }
    }
    Ok(hits)
}

fn rel(root: &Path, canon: &Path, path: &str) -> String {
    let p = Path::new(path);
    p.strip_prefix(root)
        .or_else(|_| p.strip_prefix(canon))
        .map(|r| r.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

fn clip(s: &str) -> String {
    if s.chars().count() <= MAX_HIT_LEN {
        return s.to_string();
    }
    s.chars().take(MAX_HIT_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn sandbox(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("searchsvc-test-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("src")).unwrap();
        fs::create_dir_all(d.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(d.join(".git")).unwrap();
        fs::write(d.join("README.md"), "# 标题\n找我 needle 在这里\n").unwrap();
        fs::write(d.join("src/main.rs"), "fn main() {\n    // needle\n}\n").unwrap();
        fs::write(d.join("node_modules/pkg/index.js"), "needle in noise\n").unwrap();
        fs::write(d.join(".git/config"), "needle\n").unwrap();
        // 名单里的每一个都造一份带命中的噪声 —— 循环而不是照抄名字，
        // 这样往 excludes 里加一个目录名，跳过它这件事自动就被验到了
        for name in excludes::GENERATED_DIRS {
            fs::create_dir_all(d.join(name)).unwrap();
            fs::write(d.join(name).join("noise.txt"), "needle in noise\n").unwrap();
        }
        /*
         * issue #19 的形状。
         *
         * 真身**故意放在项目外**（`~/.zshrc -> dotfiles/zshrc` 那种用法）：
         * 放项目内的话，命中会经由真身自己的路径被搜到，断言就分不清
         * 「软链被收了」还是「真身被收了」—— 那样的测试改回坏代码也不会红。
         *
         * 名字带 name 后缀，每次 sandbox 覆盖同一份，不会在 temp 里越攒越多。
         */
        let outside = std::env::temp_dir().join(format!("searchsvc-outside-{name}.txt"));
        fs::write(&outside, "needle 在项目外的真身里\n").unwrap();
        let link = d.join("link.txt");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        // 软链目录：**不该**被跟进，环的风险只在这一侧
        std::os::unix::fs::symlink(d.join("src"), d.join("linkdir")).unwrap();
        d
    }

    /*
     * 够数就掐掉 rg：命中远多于 limit 时只要 limit 条，而且**不能报错**。
     *
     * 被我们 kill 掉的进程退出码没有意义 —— 原来是 `.output()` 全缓冲，
     * 没有这个问题也没有这道防线；改成边读边掐之后，「掐了算不算失败」
     * 就成了一个必须钉住的判断（gitsvc 那边踩过同一个坑）。
     */
    #[test]
    fn rg_命中够数就停下且不算失败() {
        if !ripgrep_available() {
            return; // 机器上没有 rg，这条不适用
        }
        let d = std::env::temp_dir().join("searchsvc-test-cap");
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        // 300 行全是命中，只要 3 条
        let body = "needle\n".repeat(300);
        fs::write(d.join("many.txt"), &body).unwrap();

        let hits = grep_rg(&d, "needle", 3, &Skip::by_name()).expect("掐掉子进程不能被当成失败");
        assert_eq!(hits.len(), 3, "要几条给几条");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn rg_单行解析() {
        let root = Path::new("/proj");
        let one = br#"{"type":"match","data":{"path":{"text":"/proj/src/a.rs"},"line_number":7,"lines":{"text":"  hit here\n"}}}"#;
        let h = parse_rg_line(one, root, root).expect("这是一条命中");
        assert_eq!(h.path, "src/a.rs", "路径要转成相对根的");
        assert_eq!(h.line, 7);
        assert_eq!(h.text, "  hit here", "行尾换行要去掉，行首缩进要留着");

        // 不是命中的、坏的、空的，一律 None，不能 panic
        assert!(parse_rg_line(br#"{"type":"begin","data":{}}"#, root, root).is_none());
        assert!(parse_rg_line(b"{ this is not json", root, root).is_none());
        assert!(parse_rg_line(b"", root, root).is_none());
        assert!(
            parse_rg_line(br#"{"type":"match","data":{"path":{"text":"/proj/a"}}}"#, root, root)
                .is_none(),
            "缺字段的命中要当没有，不能 unwrap 崩掉"
        );
    }

    #[test]
    fn 索引跳过噪声目录() {
        let d = sandbox("list");
        let files = list_files(&d, &Skip::by_name()).unwrap();
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"src/main.rs".to_string()));
        for name in excludes::GENERATED_DIRS {
            assert!(
                !files.iter().any(|f| f.contains(name)),
                "{name} 不该进索引：{files:?}"
            );
        }
        assert!(
            !files.iter().any(|f| f.contains(".git")),
            "点目录不该进索引"
        );
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 内置实现能搜到内容且同样跳过噪声() {
        let d = sandbox("builtin");
        let hits = grep_builtin(&d, "needle", 50, &Skip::by_name()).unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert!(paths.contains(&"README.md"));
        assert!(paths.contains(&"src/main.rs"));
        for name in excludes::GENERATED_DIRS {
            assert!(!paths.iter().any(|p| p.contains(name)), "{name} 不该被搜到");
        }
        // 行号必须是 1-based，与编辑器一致
        let readme = hits.iter().find(|h| h.path == "README.md").unwrap();
        assert_eq!(readme.line, 2);
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 无命中返回空() {
        let d = sandbox("empty");
        assert!(grep_builtin(&d, "绝不存在的词", 50, &Skip::by_name()).unwrap().is_empty());
        assert!(
            grep(&d, "", 50, &Skip::by_name()).unwrap().is_empty(),
            "空 pattern 不该扫全项目"
        );
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn grep_入口在有无_rg_时都能工作() {
        let d = sandbox("entry");
        let hits = grep(&d, "needle", 50, &Skip::by_name()).unwrap();
        assert!(!hits.is_empty(), "无论走 rg 还是回落，都该有命中");
        for name in excludes::GENERATED_DIRS {
            assert!(hits.iter().all(|h| !h.path.contains(name)), "{name} 不该被搜到");
        }
        fs::remove_dir_all(d).ok();
    }

    /// 装没装 rg 都该搜出同一批结果 —— 这个不变量真的破过：
    /// 内置实现靠自己那份名单跳过 node_modules，而 rg 靠 .gitignore，
    /// 项目没有 .gitignore 时两边就分岔了。
    /// issue #19：软链文件在文件树里看得见、点得开、存得进去，
    /// 却在 ⌘P 和 ⇧⌘F 里搜不到。
    ///
    /// 验过红：把 `walk` 里那段改回「`if ft.is_symlink() { continue }`」，
    /// 第一条断言立刻失败。
    #[test]
    fn 软链文件要进索引_软链目录不跟进() {
        let d = sandbox("symlink");
        let (files, syms) = list_files_and_symlinks(&d, &Skip::by_name()).unwrap();

        assert!(
            files.iter().any(|f| f == "link.txt"),
            "软链文件没进索引，⌘P 就搜不到它：{files:?}"
        );
        assert!(
            syms.iter().any(|s| s == "link.txt"),
            "软链文件没被挑进给 rg 的那份名单：{syms:?}"
        );
        // linkdir -> src，src 里有 main.rs。跟进的话会冒出 linkdir/main.rs
        assert!(
            !files.iter().any(|f| f.starts_with("linkdir")),
            "软链目录被跟进了，环的风险就回来了：{files:?}"
        );

        fs::remove_dir_all(d).ok();
    }

    /// 软链文件的**内容**也要搜得到，而且两条路都要。
    ///
    /// rg 那半边单独会坏：它默认整个跳过 symlink，光改内置实现的话，
    /// 装了 rg 的人搜不到、没装的人反而搜得到。
    #[test]
    fn 软链文件的内容两条路都要搜得到() {
        let d = sandbox("symgrep");

        let hits = grep_builtin(&d, "项目外的真身", 50, &Skip::by_name()).unwrap();
        assert!(
            hits.iter().any(|h| h.path == "link.txt"),
            "内置实现没搜到软链文件：{hits:?}"
        );

        if ripgrep_available() {
            let hits = grep_rg(&d, "项目外的真身", 50, &Skip::by_name()).unwrap();
            assert!(
                hits.iter().any(|h| h.path == "link.txt"),
                "rg 没搜到软链文件：{hits:?}"
            );
        } else {
            eprintln!("跳过 rg 那半边：机器上没有 rg");
        }

        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 两条实现路径结果必须一致() {
        if !ripgrep_available() {
            eprintln!("跳过：机器上没有 rg");
            return;
        }
        let d = sandbox("parity");
        let mut a: Vec<(String, u64)> = grep_rg(&d, "needle", 100, &Skip::by_name())
            .unwrap()
            .into_iter()
            .map(|h| (h.path, h.line))
            .collect();
        let mut b: Vec<(String, u64)> = grep_builtin(&d, "needle", 100, &Skip::by_name())
            .unwrap()
            .into_iter()
            .map(|h| (h.path, h.line))
            .collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "rg 与内置实现搜出的结果不一致");
        fs::remove_dir_all(d).ok();
    }

    /// 造一个**真的 git 仓库**：`build/` 是源码（提交进去的），`dist/` 被忽略。
    ///
    /// 不用 mock —— `.gitignore` 的规则以 git 为准，自己实现一份就是在
    /// 猜（这条判据和「起 git 子进程而不是自己解析 .gitignore」是同一条）。
    /// 机器上没有 git 时整条测试跳过：宁可少测一条，也不要一条会随环境
    /// 时红时绿的测试。
    fn git_sandbox(name: &str) -> Option<PathBuf> {
        let d = std::env::temp_dir().join(format!("searchsvc-git-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("build")).unwrap();
        fs::create_dir_all(d.join("dist")).unwrap();
        fs::create_dir_all(d.join("node_modules")).unwrap();
        // build/ 是源码：CMake 项目把构建脚本放这儿，而且**提交进仓库**
        fs::write(d.join("build/toolchain.cmake"), "set(NEEDLE 1)\n").unwrap();
        // dist/ 是产物，写进 .gitignore
        fs::write(d.join("dist/bundle.js"), "var NEEDLE=1\n").unwrap();
        fs::write(d.join("node_modules/noise.txt"), "NEEDLE\n").unwrap();
        fs::write(d.join(".gitignore"), "dist/\nnode_modules/\n").unwrap();

        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        if !git(&["init", "-q"]) {
            let _ = fs::remove_dir_all(&d);
            return None;
        }
        git(&["add", "-A"]);
        Some(d)
    }

    /*
     * **issue #13 的正题：名字叫 build 不等于它是生成物。**
     *
     * 改回「一律按名字跳」的话这条会红 —— `build/toolchain.cmake` 搜不到。
     * 三条一起断言，因为它们要的是同一件事的三个面：
     * 该搜的搜得到、该跳的跳掉了、两条实现路径给同一个答案。
     */
    #[test]
    fn 源码目录叫build也要搜得到_而被忽略的dist不搜() {
        let Some(d) = git_sandbox("contested") else {
            eprintln!("跳过：机器上没有可用的 git");
            return;
        };
        let ignored = 假装问过git(&d);
        let skip = Skip::with_git(ignored);

        let paths = |hits: Vec<Hit>| {
            let mut v: Vec<String> = hits.into_iter().map(|h| h.path).collect();
            v.sort();
            v
        };

        let 内置 = paths(grep_builtin(&d, "NEEDLE", 50, &skip).unwrap());
        assert!(
            内置.iter().any(|p| p.contains("toolchain.cmake")),
            "被跟踪的 build/ 是源码，必须搜得到 —— 实得 {内置:?}"
        );
        assert!(
            !内置.iter().any(|p| p.contains("bundle.js")),
            "dist/ 在 .gitignore 里，不该搜 —— 实得 {内置:?}"
        );
        assert!(
            !内置.iter().any(|p| p.contains("node_modules")),
            "node_modules 任何时候都不该搜 —— 实得 {内置:?}"
        );

        // 索引（⌘P 那条路）同理
        let files = list_files(&d, &skip).unwrap();
        assert!(files.iter().any(|f| f.contains("toolchain.cmake")), "⌘P 也要找得到");
        assert!(!files.iter().any(|f| f.contains("bundle.js")));

        // 两条路必须一致 —— 这是这个模块的前提
        if ripgrep_available() {
            assert_eq!(
                paths(grep_rg(&d, "NEEDLE", 50, &skip).unwrap()),
                内置,
                "装了 rg 和没装 rg 搜出来的结果不一样了"
            );
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// 真去问一次 git。放在这儿而不是引 gitsvc —— searchsvc 不认识 git，
    /// 那条依赖箭头是错的；测试里现起一次就够了。
    fn 假装问过git(d: &Path) -> std::collections::BTreeSet<String> {
        let out = Command::new("git")
            .args(["ls-files", "--others", "--ignored", "--directory", "--exclude-standard", "-z"])
            .current_dir(d)
            .output()
            .expect("git 跑不起来");
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter_map(|p| p.strip_suffix('/').map(str::to_owned))
            .collect()
    }

    #[test]
    fn 命中行被截断() {
        let d = sandbox("clip");
        let long = format!("needle{}\n", "x".repeat(2000));
        fs::write(d.join("long.txt"), &long).unwrap();
        let hits = grep_builtin(&d, "needle", 50, &Skip::by_name()).unwrap();
        let h = hits.iter().find(|h| h.path == "long.txt").unwrap();
        assert!(h.text.chars().count() <= MAX_HIT_LEN);
        fs::remove_dir_all(d).ok();
    }
}
