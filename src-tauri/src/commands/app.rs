//! 应用自身：初始路径、应用日志与调试、预算上报、菜单栏（选文件夹 / 保存面板 / 最近 / 灰态）、外部链接、安装命令行工具。

use super::*;

/// 装 `lite` 命令：脚本写到应用数据目录，软链到 `/usr/local/bin/lite`。
/// 业务在 fsservice；这里只算两个路径、转错误。不提权 —— 软链装不上就把
/// 那句 `sudo ln -sf` 交给用户。
#[tauri::command]
pub fn install_cli(app: tauri::AppHandle) -> Result<CliInstallDto, String> {
    use tauri::Manager;
    let exe = std::env::current_exe().map_err(|e| format!("取不到可执行文件路径：{e}"))?;
    let bundle = fsservice::bundle_from_exe(&exe).ok_or_else(|| {
        "只有打包后的 .app 才能安装命令行工具（现在跑的是开发构建）".to_string()
    })?;
    let script = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("取不到应用数据目录：{e}"))?
        .join("bin")
        .join("lite");
    let link = Path::new("/usr/local/bin/lite");
    let r = fsservice::install_cli(&script, &fsservice::cli_script(&bundle), link)
        .map_err(|e| format!("写不了 {}：{e}", script.display()))?;
    applog::write(
        applog::Level::Info,
        "cli",
        &format!("install_cli script={} linked={} replaced={}", r.script.display(), r.linked, r.replaced),
    );
    Ok(CliInstallDto {
        link_cmd: format!("sudo ln -sf \"{}\" {}", r.script.display(), link.display()),
        script: r.script.to_string_lossy().into_owned(),
        linked: r.linked,
        replaced: r.replaced,
    })
}

/// 启动时该打开的路径：`argv` 里的（直接 exec 二进制：`lite-ide foo.log`）
/// **加上**系统在前端就绪之前送来的（Finder 双击 / 拖 Dock / `open -a`，
/// 走 `RunEvent::Opened`，见 `open.rs`）。两条路在这里汇成一个口，
/// 前端不用知道文件是从哪条路来的。
///
/// 文件和目录都接受：目录会成为项目根，文件则打开并把父目录当根。
/// 早先只认 `is_file()`，`lite-ide <目录>` 静默什么都不做。
///
/// **调用这一次就把这个窗口的收件箱标成「前端就绪」** —— 之后再来的路径直接发事件。
/// 所以前端必须**先挂好 `open-paths` 的监听再调它**，反过来中间那一拍到的就丢了。
///
/// 多窗口第 2 步起，收件箱是**每个窗口一个**（`windows.rs`），取的是调用它的那个窗口的；
/// `argv` 在 `setup` 时就送进了第一个窗口的收件箱，不再每次现读 —— 否则每个新窗口
/// 都会把启动参数再开一遍。
#[tauri::command]
pub fn initial_paths(window: tauri::Window, state: State<'_, AppState>) -> Vec<String> {
    let found = state.windows.take_inbox(window.label());
    crate::diag!("initial_paths {} -> {found:?}", window.label());
    found
}

/// 前端告诉 Rust「我这个窗口现在开着哪个项目」（空串 = 没有项目）。
///
/// 路由要用：Finder 双击一个文件，该落到项目包含它的那个窗口；打开一个目录，
/// 已经有窗口开着它就该去那个窗口（`windows::route`）。项目根的真相在前端
/// （会话恢复、⌘O、关闭项目都在那边改它），所以由前端在它变的时候报过来。
#[tauri::command]
pub fn set_window_root(app: tauri::AppHandle, window: tauri::Window, root: String, state: State<'_, AppState>) {
    crate::winctl::set_title(&window, Some(root.as_str()).filter(|r| !r.is_empty()));
    // 开了一个新项目就记进「最近打开」（同一个根重复报不算，见 Windows::set_root）
    if state.windows.set_root(window.label(), Some(root)) {
        crate::winctl::recent_changed(&app);
    }
    // 下次启动按这个开回来（windows.json）
    crate::winctl::save_soon();
}

