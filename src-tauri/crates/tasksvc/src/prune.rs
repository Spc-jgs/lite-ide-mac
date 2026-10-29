//! 任务输出的清理（2026-10-10 梳理 #48 时补的）。
//!
//! 轮转管住了「一个文件无限大」（[`crate::LOG_CAP`]，每个任务最多当前 + `.1` 两份），没管「文件无限多」：一个项目一个目录、
//! 一个任务两份，开过十几个项目、跑过几个开好几天的 dev server，就是几十 GB，而且藏在应用数据目录里没人看得见。
//!
//! 两条：**整个项目最近一份输出都超过 `keep` 没动过 → 这个项目的输出全删**；剩下的**总量超过 `cap` → 从最旧的文件删起**。
//!
//! 直接删，不进废纸篓：rust.md「删除只走废纸篓」管的是**用户的文件**；这是应用自己生成的输出（同 `replacesvc::journal::remove`），
//! 进了废纸篓照样占着那几个 GB。所以只认自己生成的东西：`<16 位十六进制>/` 目录底下的 `*.log` / `*.log.1`，别的文件、`live.json` 一概不碰。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// 一个项目多久没跑过任务就清掉它的输出
pub const KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
/// 所有项目输出的清理目标；活跃写入者优先，不为达标删它们的文件。
pub const TOTAL_CAP: u64 = 5 << 30;

// 登记与清理共用锁：启动时的后台清理也不能删掉刚起的写入者。
static WRITERS: Mutex<BTreeMap<PathBuf, usize>> = Mutex::new(BTreeMap::new());

pub(crate) struct Writer(PathBuf);

fn managed_base(log: &Path) -> Option<&Path> {
    let dir = log.parent()?;
    let name = dir.file_name()?.to_str()?;
    if !is_project(name) || !log.file_name()?.to_str()?.ends_with(".log") { return None; }
    dir.parent()
}

fn is_project(name: &str) -> bool {
    name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn protect(log: &Path) -> Option<Writer> {
    let base = managed_base(log)?;
    let mut writers = WRITERS.lock().unwrap_or_else(|e| e.into_inner());
    *writers.entry(log.to_path_buf()).or_default() += 1;
    prune_locked(base, KEEP, TOTAL_CAP, SystemTime::now(), &writers);
    Some(Writer(log.to_path_buf()))
}

pub(crate) fn rotated(log: &Path) {
    if let Some(base) = managed_base(log) {
        prune(base, KEEP, TOTAL_CAP, SystemTime::now());
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let mut writers = WRITERS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = writers.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 { writers.remove(&self.0); }
        }
        // 跟着输出管道 EOF 走，不跟着 UI 卡片的寿命；已结束卡片可能留好几天。
        if let Some(base) = managed_base(&self.0) {
            prune_locked(base, KEEP, TOTAL_CAP, SystemTime::now(), &writers);
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Pruned {
    pub files: usize,
    pub bytes: u64,
    /// 活跃文件不能删；只剩它们时可能仍超过预算，不能谎报已压到 cap。
    pub remaining_bytes: u64,
}

struct Log {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

/// `base` = `<应用数据>/runs`。启动、任务起止和轮转时调用，跳过登记的写入者及其 .1。
/// 活跃文件优先于总量目标；`remaining_bytes` 如实返回。`now` 可注入。
pub fn prune(base: &Path, keep: Duration, cap: u64, now: SystemTime) -> Pruned {
    let writers = WRITERS.lock().unwrap_or_else(|e| e.into_inner());
    prune_locked(base, keep, cap, now, &writers)
}

fn prune_locked(base: &Path, keep: Duration, cap: u64, now: SystemTime, writers: &BTreeMap<PathBuf, usize>) -> Pruned {
    let mut out = Pruned::default();
    let mut kept: Vec<Log> = Vec::new();
    let Ok(rd) = fs::read_dir(base) else { return out };
    for e in rd.flatten() {
        let dir = e.path();
        let is_ours = e.file_name().to_str().is_some_and(is_project);
        if !is_ours || !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let logs = logs_in(&dir);
        let newest = logs.iter().map(|l| l.modified).max();
        if newest.is_some_and(|m| now.duration_since(m).unwrap_or_default() > keep) {
            for l in &logs {
                if protected(&l.path, writers) || !remove(l, &mut out) {
                    kept.push(Log { path: l.path.clone(), len: l.len, modified: l.modified });
                }
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
            if !protected(&l.path, writers) && remove(l, &mut out) { total -= l.len; }
        }
    }
    out.remaining_bytes = total;
    out
}

fn protected(path: &Path, writers: &BTreeMap<PathBuf, usize>) -> bool {
    writers.contains_key(path) || (path.extension().is_some_and(|e| e == "1") && writers.contains_key(&path.with_extension("")))
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

#[allow(clippy::disallowed_methods)] // 只删自己生成的任务输出（<16 位十六进制>/*.log、*.log.1），范围在 prune 里卡死
fn remove(l: &Log, out: &mut Pruned) -> bool {
    if fs::remove_file(&l.path).is_ok() {
        out.files += 1;
        out.bytes += l.len;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 3600);

    #[test]
    fn 活跃日志和上一次的都不删_最后一个写入者释放后清理() {
        let b = base("active");
        let now = SystemTime::now();
        let p = b.join("00000000000000ee/t.log");
        let first = protect(&p).unwrap();
        file(&p, 100, now, 30 * DAY);
        file(&crate::rotated(&p), 100, now, 31 * DAY);
        let second = protect(&p).unwrap();
        let r = prune(&b, 14 * DAY, 1, now);
        assert_eq!(r, Pruned { files: 0, bytes: 0, remaining_bytes: 200 });
        drop(first);
        assert!(p.exists() && crate::rotated(&p).exists());
        drop(second);
        assert!(!p.exists() && !crate::rotated(&p).exists());
        let _ = fs::remove_dir_all(b);
    }

    #[test]
    fn 总量清理跳过活跃输出_仍回收其他项目的旧输出() {
        let b = base("active-cap");
        let now = SystemTime::now();
        let active = b.join("00000000000000ef/t.log");
        let guard = protect(&active).unwrap();
        file(&active, 100, now, DAY);
        let old = b.join("00000000000000ff/done.log");
        file(&old, 100, now, 2 * DAY);
        let r = prune(&b, 14 * DAY, 150, now);
        assert_eq!(r, Pruned { files: 1, bytes: 100, remaining_bytes: 100 });
        assert!(active.exists() && !old.exists());
        drop(guard);
        let _ = fs::remove_dir_all(b);
    }

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
        assert_eq!(r, Pruned { files: 2, bytes: 20, remaining_bytes: 20 });
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
        assert_eq!(r, Pruned { files: 2, bytes: 200, remaining_bytes: 100 }, "300 → 150：删最旧的两份");
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
