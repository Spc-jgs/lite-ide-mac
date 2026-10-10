//! 任务输出的清理（2026-10-10 梳理 #48 时补的）。
//!
//! 轮转管住了「一个文件无限大」（[`crate::LOG_CAP`]，每个任务最多当前 + `.1` 两份），没管「文件无限多」：一个项目一个目录、
//! 一个任务两份，开过十几个项目、跑过几个开好几天的 dev server，就是几十 GB，而且藏在应用数据目录里没人看得见。
//!
//! 两条：**整个项目最近一份输出都超过 `keep` 没动过 → 这个项目的输出全删**；剩下的**总量超过 `cap` → 从最旧的文件删起**。
//!
//! 直接删，不进废纸篓：rust.md「删除只走废纸篓」管的是**用户的文件**；这是应用自己生成的输出（同 `replacesvc::journal::remove`），
//! 进了废纸篓照样占着那几个 GB。所以只认自己生成的东西：`<16 位十六进制>/` 目录底下的 `*.log` / `*.log.1`，别的文件、`live.json` 一概不碰。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 一个项目多久没跑过任务就清掉它的输出
pub const KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
/// 所有项目的输出加起来最多这么大
pub const TOTAL_CAP: u64 = 5 << 30;

#[derive(Debug, Default, PartialEq)]
pub struct Pruned {
    pub files: usize,
    pub bytes: u64,
}

struct Log {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

/// `base` = `<应用数据>/runs`。**只在没有任务在写的时候调**（启动时）：正在写的文件被删了，任务照样写进一个看不见的 inode，
/// 日志视图跟着名字找不到它。`now` 由调用方给（测试里拨时间）
pub fn prune(base: &Path, keep: Duration, cap: u64, now: SystemTime) -> Pruned {
    let mut out = Pruned::default();
    let mut kept: Vec<Log> = Vec::new();
    let Ok(rd) = fs::read_dir(base) else { return out };
    for e in rd.flatten() {
        let dir = e.path();
        let is_ours = e.file_name().to_str().is_some_and(|n| n.len() == 16 && n.bytes().all(|b| b.is_ascii_hexdigit()));
        if !is_ours || !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let logs = logs_in(&dir);
        let newest = logs.iter().map(|l| l.modified).max();
        if newest.is_some_and(|m| now.duration_since(m).unwrap_or_default() > keep) {
            for l in &logs {
                remove(l, &mut out);
            }
            // 空了才删目录（里面要是有我们不认识的东西，留着）
            let _ = fs::remove_dir(&dir);
        } else {
            kept.extend(logs);
        }
    }
    let mut total: u64 = kept.iter().map(|l| l.len).sum();
    if total > cap {
        kept.sort_by_key(|l| l.modified);
        for l in &kept {
            if total <= cap {
                break;
            }
            remove(l, &mut out);
            total -= l.len;
        }
    }
    out
}

/// 目录里我们生成的输出：普通文件（不跟软链）、名字是 `*.log` 或 `*.log.1`
fn logs_in(dir: &Path) -> Vec<Log> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.ends_with(".log") || n.ends_with(".log.1")))
        .filter_map(|e| {
            let m = fs::symlink_metadata(e.path()).ok()?;
            m.is_file().then(|| Log { path: e.path(), len: m.len(), modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH) })
        })
        .collect()
}

fn remove(l: &Log, out: &mut Pruned) {
    if fs::remove_file(&l.path).is_ok() {
        out.files += 1;
        out.bytes += l.len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 3600);

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tasksvc-prune-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// 写一个文件，修改时间拨到 `age` 之前
    fn file(p: &Path, len: usize, now: SystemTime, age: Duration) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![b'x'; len]).unwrap();
        fs::File::options().write(true).open(p).unwrap().set_modified(now - age).unwrap();
    }

    #[test]
    fn 很久没跑过的项目整个清掉_最近跑过的留着_别的东西不碰() {
        let b = base("age");
        let now = SystemTime::now();
        let (old, fresh) = (b.join("00000000000000aa"), b.join("00000000000000bb"));
        file(&old.join("后端.log"), 10, now, 30 * DAY);
        file(&old.join("后端.log.1"), 10, now, 31 * DAY);
        file(&fresh.join("dev.log"), 10, now, DAY);
        file(&fresh.join("dev.log.1"), 10, now, 40 * DAY);
        file(&b.join("live.json"), 10, now, 99 * DAY);
        file(&b.join("not-a-hash").join("x.log"), 10, now, 99 * DAY);

        let r = prune(&b, 14 * DAY, u64::MAX, now);
        assert_eq!(r, Pruned { files: 2, bytes: 20 });
        assert!(!old.exists(), "30 天没跑过的项目目录整个没了");
        assert!(fresh.join("dev.log.1").exists(), "最近跑过的项目一份都不删（它的 .1 再旧也是「上一次」，留着回头看）");
        assert!(b.join("live.json").exists() && b.join("not-a-hash/x.log").exists(), "不是我们生成的不碰");
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn 总量超了从最旧的删起_删到不超为止() {
        let b = base("cap");
        let now = SystemTime::now();
        let p = b.join("00000000000000cc");
        file(&p.join("a.log.1"), 100, now, 3 * DAY);
        file(&p.join("b.log.1"), 100, now, 2 * DAY);
        file(&p.join("a.log"), 100, now, DAY);
        file(&p.join("note.txt"), 100, now, 9 * DAY);

        let r = prune(&b, 14 * DAY, 150, now);
        assert_eq!(r, Pruned { files: 2, bytes: 200 }, "300 → 150：删最旧的两份");
        assert!(!p.join("a.log.1").exists() && !p.join("b.log.1").exists() && p.join("a.log").exists());
        assert!(p.join("note.txt").exists(), "不认识的文件不删、也不算进总量");
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn 目录里有不认识的东西_清完输出目录留着() {
        let b = base("keepdir");
        let now = SystemTime::now();
        let p = b.join("00000000000000dd");
        file(&p.join("t.log"), 10, now, 30 * DAY);
        file(&p.join("别人的.txt"), 10, now, 30 * DAY);
        prune(&b, 14 * DAY, u64::MAX, now);
        assert!(!p.join("t.log").exists() && p.join("别人的.txt").exists());
        let _ = fs::remove_dir_all(&b);
    }
}