/// 前端说「我存好了」—— 回的是退出前发给它的 `flush`（`winctl::quit`）
#[tauri::command]
pub fn quit_ready(window: tauri::Window, state: State<'_, AppState>) {
    state.windows.quit_ack(window.label());
}

/// 从随处搜索里点「退出」：和菜单栏的 ⌘Q 走同一条路
#[tauri::command]
pub fn request_quit(app: tauri::AppHandle) {
    crate::winctl::quit(&app);
}

/// 前端把执行轨迹与 JS 错误报回来。release 没有 devtools，
/// 这是 WebView 里唯一的可观测通道。默认静默，`LITE_IDE_DEBUG=1` 打开。
#[tauri::command]
pub fn diag(msg: String) {
    if crate::diag::enabled() {
        eprintln!("[diag/web] {msg}");
    }
}

/// 前端把一条日志写进 `~/Library/Logs/com.liteide.app/app.log`。
///
/// **和 `diag` 是两件事，别合并：**
///
/// | | `diag` | `app_log` |
/// |---|---|---|
/// | 去哪 | stderr | 盘上的文件 |
/// | 默认 | 关（`LITE_IDE_DEBUG=1` 才开） | **开** |
/// | 给谁看 | 开发时盯着终端的我 | 出事之后回头查的人 |
///
/// `diag` 是「我现在在看」，`app_log` 是「以后有人会看」。
/// 把执行轨迹全塞进文件会把真正的错误埋掉，所以走这条的**只有异常**。
#[tauri::command]
pub fn app_log(level: String, source: String, msg: String) {
    applog::write(applog::Level::parse(&level), &source, &msg);
}

/// 清空应用日志。保留策略与「为什么只碰两个写死的名字」见 `applog::clear`。
#[tauri::command]
pub fn clear_app_log() {
    applog::clear();
}

/// 日志文件在哪。前端拿它开一个标签 —— 这个应用自己就是日志查看器。
#[tauri::command]
pub fn app_log_path(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    Ok(applog::log_path(dir).to_string_lossy().into_owned())
}

/// 启动完成时把预算量一遍写进 `app.log`（issue #28）。
///
/// **前端来叫，不是 Rust 自己定时写。** 「启动完成」这件事只有前端知道 ——
/// Rust 这边 `setup()` 返回时窗口还是白的，会话恢复、首屏渲染都在后头。
/// 而且 `tabs` / `editors` / `nodes` 这几个数本来就长在前端。
///
/// 判据、量法和「哪个数有多可信」全在 `crate::budget` 的文件头，这里只是接线。
#[tauri::command]
pub fn report_budget(app: tauri::AppHandle, tabs: u32, terms: u32, editors: u32, nodes: u32) {
    let c = crate::budget::Counts { tabs, terms, editors, nodes };
    let msg = crate::budget::line(
        crate::budget::uptime_ms(),
        crate::budget::phys_footprint_mb(),
        c,
        &app.package_info().version.to_string(),
        devtools_build(),
    );
    crate::diag!("budget {msg}");
    applog::write(applog::Level::Info, "budget", &msg);
}

/// 启动分段的打点（`budget::mark`）：前端在 `main.ts` 开始执行、App 挂上两处各叫一次。
/// 名字由前端给，但只认这两个 —— 别的名字丢掉，这条命令不该成为往日志里写任意字符串的口子。
#[tauri::command]
pub fn boot_mark(name: String) {
    if name == "js" || name == "mount" {
        crate::budget::mark(&name);
    }
}

