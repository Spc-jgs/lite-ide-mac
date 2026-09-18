//! 仓库信任（issue #24）：开仓库之前先看 `.git/config` 里有没有会执行命令的键。
//!
//! # 威胁模型（2026-09-17 实测，JOURNAL 同日）
//!
//! `git clone` **不传输** `.git/config` 和 `.git/hooks` —— 它们是本机 git 克隆时新建的。
//! 实测：源仓库里塞了 `filter.x.clean` / `core.fsmonitor` / `alias` / `post-checkout` 钩子，
//! 克隆出来的 config 只有 remote 和分支跟踪，hooks 目录里只有 `.sample`，
//! 在克隆库上 status / checkout 一个都没触发。所以别人写的 config 只从两条路来：
//! 整个目录连 `.git` 一起拿到手（zip、U 盘、AirDrop），或本机上别的东西往里写
//! （脚本、agent、husky 那类工具）。两条路的共同点是**内容不是用户自己敲的，
//! 应用又分不出来** —— 所以判据不能是「这个目录信不信」（第二条路目录是自己的），
//! 只能是「这份 config 里有没有会执行命令的键」。
//!
//! # 为什么是白名单
//!
//! `lib.rs` 的 `HARDENING` 是黑名单：想到一个 `-c` 关一个。写这个模块之前又实测了
//! 一轮，五分钟找出两条黑名单外的：`gpg.program` + `commit.gpgsign=true` 在 `git commit`
//! 时执行、`merge.<driver>.driver` 在合并冲突时执行。黑名单永远差一条，而且差的那条
//! 无声；白名单差的那条只是多问一次（lfs 仓库第一次开会被问）。两种代价不对称。
//! 黑名单照留：白名单挡的是「跑不跑 git」，`-c` 挡的是「跑了之后 git 自己会不会去执行」。
//!
//! # 扫描本身只跑 `git config --list`
//!
//! 实测它不碰 `core.pager` / `core.fsmonitor` / `alias`（全是它会读到的键）—— 它只读
//! 配置，不执行配置。用它而不是自己解析 INI，是因为 `include.path` / `includeIf` /
//! 工作树的 `.git` 文件 / `extensions.worktreeConfig` 这些 git 自己都会解析对，
//! 自己写解析器每一条都是一个漏。受限态下这是唯一会起的 git 进程。
//!
//! # 指纹就是内容本身
//!
//! 信任记的是 (仓库根, 规范化后的 `key=value` 列表)。config 一变列表就变、信任作废。
//! 不做哈希：列表几百字节，存下来直接比对，零依赖、零碰撞。
use std::path::Path;

use crate::{run_raw, R};

/// 一条白名单之外的配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suspect {
    pub key: String,
    pub value: String,
    /// `file:.git/config` 那种；被 `include` 进来的会指向别的文件，这正是要给人看的
    pub origin: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub suspects: Vec<Suspect>,
    /// `.git/hooks` 里不带 `.sample` 的可执行文件名。只列出来知情，不算受限：
    /// 提交跑 pre-commit 是这个应用本来就依赖的（issue #15）
    pub hooks: Vec<String>,
    /// 规范化后的整份配置，信任按它记
    pub fingerprint: String,
}

/// 一条 `git config --list --show-origin -z` 的记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub origin: String,
    pub key: String,
    pub value: String,
}

/// `-z --show-origin` 的输出：`origin\0key\nvalue\0` 交替。没有 `\n` 的记录是没有值的布尔键
pub fn parse_list(raw: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut it = raw.split(|b| *b == 0);
    while let (Some(origin), Some(kv)) = (it.next(), it.next()) {
        let origin = String::from_utf8_lossy(origin).into_owned();
        let kv = String::from_utf8_lossy(kv);
        let (key, value) = match kv.split_once('\n') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (kv.to_string(), String::new()),
        };
        if key.is_empty() {
            continue;
        }
        out.push(Entry { origin, key, value });
    }
    out
}

