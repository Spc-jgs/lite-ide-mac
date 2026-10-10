//! 哪些 Tauri 命令跑在主线程上（同步的 `pub fn`），由这张表说了算 —— 同 `dto_sync.rs` / `menu_sync.rs`：只读源码，比集合。
//!
//! 同步命令跑在主线程上，主线程就是 NSApplication 的事件循环，它一堵整个窗口就不响应（rust.md「同步命令跑在主线程上」）。
//! 判据只有一条：碰盘、碰子进程、碰网络的走 `commands::blocking`。**这条判据原来只写在文档里**，2026-10-10 整体审核时还是在主线程上
//! 找到六个碰盘的（`open_log`、`log_refresh` 每 500ms 一次、`detect_encoding`、新建 / 移动 / 改名）。所以挪完之后由这条测试卡住：
//! 新加一个同步命令，不在下面的表里就红 —— 写进表里要写理由，写不出理由就该是 `async` + `blocking`。

use std::collections::BTreeMap;

/// 留在主线程上的命令，和它为什么可以。分三类：
/// - 内存：只读写进程里的状态，常数时间
/// - 例外：碰盘 / 起子进程，但**有界**且只碰本机内置盘上应用自己的东西，或者本来就要在主线程上做（窗口、菜单）
const ALLOW: &[(&str, &str)] = &[
    // ── app.rs ──
    ("adopt_recent", "内存：最近项目名单"),
    ("app_log", "例外：往应用日志追加一行（应用数据目录，本机盘，2MB 轮转）；前端的报错都走它，挪走就多一次调度"),
    ("app_log_path", "内存：拼路径"),
    ("boot_mark", "内存：启动计时"),
    ("claim_empty_session", "内存：窗口表"),
    ("clear_app_log", "例外：截断应用日志（应用数据目录，本机盘）"),
    ("clear_recent", "内存：最近项目名单"),
    ("devtools_build", "内存：编译期常量"),
    ("diag", "内存：调试开关关着时什么都不做"),
    ("diag_enabled", "内存：环境变量"),
    ("forget_recent", "内存：最近项目名单"),
    ("initial_paths", "内存：启动时送来的文件"),
    ("install_cli", "例外：建一个软链（/usr/local/bin/lite），人点一次菜单才跑"),
    ("open_external", "例外：起 /usr/bin/open 交给浏览器，LaunchServices 毫秒级返回"),
    ("open_window", "窗口：建窗口要在主线程上"),
    ("quit_ready", "内存：退出流程的应答"),
    ("recent_projects", "内存：最近项目名单"),
    ("report_budget", "内存：性能预算记录"),
    ("request_quit", "内存：发起退出（等待在别的线程里）"),
    ("set_window_root", "内存：窗口表"),
    ("sync_menu_state", "窗口：改原生菜单要在主线程上"),
    ("take_start_scratch", "内存：取走启动时的草稿"),
    // ── fs.rs ──
    ("list_encodings", "内存：常量表"),
    ("reveal_in_finder", "例外：起 /usr/bin/open -R，毫秒级返回"),
    ("watch_root", "例外：建 FSEvents 监听（系统调用，不遍历目录）"),
    // ── git.rs / remote.rs ──
    ("clear_git_console", "内存：Git 控制台的记录"),
    ("git_console", "内存：Git 控制台的记录"),
    ("git_cancel", "内存：置取消标记（杀进程组在看门线程里）"),
    // ── log.rs：读行、过滤都是 mmap 上的内存操作（首屏 50 行 0.008ms）；open_log / log_refresh 碰盘，已挪走 ──
    ("close_log", "内存：句柄表"),
    ("log_filter", "内存：编译查询（正则）、起一个后台过滤任务，立刻返回；扫文件在那个任务里"),
    ("log_filter_map", "内存：过滤结果"),
    ("log_filter_stat", "内存：过滤进度"),
    ("log_lines", "内存：mmap 上取行"),
    ("log_lines_filtered", "内存：mmap 上取行"),
    ("log_stat", "内存：索引进度"),
    // ── pty.rs ──
    ("pty_ack", "内存：背压计数"),
    ("pty_cwd", "例外：proc_pidinfo 问前台进程的 cwd（系统调用，不碰盘）"),
    ("pty_kill", "例外：发信号，收尸有界（rust.md「ptysvc 的 kill() 里有一条排空线程」）"),
    ("pty_resize", "内存：ioctl"),
    ("pty_spawn", "例外：起登录 shell（posix_spawn 毫秒级），读线程另起"),
    ("pty_write", "内存：写 pty master（rust.md：挪走只会多一次调度延迟）"),
    // ── scratch.rs ──
    ("create_scratch", "例外：在应用数据目录里建一个小文件（本机盘）"),
    ("discard_empty_scratch", "例外：删应用数据目录里的一个空草稿（本机盘）"),
    ("scratch_dir", "内存：拼路径"),
    // ── settings.rs ──
    ("settings", "内存：设置的那份内存拷贝"),
    ("settings_schema", "内存：常量表"),
    // ── tasks.rs ──
    ("task_close", "内存：摘表，收尾在后台线程里"),
    ("task_stale", "例外：proc_pidinfo 核对组长还在不在（系统调用，不碰盘）"),
    ("task_stop", "内存：发信号，等待在后台线程里"),
];

/// 源码里同步的命令：`#[tauri::command]` 后面（跳过属性和文档注释）是 `pub fn`（不是 `pub async fn`）
fn sync_commands() -> BTreeMap<String, String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let mut lines = text.lines();
        while let Some(l) = lines.next() {
            if !l.trim_start().starts_with("#[tauri::command") {
                continue;
            }
            for next in lines.by_ref() {
                let t = next.trim_start();
                if t.starts_with("#[") || t.starts_with("///") || t.starts_with("//") {
                    continue;
                }
                if let Some(rest) = t.strip_prefix("pub fn ") {
                    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    out.insert(name, p.file_name().unwrap().to_string_lossy().into_owned());
                }
                break;
            }
        }
    }
    out
}

#[test]
fn 主线程上的命令都在表里_表里的都还是同步的() {
    let found = sync_commands();
    assert!(found.len() > 20, "一个同步命令都没认出来？解析坏了：{found:?}");
    let allow: BTreeMap<_, _> = ALLOW.iter().cloned().collect();
    let extra: Vec<_> = found.iter().filter(|(n, _)| !allow.contains_key(n.as_str())).map(|(n, f)| format!("{f}: {n}")).collect();
    assert!(
        extra.is_empty(),
        "这些命令是同步的（跑在主线程上），但不在 ALLOW 里：{extra:?}\n碰盘 / 碰子进程 / 碰网络的改成 `async` + `commands::blocking`；真是内存操作就写进 ALLOW、写清理由"
    );
    let stale: Vec<_> = allow.keys().filter(|n| !found.contains_key(**n)).collect();
    assert!(stale.is_empty(), "ALLOW 里这些已经不是同步命令了（挪走了或删了），从表里拿掉：{stale:?}");
}
