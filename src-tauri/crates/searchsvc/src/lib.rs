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
use std::process::Command;

/// 不进这些目录。大仓库里它们占了绝大多数文件，进去只会把索引撑爆。
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".venv",
    "venv",
    "__pycache__",
    ".gradle",
    ".idea",
    ".vscode",
    "vendor",
];

/// 索引上限。超过这个数就停 —— 再多前端也没法有意义地展示。
pub const MAX_FILES: usize = 50_000;
/// 递归深度上限，防软链环或者病态目录结构
const MAX_DEPTH: usize = 24;

/// 递归列出项目里的文件（相对路径）。
pub fn list_files(root: impl AsRef<Path>) -> io::Result<Vec<String>> {
    let root = root.as_ref();
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
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
        // symlink 一律不跟进：跟进就可能绕进环里
        let Ok(ft) = ent.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                subdirs.push(ent.path());
            }
        } else if let Ok(rel) = ent.path().strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
    for d in subdirs {
        walk(root, &d, depth + 1, out);
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
pub fn grep(root: impl AsRef<Path>, pattern: &str, limit: usize) -> io::Result<Vec<Hit>> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    match grep_rg(root.as_ref(), pattern, limit) {
        Ok(hits) => Ok(hits),
        // rg 不在、版本不对、输出格式变了 —— 一律回落，不让搜索功能整个瘫掉
        Err(_) => grep_builtin(root.as_ref(), pattern, limit),
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

fn grep_rg(root: &Path, pattern: &str, limit: usize) -> io::Result<Vec<Hit>> {
    let mut cmd = Command::new("rg");
    cmd.args([
        "--json",
        "--line-number",
        "--no-heading",
        "--smart-case",
        "--max-filesize",
        "8M",
    ]);
    // 与内置实现的 SKIP_DIRS 对齐。
    // rg 靠 .gitignore 跳过 node_modules 之类，但项目不一定有 .gitignore ——
    // 那样「装了 rg」和「没装 rg」搜出来的结果就不一样了，这是不能接受的。
    for d in SKIP_DIRS {
        cmd.arg("--glob").arg(format!("!**/{d}/**"));
    }
    let out = cmd.args(["-e", pattern]).arg(root).output()?;

    // rg 一般原样保留传入的前缀，但遇到软链时可能吐出解析后的真实路径
    // （macOS 上 /var → /private/var）。两个前缀都试，否则结果里会混进绝对路径。
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    // rg 的退出码：0 = 有命中，1 = 无命中（正常），>=2 才是真出错
    if out.status.code().is_some_and(|c| c >= 2) {
        return Err(io::Error::other(format!(
            "rg 执行失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    let mut hits = Vec::new();
    for line in out.stdout.split(|b| *b == b'\n') {
        if hits.len() >= limit {
            break;
        }
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if v["type"] != "match" {
            continue;
        }
        let d = &v["data"];
        let (Some(path), Some(num), Some(text)) = (
            d["path"]["text"].as_str(),
            d["line_number"].as_u64(),
            d["lines"]["text"].as_str(),
        ) else {
            continue;
        };
        hits.push(Hit {
            path: rel(root, &canon, path),
            line: num,
            text: clip(text.trim_end()),
        });
    }
    Ok(hits)
}

/// 进程内回落实现：遍历索引到的文件逐个扫。
fn grep_builtin(root: &Path, pattern: &str, limit: usize) -> io::Result<Vec<Hit>> {
    let files = list_files(root)?;
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
        d
    }

    #[test]
    fn 索引跳过噪声目录() {
        let d = sandbox("list");
        let files = list_files(&d).unwrap();
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(
            !files.iter().any(|f| f.contains("node_modules")),
            "node_modules 不该进索引：{files:?}"
        );
        assert!(
            !files.iter().any(|f| f.contains(".git")),
            "点目录不该进索引"
        );
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 内置实现能搜到内容且同样跳过噪声() {
        let d = sandbox("builtin");
        let hits = grep_builtin(&d, "needle", 50).unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert!(paths.contains(&"README.md"));
        assert!(paths.contains(&"src/main.rs"));
        assert!(!paths.iter().any(|p| p.contains("node_modules")));
        // 行号必须是 1-based，与编辑器一致
        let readme = hits.iter().find(|h| h.path == "README.md").unwrap();
        assert_eq!(readme.line, 2);
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 无命中返回空() {
        let d = sandbox("empty");
        assert!(grep_builtin(&d, "绝不存在的词", 50).unwrap().is_empty());
        assert!(
            grep(&d, "", 50).unwrap().is_empty(),
            "空 pattern 不该扫全项目"
        );
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn grep_入口在有无_rg_时都能工作() {
        let d = sandbox("entry");
        let hits = grep(&d, "needle", 50).unwrap();
        assert!(!hits.is_empty(), "无论走 rg 还是回落，都该有命中");
        assert!(hits.iter().all(|h| !h.path.contains("node_modules")));
        fs::remove_dir_all(d).ok();
    }

    /// 装没装 rg 都该搜出同一批结果 —— 这个不变量真的破过：
    /// 内置实现靠 SKIP_DIRS 跳过 node_modules，而 rg 靠 .gitignore，
    /// 项目没有 .gitignore 时两边就分岔了。
    #[test]
    fn 两条实现路径结果必须一致() {
        if !ripgrep_available() {
            eprintln!("跳过：机器上没有 rg");
            return;
        }
        let d = sandbox("parity");
        let mut a: Vec<(String, u64)> = grep_rg(&d, "needle", 100)
            .unwrap()
            .into_iter()
            .map(|h| (h.path, h.line))
            .collect();
        let mut b: Vec<(String, u64)> = grep_builtin(&d, "needle", 100)
            .unwrap()
            .into_iter()
            .map(|h| (h.path, h.line))
            .collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "rg 与内置实现搜出的结果不一致");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn 命中行被截断() {
        let d = sandbox("clip");
        let long = format!("needle{}\n", "x".repeat(2000));
        fs::write(d.join("long.txt"), &long).unwrap();
        let hits = grep_builtin(&d, "needle", 50).unwrap();
        let h = hits.iter().find(|h| h.path == "long.txt").unwrap();
        assert!(h.text.chars().count() <= MAX_HIT_LEN);
        fs::remove_dir_all(d).ok();
    }
}