/// 拆 `section.subsection.name`：subsection 里可以有点（`remote.my.remote.url`），
/// 所以 section 取第一段、name 取最后一段、中间全是 subsection。
fn split_key(key: &str) -> (String, Option<&str>, String) {
    let first = key.find('.');
    let last = key.rfind('.');
    match (first, last) {
        (Some(f), Some(l)) if f < l => (
            key[..f].to_ascii_lowercase(),
            Some(&key[f + 1..l]),
            key[l + 1..].to_ascii_lowercase(),
        ),
        (Some(f), _) => (key[..f].to_ascii_lowercase(), None, key[f + 1..].to_ascii_lowercase()),
        _ => (key.to_ascii_lowercase(), None, String::new()),
    }
}

/// remote 的 url：只认明文传输。`ext::` `fd::` 以及任何 `xxx::`（远程助手 `git-remote-xxx`）
/// 都是「把一个命令当传输层」，一律可疑
fn url_is_plain(v: &str) -> bool {
    let v = v.trim();
    if v.is_empty() {
        return true;
    }
    let lower = v.to_ascii_lowercase();
    for scheme in ["https://", "http://", "ssh://", "git://", "file://"] {
        if lower.starts_with(scheme) {
            return true;
        }
    }
    // 远程助手 / ext / fd：`name::rest`
    if let Some(i) = v.find("::") {
        if !v[..i].contains('/') {
            return false;
        }
    }
    if v.contains("://") {
        return false;
    }
    // scp 式 `user@host:path`、本地路径
    true
}

/// 这个键 git 只拿它当数据，不当命令。**不在这张表里的一律可疑** —— 这是整个模块的全部逻辑。
pub fn is_inert(key: &str, value: &str) -> bool {
    let (section, sub, name) = split_key(key);
    let n = name.as_str();
    match section.as_str() {
        "core" if sub.is_none() => matches!(
            n,
            "repositoryformatversion"
                | "filemode"
                | "bare"
                | "logallrefupdates"
                | "ignorecase"
                | "precomposeunicode"
                | "symlinks"
                | "autocrlf"
                | "safecrlf"
                | "eol"
                | "worktree"
                | "sparsecheckout"
                | "sparsecheckoutcone"
                | "quotepath"
                | "commitgraph"
                | "compression"
                | "bigfilethreshold"
                | "hidedotfiles"
                | "excludesfile"
                | "attributesfile"
                | "packedgitlimit"
                | "packedgitwindowsize"
                | "deltabasecachelimit"
                | "untrackedcache"
                | "splitindex"
                | "abbrev"
                | "warnambiguousrefs"
        ),
        "remote" if sub.is_some() => match n {
            "url" | "pushurl" => url_is_plain(value),
            "fetch" | "push" | "tagopt" | "prune" | "prunetags" | "mirror" | "skipdefaultupdate"
            | "skipfetchall" | "promisor" | "partialclonefilter" => true,
            _ => false,
        },
        "branch" if sub.is_some() => {
            matches!(n, "remote" | "merge" | "rebase" | "pushremote" | "description")
        }
        "user" if sub.is_none() => matches!(n, "name" | "email" | "signingkey" | "usernameconfig"),
        "commit" if sub.is_none() => matches!(n, "gpgsign" | "cleanup" | "verbose" | "status"),
        "tag" if sub.is_none() => matches!(n, "gpgsign" | "sort" | "forcesignannotated"),
        // 整段放行的：这些段里的两段键都是行为开关，而且是这个应用会跑的子命令读的。
        // `interactive.difffilter` / `sendemail.smtpserver` 这类会跑程序的段**不在**这里
        "pull" | "push" | "fetch" | "rebase" | "init" | "status" | "log" | "color" | "column"
        | "help" | "advice" | "i18n" | "index" | "pack" | "gc" | "maintenance" | "gui" | "feature"
        | "transfer" | "checkout" | "stash" | "worktree" | "rerere"
            if sub.is_none() =>
        {
            true
        }
        "safe" if sub.is_none() => n == "directory",
        "extensions" if sub.is_none() => {
            matches!(n, "worktreeconfig" | "objectformat" | "preciousobjects" | "refstorage")
        }
        // merge / diff：三段的是驱动（`merge.<x>.driver` / `diff.<x>.textconv` 会执行），
        // 两段的也不能整段放行 —— `diff.external` 就是两段的。逐个点名
        "merge" if sub.is_none() => matches!(
            n,
            "ff" | "conflictstyle" | "autostash" | "log" | "renamelimit" | "renormalize" | "stat"
                | "verbosity" | "tool" | "guitool" | "directoryrenames" | "renames"
                | "defaulttoupstream" | "branchdesc" | "suppressdest"
        ),
        "diff" if sub.is_none() => matches!(
            n,
            "renames" | "algorithm" | "colormoved" | "colormovedws" | "mnemonicprefix" | "noprefix"
                | "context" | "interhunkcontext" | "indentheuristic" | "orderfile" | "statgraphwidth"
                | "submodule" | "wserrorhighlight" | "relative" | "ignoresubmodules" | "tool"
                | "guitool" | "autorefreshindex" | "dirstat" | "suppressblankempty" | "srcprefix"
                | "dstprefix" | "renamelimit" | "wordregex"
        ),
        "submodule" if sub.is_some() => match n {
            "update" => !value.trim_start().starts_with('!'),
            "url" | "path" | "active" | "branch" | "ignore" | "fetchrecursesubmodules" | "recurse"
            | "shallow" => true,
            _ => false,
        },
        "submodule" => matches!(n, "active" | "recurse" | "fetchjobs" | "propagatebranches"),
        // lfs 的两段键是地址和版本；`filter.lfs.*` 那三条是命令，不归这里
        "lfs" if sub.is_none() => true,
        _ => false,
    }
}