/// 这份构建带不带 Web Inspector（issue #20）。
///
/// `pnpm app:bundle:devtools` 和 `pnpm app:bundle` **装在同一个路径上**，
/// 而「盘上只留一份 .app」是这个仓库的硬纪律 —— 所以分辨它们的办法
/// 不能是「看是哪个文件」，只能是**让它自报家门**。
///
/// 悬停标题栏的项目挂件就能看到，和已有的「构建时间」并排：
/// 排查时本来就要看那一眼，不多一个新习惯。
///
/// 这不是「INFO 级别的小事」：带着 inspector 的那份，任何本机进程都能
/// 附加到这个 webview 上读写页面。忘了打回去的话，界面上原本**没有
/// 任何迹象**说明这件事。
#[tauri::command]
pub fn devtools_build() -> bool {
    cfg!(feature = "devtools")
}

/// 诊断开着没有。前端拿它决定**要不要建那条统计定时器** ——
/// 关着的时候一次都不算，不能让调试设施在所有人机器上白跑。
///
/// 不用 `import.meta.env.DEV` 判是因为要量的正是 **release 包**：
/// `pnpm app:bundle` 出来的那份才是人真正在跑的东西，dev 模式下
/// 加载的是 localhost 的前端，量它没有意义。
#[tauri::command]
pub fn diag_enabled() -> bool {
    crate::diag::enabled()
}


/// 开原生的「选择文件夹」面板，返回选中的路径；取消返回 `None`。
///
/// **必须在 Rust 侧开。** `NSOpenPanel` 是主线程亲和的原生控件，
/// webview 里没有等价物（HTML 的 `<input webkitdirectory>` 给的是
/// 一堆文件条目，不是目录路径，而且拿不到绝对路径）。
///
/// 用 `blocking_pick_folder` 而不是回调版：这个命令跑在 Tauri 的命令线程上，
/// 不是主线程 —— 插件内部会把面板调度到主线程，这里阻塞等它就行。
/// 反过来（在主线程上 blocking）才会死锁。
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    crate::diag!("pick_folder 打开面板");
    let picked = app
        .dialog()
        .file()
        .set_title("打开文件夹")
        .blocking_pick_folder();
    picked.map(|p| p.to_string())
}

/// 「另存为…」的原生保存面板。返回选中的完整路径；取消是 None。
///
/// 覆盖确认由面板自己做（macOS 会问「x 已存在，要替换吗」），这里不再问第二遍。
/// `dir` 是默认目录：有项目就是项目根 —— 草稿「毕业」多半是进当前项目。
#[tauri::command]
pub async fn pick_save_path(app: tauri::AppHandle, dir: Option<String>, name: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let mut d = app.dialog().file().set_title("另存为").set_file_name(&name);
    if let Some(dir) = dir {
        d = d.set_directory(dir);
    }
    d.blocking_save_file().map(|p| p.to_string())
}

/// 「最近打开」的项目。名单在 Rust（多窗口第 4 步）：原生菜单整个应用只有一份，
/// 名单也只能有一个主人 —— 原来每个窗口各存一份、互相覆盖。变了的时候 Rust 会广播
/// `recent-changed`，前端只在启动时主动取这一次。
#[tauri::command]
pub fn recent_projects(app: tauri::AppHandle) -> RecentDto {
    crate::winctl::recent_dto(&app)
}

/// 点了一个最近项目，发现目录没了：从名单里拿掉
#[tauri::command]
pub fn forget_recent(app: tauri::AppHandle, dir: String, state: State<'_, AppState>) -> RecentDto {
    if state.windows.forget_recent(&dir) {
        crate::winctl::recent_changed(&app);
    }
    crate::winctl::recent_dto(&app)
}

#[tauri::command]
pub fn clear_recent(app: tauri::AppHandle, state: State<'_, AppState>) -> RecentDto {
    if state.windows.clear_recent() {
        crate::winctl::recent_changed(&app);
    }
    crate::winctl::recent_dto(&app)
}

/// 升级后第一次启动：前端读到旧版的全局快照，把里面的「最近打开」交过来。
/// 这边已经有名单了就不收（`Windows::adopt_recent`）
#[tauri::command]
pub fn adopt_recent(app: tauri::AppHandle, projects: Vec<String>, state: State<'_, AppState>) -> RecentDto {
    if state.windows.adopt_recent(projects) {
        crate::winctl::recent_changed(&app);
    }
    crate::winctl::recent_dto(&app)
}

