//! 配置文件的读盘、监听、存盘、广播（issue #44 第 2 步，docs/SETTINGS.md 第 7 节）。
//!
//! `settings.rs` 做决定（怎么解析、哪种错怎么退、两份怎么合起来），这里动手 —— 和 `windows.rs` / `winctl.rs`
//! 是同一种拆法：决定要能在裸单测里测全，动手的这一层薄到只剩接线。

use crate::settings::{self, UiState};
use crate::state::AppState;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// 设置变了：广播给**所有**窗口（同 `recent-changed`），负载是全量的 `SettingsDto`
pub const EVENT: &str = "settings-changed";

fn dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok()
}

/// `settings.json` 在哪（「设置…」要打开它，状态栏的提示点了也要跳过去）
pub fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    dir(app).map(|d| d.join("settings.json"))
}

fn ui_path(app: &AppHandle) -> Option<PathBuf> {
    dir(app).map(|d| d.join("ui-state.json"))
}

/// 不存在、读不了、不是 UTF-8 都当「没有」—— 启动路径上不许抛。读不了和不存在对用户是一回事：用默认值
fn read(p: &Option<PathBuf>) -> Option<String> {
    p.as_ref().and_then(|p| std::fs::read_to_string(p).ok())
}

/// `terminal.shell` 写的路径能不能当 shell 起：是个文件、有执行位。目录、断掉的软链、没 x 位的都不行
pub fn shell_ok(p: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// 新开终端用哪个 shell：设置里写的（`terminal.shell`），空 = `$SHELL`（`ptysvc::Session::spawn_with` 收到空串就用它）。
/// **起的这一刻再判一次**：读设置时还在、后来被删了的（卸载了 fish）也要退回 `$SHELL`，不然终端起不来
pub fn terminal_shell(st: &AppState) -> String {
    let shell = st.settings.view().0.terminal_shell;
    if shell.is_empty() || shell_ok(&shell) {
        return shell;
    }
    applog::write(applog::Level::Warn, "settings", &format!("terminal.shell 指的 {shell} 用不了了，这个终端用的是 $SHELL"));
    String::new()
}

/// 文件监听活多久应用就活多久，放在这儿而不是每个窗口一份
static WATCH: Mutex<Option<fsservice::watch::FileWatch>> = Mutex::new(None);

/// 启动时（`setup` 里，任何窗口的前端开口要设置之前）：读两份文件，起 `settings.json` 的监听。
///
/// **必须在 `setup` 里读完。** 前端挂载之前就 `await settings()`（第 0 步实测一次往返 < 1ms，所以不做首屏缓存），
/// 而同步命令跑在主线程上，`setup` 没跑完前端的请求只能排队 —— 在这儿读好，前端拿到的就一定是读好的值。
pub fn init(app: &AppHandle) {
    let st = app.state::<AppState>();
    let sp = settings_path(app);
    st.settings.load_settings(read(&sp), shell_ok);
    hand_out(&st.settings.current());
    st.settings.load_ui(read(&ui_path(app)).as_deref());
    let (_, problems) = st.settings.view();
    for p in &problems {
        applog::write(applog::Level::Warn, "settings", &p.text());
    }

    let Some(sp) = sp else { return };
    // 监听的是父目录（`watch_file` 的注释）：目录得先在。它就是 windows.json 待的那个目录，多半早就有了
    if let Some(d) = sp.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let a = app.clone();
    match fsservice::watch::watch_file(&sp, move || reload(&a)) {
        Ok(w) => *WATCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(w),
        // 起不来就没有热加载，重启照样生效 —— 记一笔，不挡启动
        Err(e) => applog::write(applog::Level::Warn, "settings", &format!("监听不了设置文件，改了要重启才生效：{e}")),
    }
}

/// 文件监听报「碰过」：重读，真变了才广播（原文没变 `load_settings` 自己会认出来）
fn reload(app: &AppHandle) {
    let text = read(&settings_path(app));
    let st = app.state::<AppState>();
    if st.settings.load_settings(text, shell_ok) {
        hand_out(&st.settings.current());
        crate::diag!("settings-changed（文件）");
        broadcast(app);
    }
}

/// 不在前端用、由 Rust 各处读的那些设置，交给各自的使用方（SETTINGS.md 13.4）：Maven 怎么调。
/// 交出去而不是让 `maven.rs` 来问 `AppState`：`maven.rs` 不依赖 Tauri，单元测试里直接设
fn hand_out(s: &settings::Settings) {
    crate::maven::set_cfg(crate::maven::MavenCfg {
        use_wrapper: s.maven_use_wrapper,
        home: s.maven_home.clone(),
        settings_file: s.maven_settings_file.clone(),
        local_repo: s.maven_local_repository.clone(),
        extra_args: s.maven_extra_args.clone(),
        opts: s.maven_opts.clone(),
    });
    crate::jdks::set_user(s.java_jdks.clone());
}

/// 「此项目」里给你看的代码风格（和起 jdtls 时读的是同一个函数，`java.rs`）
fn code_style(root: &std::path::Path) -> crate::commands::CodeStyleDto {
    use crate::codestyle::{self, Entry};
    let s = codestyle::resolve(root, std::env::var_os("HOME").and_then(|h| codestyle::idea_config_dir(std::path::Path::new(&h))).as_deref());
    let imports = s
        .imports
        .iter()
        .filter_map(|e| match e {
            Entry::Pkg { name, statik } => Some(format!("{}{}", if *statik { "static " } else { "" }, if name.is_empty() { "其余" } else { name })),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" · ");
    crate::commands::CodeStyleDto {
        notes: codestyle::notes(&s),
        origin: s.origin,
        indent: s.indent,
        cont: s.cont,
        margin: s.margin,
        imports,
        star: s.star,
        static_star: s.static_star,
    }
}

pub fn broadcast(app: &AppHandle) {
    let _ = app.emit(EVENT, dto(app));
}

pub fn dto(app: &AppHandle) -> crate::commands::SettingsDto {
    let (e, problems) = app.state::<AppState>().settings.view();
    crate::commands::SettingsDto {
        editor_font_family: e.editor_font_family,
        editor_font_size: e.editor_font_size,
        editor_font_base: e.editor_font_base,
        editor_highlight_usages: e.editor_highlight_usages,
        editor_inlay_hints: e.editor_inlay_hints,
        java_hotswap_on_save: e.java_hotswap_on_save,
        terminal_font_family: e.terminal_font_family,
        terminal_font_size: e.terminal_font_size,
        terminal_shell: e.terminal_shell,
        minimap: e.minimap,
        tree_compact: e.tree_compact,
        tree_follow: e.tree_follow,
        git_grouped: e.git_grouped,
        problems: problems
            .iter()
            .map(|p| crate::commands::SettingProblemDto {
                text: p.text(),
                line: p.pos().map(|x| x.line),
                col: p.pos().map(|x| x.col),
                fatal: p.is_fatal(),
            })
            .collect(),
        path: settings_path(app).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

/// 同一时刻只许一个线程写 `ui-state.json`，而且**改和写在同一把锁里**。
///
/// 命令走阻塞池（碰盘），两个窗口同时按 ⌘= 就是两条线程：`fsservice::write_text` 的临时文件名是固定的，
/// 不排队就是 `windows.json` 那次的并发写坏（winctl 的 `SAVE_LOCK`）；改和写分开拿锁的话，
/// 后改的那份可能先落盘、又被先改的那份盖掉。
static UI_SAVE: Mutex<()> = Mutex::new(());

/// 改界面状态 → 存盘 → 广播。返回改完的样子（发起的窗口直接用，不用等广播）
pub fn update_ui(
    app: &AppHandle,
    f: impl FnOnce(&mut UiState, i64) -> Result<bool, String>,
) -> Result<crate::commands::SettingsDto, String> {
    let changed = {
        let _g = UI_SAVE.lock().unwrap_or_else(|e| e.into_inner());
        let st = app.state::<AppState>();
        let changed = st.settings.update_ui(f)?;
        if changed {
            save_ui_locked(app, &st.settings.ui_text());
        }
        changed
    };
    if changed {
        broadcast(app);
    }
    Ok(dto(app))
}

/// 升级后第一次启动：前端交来的旧 localStorage 偏好（`Store::adopt_legacy` 决定收不收）
pub fn adopt_legacy(app: &AppHandle, legacy: UiState) -> crate::commands::SettingsDto {
    let changed = {
        let _g = UI_SAVE.lock().unwrap_or_else(|e| e.into_inner());
        let st = app.state::<AppState>();
        let changed = st.settings.adopt_legacy(legacy);
        if changed {
            save_ui_locked(app, &st.settings.ui_text());
        }
        changed
    };
    if changed {
        broadcast(app);
    }
    dto(app)
}

fn save_ui_locked(app: &AppHandle, text: &str) {
    let Some(p) = ui_path(app) else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Err(e) = fsservice::write_text(&p, text) {
        applog::write(applog::Level::Warn, "settings", &format!("界面状态存不下：{e}"));
    }
}

/// 「设置…」：`settings.json` 不存在就建一份模板（`settings::template`），返回它的路径，前端在当前窗口打开。
pub fn open_settings(app: &AppHandle) -> Result<String, String> {
    let p = settings_path(app).ok_or("找不到应用数据目录")?;
    ensure_template(&p).map_err(|e| format!("建不了设置文件 {}：{e}", p.display()))?;
    Ok(p.to_string_lossy().into_owned())
}

/// 不存在才建，**已经有了一个字节都不碰**（SETTINGS.md 3.2：那是你写的）。
///
/// 用 `create_new` 而不是「先看在不在、不在再写」：两个窗口同时点「设置…」、或者你恰好在 Finder 里刚建了它，
/// 先查后写中间那一下就会把你的内容换成模板（rust.md「改盘的 std API 默认都会吃掉已有文件」）。
fn ensure_template(p: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(p) {
        Ok(mut f) => f.write_all(settings::template().as_bytes()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

/// 给前端补全用的键定义（`settings::DEFS` 原样转出去）
pub fn schema() -> Vec<crate::commands::SettingDefDto> {
    settings::DEFS
        .iter()
        .map(|d| {
            let (kind, min, max) = match d.kind {
                settings::Kind::Str { .. } => ("string", None, None),
                settings::Kind::Int { min, max } => ("integer", Some(min), Some(max)),
                settings::Kind::Bool => ("boolean", None, None),
                settings::Kind::StrList => ("array", None, None),
            };
            crate::commands::SettingDefDto {
                key: d.key.into(),
                label: d.label.into(),
                kind: kind.into(),
                default: d.default.into(),
                doc: d.doc.into(),
                min,
                max,
            }
        })
        .collect()
}

/// JDK 列表转出去：自动找到的 + 手动加的，手动加的标出来
pub fn jdk_dtos() -> Vec<crate::commands::JdkDto> {
    let manual: Vec<std::path::PathBuf> = crate::jdks::user().iter().filter_map(|d| crate::jdks::at(d)).map(|(_, h)| real(&h)).collect();
    crate::jdks::all()
        .into_iter()
        .map(|(v, h)| crate::commands::JdkDto { version: v, manual: manual.contains(&real(&h)), home: h.to_string_lossy().into_owned() })
        .collect()
}

fn real(p: &std::path::Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// 设置对话框打开时要的全部（docs/SETTINGS.md 第 13 节）。`root`：这个窗口的项目，没有就不给「此项目」那一栏
pub fn page(app: &AppHandle, root: Option<&str>) -> crate::commands::SettingsPageDto {
    let st = app.state::<AppState>();
    let path = settings_path(app);
    let text = read(&path).unwrap_or_default();
    let (_, problems) = st.settings.view();
    let project = root.filter(|r| !r.is_empty()).map(|r| {
        let rp = std::path::Path::new(r);
        let prefs = crate::maven::prefs(rp);
        crate::commands::ProjectPageDto {
            root: r.to_string(),
            jdk: crate::projectcfg::get(rp).jdk,
            auto_jdk: auto_jdk(rp),
            offline: prefs.offline,
            skip_tests: prefs.skip_tests,
            wrapper: rp.join("mvnw").is_file(),
            code_style: code_style(rp),
        }
    });
    crate::commands::SettingsPageDto {
        defs: schema(),
        values: settings::values(&st.settings.current()),
        written: settings::written_keys(&text),
        path: path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        broken: problems.iter().find(|p| p.is_fatal()).map(|p| match p.pos() {
            Some(pos) => format!("第 {} 行：{}", pos.line, p.text()),
            None => p.text(),
        }),
        jdks: jdk_dtos(),
        bad_jdks: crate::jdks::user().into_iter().filter(|d| crate::jdks::at(d).is_none()).collect(),
        project,
    }
}

/// 「按 pom 自动挑」的话挑的是哪个，给人看的一句（设置页上「项目 JDK：自动」旁边那句）
fn auto_jdk(root: &std::path::Path) -> String {
    let rel = crate::maven::reactors(root).into_iter().next().unwrap_or_default();
    match crate::maven::module_java(&root.join(&rel), "") {
        None => "pom 没写 Java 版本：用你默认的那个".into(),
        Some(want) => match lspsvc::jdtls::pick_jdk(&crate::jdks::all(), want) {
            Some((v, h)) => format!("pom 要 Java {want} → Java {v}（{}）", h.display()),
            None => format!("pom 要 Java {want}，这台机器上没有够格的：用你默认的那个"),
        },
    }
}

/// 项目那一栏交回来的
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectApply {
    pub root: String,
    pub jdk: String,
    pub offline: bool,
    pub skip_tests: bool,
}

/// 设置对话框的「应用 / 确定」：全局的一批写进 settings.json（值是 null 就是恢复默认），项目的写进各自的地方。
/// 全局的先全部校验、一个不对一个都不写（同对话框「要么都生效要么都不」的期待）；写完当场重读、广播，不等文件监听
pub fn apply(app: &AppHandle, global: serde_json::Map<String, serde_json::Value>, project: Option<ProjectApply>) -> Result<(), String> {
    if !global.is_empty() {
        for (k, v) in &global {
            if !v.is_null() {
                settings::validate(k, v)?;
            }
        }
        let path = settings_path(app).ok_or("找不到应用数据目录")?;
        ensure_template(&path).map_err(|e| format!("建不了 {}：{e}", path.display()))?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("读不了 {}：{e}", path.display()))?;
        let changes: Vec<(&str, Option<serde_json::Value>)> = global.iter().map(|(k, v)| (k.as_str(), (!v.is_null()).then(|| v.clone()))).collect();
        let next = settings::edit(&text, &changes).map_err(|p| match p.pos() {
            Some(pos) => format!("settings.json 第 {} 行有错，先修好它再用设置页：{}", pos.line, p.text()),
            None => format!("settings.json 读不懂，先修好它再用设置页：{}", p.text()),
        })?;
        if next != text {
            fsservice::write_text(&path, &next).map_err(|e| format!("写不了 {}：{e}", path.display()))?;
        }
        reload(app);
    }
    if let Some(p) = project {
        let rp = std::path::Path::new(&p.root);
        let mut cfg = crate::projectcfg::get(rp);
        cfg.jdk = p.jdk.trim().to_string();
        crate::projectcfg::set(rp, cfg).map_err(|e| format!("记不下项目设置：{e}"))?;
        let mut prefs = crate::maven::prefs(rp);
        prefs.offline = p.offline;
        prefs.skip_tests = p.skip_tests;
        crate::maven::set_prefs(rp, &prefs).map_err(|e| format!("记不下 Maven 的开关：{e}"))?;
    }
    Ok(())
}

/// 运行配置对话框：这个项目认出来的 Spring 服务和各自的配置
pub fn run_configs(root: &str) -> crate::commands::RunConfigsDto {
    let rp = std::path::Path::new(root);
    let cfg = crate::projectcfg::get(rp);
    let (tasks, _) = crate::taskdefs::spring_tasks(rp);
    let (idea, idea_problems) = crate::ideacfg::read(rp);
    let services = tasks
        .iter()
        .map(|t| {
            let r = cfg.runs.get(&t.def.name).cloned().unwrap_or_default();
            // 按主类对上；好几个服务同一个主类（各模块一个 Application）时再按模块名分，IDEA 的模块名一般是 artifactId、我们这边是目录名
            let same_main = tasks.iter().filter(|o| o.app.main_class == t.app.main_class).count();
            let dir = t.app.module.rsplit('/').next().unwrap_or("");
            let idea = idea
                .iter()
                .filter(|i| i.main_class == t.app.main_class && (same_main == 1 || i.module == dir || i.module.is_empty()))
                .map(|i| crate::commands::IdeaRunDto {
                    name: i.name.clone(),
                    profiles: i.cfg.profiles.clone(),
                    vm_options: i.cfg.vm_options.clone(),
                    program_args: i.cfg.program_args.clone(),
                    env: i.cfg.env.iter().map(|(name, value)| crate::commands::EnvVarDto { name: name.clone(), value: value.clone() }).collect(),
                    jdk: i.cfg.jdk.clone(),
                    notes: i.notes.clone(),
                })
                .collect();
            crate::commands::RunConfigDto {
                main_class: t.app.main_class.clone(),
                module: match &t.def.source {
                    crate::taskdefs::Source::Spring { module } => module.clone(),
                    _ => String::new(),
                },
                name: t.def.name.clone(),
                idea,
                profiles: r.profiles,
                vm_options: r.vm_options,
                program_args: r.program_args,
                env: r.env.into_iter().map(|(name, value)| crate::commands::EnvVarDto { name, value }).collect(),
                jdk: r.jdk,
            }
        })
        .collect();
    let found = crate::taskdefs::discover(rp);
    let user = crate::projectcfg::get(rp).configs.into_iter().map(user_dto).collect();
    let others = found
        .defs
        .iter()
        .filter_map(|d| match d.source {
            crate::taskdefs::Source::Package { .. } => Some("package"),
            crate::taskdefs::Source::File => Some("file"),
            _ => None,
        }.map(|s| crate::commands::OtherRunDto { name: d.name.clone(), source: s.into(), command: d.command.clone() }))
        .collect();
    let mut modules = Vec::new();
    for r in crate::maven::reactors(rp) {
        for m in crate::maven::modules(&rp.join(&r)) {
            modules.push([r.as_str(), m.rel.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("/"));
        }
    }
    crate::commands::RunConfigsDto { services, jdks: jdk_dtos(), idea_problems, user, others, modules, problems: found.problems }
}

fn user_dto(u: crate::projectcfg::UserCfg) -> crate::commands::UserConfigDto {
    crate::commands::UserConfigDto {
        name: u.name,
        kind: u.kind,
        main_class: u.main_class,
        module: u.module,
        goals: u.goals,
        command: u.command,
        cwd: u.cwd,
        items: u.items,
        profiles: u.run.profiles,
        vm_options: u.run.vm_options,
        program_args: u.run.program_args,
        env: u.run.env.into_iter().map(|(name, value)| crate::commands::EnvVarDto { name, value }).collect(),
        jdk: u.run.jdk,
        host: u.host,
        port: u.port,
    }
}

/// 界面交回来的那份 → 存的样子
pub fn user_cfg(d: crate::commands::UserConfigDto) -> crate::projectcfg::UserCfg {
    crate::projectcfg::UserCfg {
        name: d.name,
        kind: d.kind,
        main_class: d.main_class.trim().to_string(),
        module: d.module,
        goals: d.goals,
        command: d.command,
        cwd: d.cwd.trim().trim_matches('/').to_string(),
        items: d.items,
        run: crate::projectcfg::RunCfg {
            profiles: d.profiles,
            vm_options: d.vm_options,
            program_args: d.program_args,
            env: d.env.into_iter().map(|e| (e.name.trim().to_string(), e.value)).filter(|(k, _)| !k.is_empty()).collect(),
            jdk: d.jdk,
        },
        host: d.host.trim().to_string(),
        port: d.port,
    }
}

/// 远程调试的主机：主机名、IPv4、IPv6（方括号也认）的字符，不能空
fn host_ok(h: &str) -> bool {
    !h.is_empty() && h.len() <= 255 && h.chars().all(|c| c.is_ascii_alphanumeric() || ".-:_[]".contains(c))
}

/// 你建的运行配置（docs/TAKEOVER.md R1）配不配得出来。说不行时一句人话，界面原样显示
fn check_user(root: &std::path::Path, old: &str, u: &crate::projectcfg::UserCfg) -> Result<(), String> {
    let name = u.name.as_str();
    if name.is_empty() || name.contains(['\n', '\r']) {
        return Err("名字不能空着".into());
    }
    if !crate::projectcfg::KINDS.contains(&u.kind.as_str()) {
        return Err(format!("不认识的运行配置类型：{}", u.kind));
    }
    // 名字要和列出来的都不一样（认出来的、tasks.json 里的、你建的别的）；改的是自己就不算撞
    let found = crate::taskdefs::discover(root);
    let taken = |n: &str| found.defs.iter().any(|d| d.name == n && !(n == old && matches!(d.source, crate::taskdefs::Source::User { .. })));
    if taken(name) {
        return Err(format!("已经有一份叫「{name}」的了"));
    }
    if let Some((k, _)) = u.run.env.iter().find(|(k, _)| !crate::projectcfg::env_name_ok(k)) {
        return Err(format!("环境变量名「{k}」不行：要字母或下划线开头，后面只有字母、数字、下划线"));
    }
    let module_ok = |m: &str| root.join(m).join("pom.xml").is_file();
    match u.kind.as_str() {
        "application" => {
            if !crate::projectcfg::main_class_ok(&u.main_class) {
                return Err("主类要写全名（com.demo.Tool）".into());
            }
            if !module_ok(&u.module) {
                return Err(format!("「{}」底下没有 pom.xml：要选一个 Maven 模块（主类在它的 src/main/java 里）", if u.module.is_empty() { "项目根" } else { &u.module }));
            }
        }
        "maven" => {
            if !module_ok(&u.module) {
                return Err(format!("「{}」底下没有 pom.xml", if u.module.is_empty() { "项目根" } else { &u.module }));
            }
            if crate::maven::goal_command("", &u.goals, &crate::maven::Prefs::default(), "mvn").is_none() {
                return Err("目标要填（空格隔开，比如 clean package），只能有字母、数字和 - _ . : = , / @ 这几个符号".into());
            }
        }
        "shell" => {
            if u.command.trim().is_empty() {
                return Err("命令不能空着".into());
            }
        }
        "remote" => {
            if !host_ok(&u.host) {
                return Err("主机要填（localhost、IP 或者主机名）".into());
            }
            if u.port == 0 || u.port > 65535 {
                return Err("端口要在 1–65535 之间（对面 JVM 的 address=*:5005 里那个数）".into());
            }
        }
        _ => {
            if u.items.is_empty() {
                return Err("组合配置至少要选一份".into());
            }
            for it in &u.items {
                if it == name {
                    return Err("组合配置不能包含它自己".into());
                }
                match found.defs.iter().find(|d| &d.name == it) {
                    None => return Err(format!("没有叫「{it}」的运行配置")),
                    // 套组合会转圈（a 套 b、b 又套 a），IDEA 也不让
                    Some(d) if matches!(&d.source, crate::taskdefs::Source::User { kind, .. } if kind == "compound") => return Err(format!("「{it}」本身是组合配置，组合里不能再套组合")),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// 存一份你建的运行配置：新建（`old` 空）、改、改名。改名时组合配置里写着旧名字的跟着改
pub fn set_user_config(root: &str, old: &str, mut u: crate::projectcfg::UserCfg) -> Result<(), String> {
    let rp = std::path::Path::new(root);
    u.name = u.name.trim().to_string();
    u.module = u.module.trim().trim_matches('/').to_string();
    check_user(rp, old, &u)?;
    let mut cfg = crate::projectcfg::get(rp);
    let new = u.name.clone();
    match cfg.configs.iter().position(|c| !old.is_empty() && c.name == old) {
        Some(i) => cfg.configs[i] = u,
        None => cfg.configs.push(u),
    }
    if !old.is_empty() && old != new {
        for c in &mut cfg.configs {
            for it in &mut c.items {
                if it == old {
                    *it = new.clone();
                }
            }
        }
    }
    crate::projectcfg::set(rp, cfg).map_err(|e| format!("记不下运行配置：{e}"))
}

/// 删一份你建的运行配置；组合配置里写着它的也拿掉（拿空了的组合留着，界面上会说它空了）
pub fn delete_user_config(root: &str, name: &str) -> Result<(), String> {
    let rp = std::path::Path::new(root);
    let mut cfg = crate::projectcfg::get(rp);
    cfg.configs.retain(|c| c.name != name);
    for c in &mut cfg.configs {
        c.items.retain(|it| it != name);
    }
    crate::projectcfg::set(rp, cfg).map_err(|e| format!("记不下运行配置：{e}"))
}

/// 记下一个服务的运行配置。环境变量名不像变量名的拒掉（进 shell 会变成别的意思），说清是哪个
pub fn set_run_config(root: &str, name: &str, run: crate::projectcfg::RunCfg) -> Result<(), String> {
    if let Some((k, _)) = run.env.iter().find(|(k, _)| !crate::projectcfg::env_name_ok(k)) {
        return Err(format!("环境变量名「{k}」不行：要字母或下划线开头，后面只有字母、数字、下划线"));
    }
    let rp = std::path::Path::new(root);
    let mut cfg = crate::projectcfg::get(rp);
    cfg.runs.insert(name.to_string(), run);
    crate::projectcfg::set(rp, cfg).map_err(|e| format!("记不下运行配置：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 你建的运行配置（docs/TAKEOVER.md R1）配不出来时说清为什么：名字撞了、主类没写全、模块底下没 pom、目标带 shell 特殊字符、组合包含自己 / 没有的
    #[test]
    fn 你建的运行配置_配不出来说清为什么() {
        use crate::projectcfg::{RunCfg, UserCfg};
        let d = std::env::temp_dir().join(format!("lite-ide-usercfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("order")).unwrap();
        std::fs::create_dir_all(d.join(".lite-ide")).unwrap();
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        let w = |p: &str, t: &str| std::fs::write(d.join(p), t).unwrap();
        w("pom.xml", "<project><artifactId>shop</artifactId><modules><module>order</module></modules></project>");
        w("order/pom.xml", "<project><artifactId>order</artifactId></project>");
        w(".lite-ide/tasks.json", r#"[{ "name": "后端", "command": "mvn spring-boot:run" }]"#);
        let app = |name: &str, main: &str, module: &str| UserCfg { name: name.into(), kind: "application".into(), main_class: main.into(), module: module.into(), ..Default::default() };
        let err = |u: UserCfg| check_user(&d, "", &u).err().unwrap_or_default();
        assert_eq!(check_user(&d, "", &app("工具", "com.demo.Tool", "order")), Ok(()));
        assert!(err(app("后端", "a.B", "order")).contains("已经有一份叫「后端」"), "和 tasks.json 里的撞名");
        assert!(err(app("工具", "com.demo.", "order")).contains("全名"), "主类没写全");
        assert!(err(app("工具", "1x.Tool", "order")).contains("全名"));
        assert!(err(app("工具", "a.B", "nope")).contains("没有 pom.xml"), "模块底下没 pom");
        // 空白名字由 set_user_config 先 trim 成空串再交过来
        assert!(err(app("", "a.B", "order")).contains("名字"), "名字空着");
        let mvn = |goals: &str| UserCfg { name: "打包".into(), kind: "maven".into(), module: "order".into(), goals: goals.into(), ..Default::default() };
        assert_eq!(check_user(&d, "", &mvn("clean package -DskipTests")), Ok(()));
        assert!(err(mvn("clean && rm -rf ~")).contains("目标"), "目标带 shell 特殊字符");
        assert!(err(mvn("")).contains("目标"));
        let comp = |items: &[&str]| UserCfg { name: "全部".into(), kind: "compound".into(), items: items.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        assert_eq!(check_user(&d, "", &comp(&["后端"])), Ok(()));
        assert!(err(comp(&["全部"])).contains("它自己"));
        assert!(err(comp(&["没这个"])).contains("没有叫「没这个」"));
        assert!(err(comp(&[])).contains("至少"));
        let shell = UserCfg { name: "脚本".into(), kind: "shell".into(), command: "echo hi".into(), run: RunCfg { env: vec![("bad-name".into(), "x".into())], ..Default::default() }, ..Default::default() };
        assert!(err(shell).contains("环境变量名「bad-name」"));
        let remote = |host: &str, port: u32| UserCfg { name: "远程".into(), kind: "remote".into(), host: host.into(), port, ..Default::default() };
        assert_eq!(check_user(&d, "", &remote("10.0.0.5", 5005)), Ok(()));
        assert_eq!(check_user(&d, "", &remote("[::1]", 5005)), Ok(()), "IPv6 带方括号");
        assert!(err(remote("", 5005)).contains("主机"));
        assert!(err(remote("a;rm -rf ~", 5005)).contains("主机"), "主机名里不许有 shell 的字符（它会拼进占位的那条命令）");
        assert!(err(remote("localhost", 0)).contains("端口"));
        assert!(err(remote("localhost", 70000)).contains("端口"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 设置文件_不存在才建模板_已经有了一个字节都不碰() {
        let d = std::env::temp_dir().join(format!("lite-ide-settings-tpl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("sub/settings.json");
        ensure_template(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), settings::template(), "不存在：建出模板（目录也建出来）");
        std::fs::write(&p, "{ \"editor.fontSize\": 15 } // 我写的").unwrap();
        ensure_template(&p).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(after, "{ \"editor.fontSize\": 15 } // 我写的", "已经有了：不能换成模板");
    }
}