/// 规范化：按 `key=value` 排序拼起来。origin 不进指纹 —— 同一份内容从 include 挪进主文件
/// 不该让信任作废
pub fn fingerprint(entries: &[Entry]) -> String {
    let mut lines: Vec<String> = entries.iter().map(|e| format!("{}={}", e.key, e.value)).collect();
    lines.sort();
    lines.join("\n")
}

/// 从一批记录里挑出可疑的
pub fn suspects(entries: &[Entry]) -> Vec<Suspect> {
    entries
        .iter()
        .filter(|e| !is_inert(&e.key, &e.value))
        .map(|e| Suspect { key: e.key.clone(), value: e.value.clone(), origin: e.origin.clone() })
        .collect()
}

/// 扫一个仓库。只跑两条只读的 git：`config --list` 和 `rev-parse --git-path hooks`。
pub fn scan(root: impl AsRef<Path>) -> R<Scan> {
    let root = root.as_ref();
    let raw = run_raw(root, &["config", "--list", "--local", "--show-origin", "-z"])?;
    let entries = parse_list(&raw);
    let hooks_dir = String::from_utf8_lossy(&run_raw(root, &["rev-parse", "--git-path", "hooks"])?)
        .trim()
        .to_string();
    let hooks = list_hooks(&root.join(hooks_dir));
    Ok(Scan { suspects: suspects(&entries), hooks, fingerprint: fingerprint(&entries) })
}