/// 没有项目的窗口要恢复 / 写「没有项目的那份会话快照」之前先问一句：给不给它。
/// 同一时刻只归一个窗口（`Windows::claim_empty`），不然两个空窗口会开出同一批标签
#[tauri::command]
pub fn claim_empty_session(window: tauri::Window, state: State<'_, AppState>) -> bool {
    state.windows.claim_empty(window.label())
}

/// 在合适的窗口里打开一个目录（⌘O、最近打开、打开工作树、拖进来一个文件夹）。
///
/// 走和 Finder 送来的路径同一个路由：已经有窗口开着它就去那个窗口，否则开新窗口
/// （调用它的窗口已经有项目了 —— 没有项目的窗口前端自己就地打开，不来这儿）。
/// 路由时把**调用它的窗口**当前台，不看焦点（`Windows::deliver` 的 `from`）。
/// 返回 true = 交给 Rust 了；浏览器桩返回 false，前端退回「就地换项目」。
#[tauri::command]
pub fn open_window(app: tauri::AppHandle, window: tauri::Window, path: String) -> bool {
    crate::open::deliver_from(&app, vec![path], Some(window.label()));
    true
}

/// 这个窗口起来之后要不要新建一份草稿（没有窗口时按了「新建草稿」，`Windows::want_scratch`）。
/// 前端在启动流程里决定「落进哪份草稿」的那一刻问一次
#[tauri::command]
pub fn take_start_scratch(window: tauri::Window, state: State<'_, AppState>) -> bool {
    state.windows.take_start_scratch(window.label())
}

/// 按当下的上下文让菜单项变灰。
///
/// 今天所有键位都是 window 级监听，**不管当下有没有意义都会触发** ——
/// 没有标签时按 ⌘S、不是 Git 仓库时按 ⇧⌘G，都是走一遍然后什么也没发生。
/// 灰掉的菜单项本身就是一句解释：不是坏了，是现在用不上。
///
/// 原生菜单整个应用只有一份，而每个窗口的状态不一样（多窗口第 2 步）：先存进这个窗口
/// 自己那份，**它在前台才真的改菜单**；在后台的等它到前台时由 `lib.rs` 套上。
/// 否则菜单反映的是「最后一个推过来的窗口」，不一定是你正在看的那个。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn sync_menu_state(
    app: tauri::AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    has_tab: bool,
    has_repo: bool,
    has_term: bool,
    has_root: bool,
    can_move: bool,
    split: bool,
) {
    let m = crate::windows::MenuState { has_tab, has_repo, has_term, has_root, can_move, split };
    if state.windows.set_menu(window.label(), m) {
        crate::menu::apply(&app, m);
    }
}

/// 交给系统默认浏览器打开一个网址。目前只服务「帮助 › 项目主页」。
///
/// **只认 `https://`。** 这是个能让应用启动任意外部程序的口子 ——
/// `open` 会按 scheme 派发，`file://` 能拉起任意程序、自定义 scheme
/// 更是。前端目前只用常量调它，但把判据写在 Rust 侧才算数：
/// 命令一旦存在，它的约束就不能靠调用方自觉。
///
/// 绝对路径而不是靠 PATH，理由同 `fsservice::reveal_in_finder`：
/// 从终端启动时 PATH 是用户的，不该让它决定我们调到哪个 `open`。
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    // 只放行 http / https：闸要拦的是别的 scheme（file://、能拉起应用的自定义 scheme），
    // 这两个都只会交给浏览器。http 是 2026-09-23 为终端里的链接放开的 ——
    // 最常点的恰恰是 `http://localhost:8080`（src/lib/terminal/links.ts）
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("只允许 http / https 链接，实得：{url}"));
    }
    crate::diag!("open_external {url}");
    let st = std::process::Command::new("/usr/bin/open")
        .arg("--")
        .arg(&url)
        .status()
        .map_err(|e| format!("起不来 open：{e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("打不开 {url}"))
    }
}
