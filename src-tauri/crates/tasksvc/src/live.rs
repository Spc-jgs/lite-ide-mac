//! 「哪些任务还在跑」落盘一份，给应用崩了之后收尸用（TASKS.md 第 5 节）。
//!
//! Rust 侧是 `panic = abort`：应用一崩进程当场死，`Drop` 一个都不跑，而任务在**自己的进程组**里（为了停得干净），
//! 不会跟着死 —— 这正是 IDEA 论坛里「IDE 崩了，JVM 还占着 8080」。所以起任务时记下来，停掉时划掉，下次启动时还在的就是没收尸的。
//!
//! **进程号会被复用**：崩了之后过几个小时，同一个组号可能已经是别人的程序。所以不光记组号，还记组长的**启动时间**
//! （内核给的，精确到微秒），收尸前两样都对上才算 —— 只看组号就是「拿着旧门牌号去砸新住户的门」。

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    /// 进程组号 = 组长的 pid（`process_group(0)` 起的）
    pub pgid: i32,
    /// 组长的启动时间（微秒，内核记的）。和组号一起才认得出「还是那一个」
    pub started_us: u64,
    /// 任务名、整条命令：卡片上给人看的
    pub name: String,
    pub command: String,
    /// 哪个项目的（卡片上说「哪个项目的任务」）
    pub root: String,
}

/// 进程的启动时间（微秒）。进程不在了 / 不让看 → None
#[cfg(target_os = "macos")]
pub fn start_time_us(pid: i32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: 缓冲区是一个完整的 proc_bsdinfo，大小如实告诉内核
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    (n == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

#[cfg(not(target_os = "macos"))]
pub fn start_time_us(_pid: i32) -> Option<u64> {
    None
}

/// 这一条还是不是当初那个、而且还活着：组长的启动时间对得上（不是复用了号的别人），组里还有进程
pub fn still_running(l: &Live) -> bool {
    start_time_us(l.pgid) == Some(l.started_us) && crate::group_alive(l.pgid)
}

/// 读。文件不在、坏了 → 空：这是收尸的线索，不是启动的前提，读不出来不能挡住启动
pub fn load(path: &Path) -> Vec<Live> {
    fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// 整份写。先写临时文件再改名：写到一半崩了，留下的也是上一份完整的
pub fn save(path: &Path, all: &[Live]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(all).map_err(io::Error::other)?)?;
    fs::rename(&tmp, path)
}