fn list_hooks(dir: &Path) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let mut out: Vec<String> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                !name.ends_with(".sample")
                    && e.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
            })
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: &str, value: &str) -> Entry {
        Entry { origin: "file:.git/config".into(), key: key.into(), value: value.into() }
    }

    #[test]
    fn 克隆出来的_config_一条都不可疑() {
        // 这就是 git clone 生成的那份，一字不差
        let clean = [
            ("core.repositoryformatversion", "0"),
            ("core.filemode", "true"),
            ("core.bare", "false"),
            ("core.logallrefupdates", "true"),
            ("core.ignorecase", "true"),
            ("core.precomposeunicode", "true"),
            ("remote.origin.url", "https://github.com/x/y.git"),
            ("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"),
            ("branch.main.remote", "origin"),
            ("branch.main.merge", "refs/heads/main"),
            ("user.name", "x"),
            ("user.email", "x@y"),
            ("pull.rebase", "true"),
            ("remote.upstream.url", "git@github.com:x/y.git"),
            ("remote.my.remote.url", "/Users/x/repo"),
            ("submodule.libs/a.url", "https://x/a"),
            ("lfs.url", "https://x/lfs"),
            ("safe.directory", "*"),
            ("merge.ff", "only"),
            ("diff.renames", "true"),
        ];
        for (k, v) in clean {
            assert!(is_inert(k, v), "{k} 不该可疑");
        }
    }

    /// 每一条都是实测会执行命令的（JOURNAL 2026-09-17 的那张表），一条都不许放行
    #[test]
    fn 会执行命令的键全部可疑() {
        let bad = [
            ("core.fsmonitor", "touch /tmp/pwn; false"),
            ("core.hookspath", ".husky"),
            ("core.sshcommand", "evil"),
            ("core.gitproxy", "evil"),
            ("core.pager", "evil"),
            ("core.editor", "evil"),
            ("diff.external", "evil"),
            ("diff.x.textconv", "evil"),
            ("diff.x.command", "evil"),
            ("merge.x.driver", "evil"),
            ("filter.lfs.clean", "git-lfs clean -- %f"),
            ("filter.x.smudge", "evil"),
            ("filter.x.process", "evil"),
            ("gpg.program", "/tmp/fakegpg"),
            ("gpg.x509.program", "evil"),
            ("credential.helper", "!evil"),
            ("alias.st", "!touch /tmp/pwn"),
            ("alias.co", "checkout"),
            ("include.path", "/tmp/inc"),
            ("includeif.gitdir:/x.path", "/tmp/inc"),
            ("protocol.ext.allow", "always"),
            ("remote.origin.url", "ext::sh -c touch% /tmp/pwn"),
            ("remote.origin.url", "fd::17"),
            ("remote.origin.url", "evil::x"),
            ("uploadpack.packobjectshook", "evil"),
            ("receive.fsck.x", "x"),
            ("sequence.editor", "evil"),
            ("submodule.x.update", "!evil"),
            ("mergetool.x.cmd", "evil"),
            ("difftool.x.cmd", "evil"),
            ("browser.x.cmd", "evil"),
            ("url.evil::.insteadof", "https://"),
            ("core.somethingnew", "x"),
            ("interactive.difffilter", "evil"),
            ("sendemail.smtpserver", "/tmp/evil"),
        ];
        for (k, v) in bad {
            assert!(!is_inert(k, v), "{k} 必须可疑");
        }
    }

    #[test]
    fn 解析_z_输出() {
        let raw = b"file:.git/config\0core.bare\nfalse\0file:/tmp/inc\0core.fsmonitor\ntouch /tmp/pwn\0file:.git/config\0core.flag\0";
        let es = parse_list(raw);
        assert_eq!(es.len(), 3);
        assert_eq!(es[0], e("core.bare", "false"));
        assert_eq!(es[1].origin, "file:/tmp/inc");
        assert_eq!(es[1].value, "touch /tmp/pwn");
        assert_eq!(es[2].value, "", "没有值的布尔键");
        let s = suspects(&es);
        assert_eq!(s.len(), 2, "fsmonitor 和不认识的 core.flag");
        assert_eq!(s[0].origin, "file:/tmp/inc", "被 include 进来的要指向原文件");
    }

    #[test]
    fn 指纹不看来源看内容() {
        let a = [e("core.bare", "false"), e("user.name", "x")];
        let mut b = vec![e("user.name", "x"), e("core.bare", "false")];
        b[0].origin = "file:/elsewhere".into();
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert_ne!(fingerprint(&a), fingerprint(&[e("core.bare", "true"), e("user.name", "x")]));
    }

    #[test]
    fn 真仓库_干净的不可疑_塞一条就可疑() {
        let dir = std::env::temp_dir().join(format!("gitsvc-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        crate::run(&dir, &["config", "user.email", "t@t.t"]).unwrap();
        let s = scan(&dir).unwrap();
        assert!(s.suspects.is_empty(), "{:?}", s.suspects);
        assert!(s.hooks.is_empty());
        let fp = s.fingerprint.clone();
        crate::run(&dir, &["config", "filter.evil.clean", "touch /tmp/pwn; cat"]).unwrap();
        let s2 = scan(&dir).unwrap();
        assert_eq!(s2.suspects.len(), 1);
        assert_eq!(s2.suspects[0].key, "filter.evil.clean");
        assert_ne!(s2.fingerprint, fp, "config 一变指纹要变");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
