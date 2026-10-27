//! 任务从哪来（#48 第 2 步，docs/TASKS.md Q2）：项目里的 `.lite-ide/tasks.json`，加上自动认出来的 `package.json` scripts、
//! 语言包声明的任务（docs/LANGPACK.md 第 7 节，比如 Rust 包的 `cargo test`）。
//!
//! 纯函数 + 读盘，不碰进程（起和停在 `tasksvc`）。放在主 crate 里、挨着 `settings.rs`：`tasks.json` 和 `settings.json` 是同一种 JSONC
//! （注释、末尾逗号），用的是同一个 [`crate::settings::strip`] —— 一种格式一个解析器，不另写一份。
//!
//! **为什么不自动认 Python**：Python 跑哪个入口，猜错比不猜糟（Q2）。（Gradle 项目里的 Spring Boot 应用 2026-10-26 起认了，同 Maven，见 `gradle.rs`）
//! package.json 不一样：`scripts` 就是作者自己写下的「这个项目能跑什么」，照着列出来不算猜。
//! **Spring Boot 应用自动认**（2026-10-14，docs/IDEA.md 第 1 步）：`@SpringBootApplication` + `main` 同样是作者写下的声明，
//! 不带 profile 起（IDEA 的默认）—— Q2 当初担心的「要不要带 profile」由 tasks.json 里同名的任务回答。怎么认、怎么起在 `maven.rs`

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// 任务定义文件，相对项目根
pub const FILE: &str = ".lite-ide/tasks.json";
/// 读文件的上限：`tasks.json`、`package.json` 都是人写的几 KB，超过这个数多半不是我们要的那种文件，不读进内存
const MAX_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// `.lite-ide/tasks.json` 里写的
    File,
    /// 某个 `package.json` 的 scripts；`dir` 相对项目根（根目录是 ""）
    Package { dir: String },
    /// 语言包声明的（`langs::PackTasks`，docs/LANGPACK.md 第 7 节）：`group` 是运行列表里那一组的名字（「Cargo」），`dir` 同上
    Pack { group: String, dir: String },
    /// Maven 项目里认出来的 Spring Boot 应用（`maven.rs`）；`module` 相对项目根。在 Services 工具窗里跑（同 IDEA），不进运行窗
    Spring { module: String },
    /// Maven 工具窗里点的目标（「artifactId [目标]」）：在运行窗里跑。和 `Spring` 分开，是因为两种跑起来去的工具窗不一样
    Goal { module: String },
    /// 你自己建的运行配置（`projectcfg::UserCfg`，docs/TAKEOVER.md R1）：`kind` 是 application / maven / shell / compound。
    /// compound 没有命令（几份一起起，前端一份份起），`items` 是那几份的名字
    User { kind: String, items: Vec<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    pub name: String,
    /// 一整行，交给登录 shell
    pub command: String,
    /// 相对项目根；"" = 根
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub source: Source,
}

#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub defs: Vec<Def>,
    /// `tasks.json` 在不在（列表底部给「打开 tasks.json」还是「新建 tasks.json」）
    pub file: bool,
    /// 读的时候发现的问题。**都要告诉人**（同 settings.json 的规矩）：静默跳过会让人以为任务写对了
    pub problems: Vec<String>,
}

/// 新建 `tasks.json` 时的模板：三个常见的例子，全注释掉 —— 解析出来是空数组，人照着改
pub fn template() -> &'static str {
    r#"// lite-ide 的任务（docs/USAGE.md「任务」）。⌃⌥R 选一个跑，⌃R 再跑上一个，⌘F2 停。
// 每个任务：name（列表里显示的名字）、command（一整行，交给你的登录 shell，和在终端里敲一样）、
// cwd（相对项目根，可以不写）、env（额外的环境变量，可以不写）。
// package.json 里的 scripts 不用写在这儿，会自动列出来；同名的以这里为准。
[
  // { "name": "后端", "command": "mvn spring-boot:run", "cwd": "admin" },
  // { "name": "前端", "command": "pnpm dev", "cwd": "web" },
  // { "name": "脚本", "command": "python3 main.py", "env": { "APP_ENV": "dev" } },
]
"#
}

/// 解析 `tasks.json` 的文本。坏掉的那一个任务跳过、说一句，别的照常 —— 一处写错不该让整份都不能用
pub fn parse_file(text: &str) -> (Vec<Def>, Vec<String>) {
    let clean = match crate::settings::strip(text) {
        Ok(c) => c,
        Err(p) => return (Vec::new(), vec![format!("{FILE} 格式错误：{}", p.text())]),
    };
    if clean.trim().is_empty() {
        return (Vec::new(), Vec::new());
    }
    let value: Value = match serde_json::from_str(&clean) {
        Ok(v) => v,
        Err(e) => {
            let pos = crate::settings::pos_of_line_byte(text, e.line(), e.column());
            let msg = if e.classify() == serde_json::error::Category::Eof {
                "内容不完整，可能缺少 ] 或引号未闭合".to_string()
            } else {
                e.to_string().split(" at line ").next().unwrap_or_default().to_string()
            };
            return (Vec::new(), vec![format!("{FILE} 第 {} 行有错误：{msg}", pos.line)]);
        }
    };
    let Value::Array(items) = value else {
        return (Vec::new(), vec![format!("{FILE} 的顶层必须是数组 [ … ]（任务列表）")]);
    };
    let mut defs: Vec<Def> = Vec::new();
    let mut problems = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let Value::Object(o) = item else {
            problems.push(format!("{FILE} 中第 {n} 个任务必须是对象 {{ … }}，已跳过"));
            continue;
        };
        let name = o.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
        let command = o.get("command").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if name.is_empty() || command.is_empty() {
            problems.push(format!("{FILE} 中第 {n} 个任务缺少 name 或 command（须为非空字符串），已跳过"));
            continue;
        }
        let who = format!("{FILE} 中的「{name}」");
        for k in o.keys() {
            if !matches!(k.as_str(), "name" | "command" | "cwd" | "env") {
                // 拼错时（`cmd`、`dir`）这是唯一的线索 —— 不说的话它照样跑，只是没在你以为的目录里
                problems.push(format!("{who}：未知的键 {k}（支持 name、command、cwd、env）"));
            }
        }
        let cwd = match o.get("cwd") {
            None => String::new(),
            Some(Value::String(s)) => match inside(s) {
                Some(c) => c,
                None => {
                    problems.push(format!("{who}：cwd 须为项目内的相对路径（不能为绝对路径或包含 ..），已跳过"));
                    continue;
                }
            },
            Some(_) => {
                problems.push(format!("{who}：cwd 须为字符串，已跳过"));
                continue;
            }
        };
        let mut env = Vec::new();
        match o.get("env") {
            None => {}
            Some(Value::Object(m)) => {
                for (k, v) in m {
                    match v.as_str() {
                        Some(v) => env.push((k.clone(), v.to_string())),
                        None => problems.push(format!("{who}：env 中的 {k} 须为字符串，已忽略")),
                    }
                }
            }
            Some(_) => problems.push(format!("{who}：env 须为 {{ \"名称\": \"值\" }} 格式，已忽略")),
        }
        if defs.iter().any(|d| d.name == name) {
            problems.push(format!("{who} 名称重复，使用第一个"));
            continue;
        }
        defs.push(Def { name: name.to_string(), command: command.to_string(), cwd, env, source: Source::File });
    }
    (defs, problems)
}

/// `cwd` 规整成项目里的相对路径；绝对路径、`..` 出去 → None
fn inside(s: &str) -> Option<String> {
    let p = Path::new(s.trim());
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(x) => parts.push(x.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// `package.json` 里的 scripts，**按文件里的顺序**。
///
/// serde_json 的 `Map` 没开 `preserve_order`，读出来按键名排序 —— `dev` 会排到 `build` 后面，而作者是按「最常用的在前」写的。
/// 为这一处去开整个依赖树的 feature 不值，这里自己收：反序列化时键本来就是按文档顺序一个个交过来的
struct Ordered(Vec<(String, String)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("scripts 对象")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<Ordered, M::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = m.next_entry::<String, Value>()? {
                    // 值不是字符串的（坏掉的 package.json）跳过，不让一条坏的拖垮整份
                    if let Value::String(s) = v {
                        out.push((k, s));
                    }
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    #[serde(default)]
    scripts: Option<Ordered>,
    /// corepack 的约定：`"packageManager": "pnpm@9.1.0"`。比看锁文件准（锁文件可能是别人提交错的）
    #[serde(default)]
    package_manager: Option<String>,
}

/// npm 自己在 install / publish / version 时会跑的那些，不是给人手动跑的；`preX` / `postX` 是 `X` 的附属，跑 `X` 时 npm 自己带上
const LIFECYCLE: &[&str] = &[
    "preinstall", "install", "postinstall", "prepublish", "preprepare", "prepare", "postprepare", "prepublishOnly",
    "prepack", "postpack", "publish", "postpublish", "preversion", "version", "postversion", "dependencies",
];

/// 一份 `package.json` 能跑的 scripts（去掉生命周期钩子）和它声明的包管理器
pub fn package_scripts(text: &str) -> Result<(Vec<String>, Option<String>), String> {
    let p: PackageJson = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let all = p.scripts.map(|o| o.0).unwrap_or_default();
    let names: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
    let hook = |k: &str| {
        ["pre", "post"].iter().any(|pre| k.strip_prefix(pre).is_some_and(|rest| names.contains(&rest)))
    };
    let runnable = all.iter().map(|(k, _)| k.clone()).filter(|k| !LIFECYCLE.contains(&k.as_str()) && !hook(k)).collect();
    let pm = p.package_manager.and_then(|s| {
        let n = s.split('@').next().unwrap_or_default().to_string();
        ["pnpm", "yarn", "npm", "bun"].contains(&n.as_str()).then_some(n)
    });
    Ok((runnable, pm))
}

/// 用哪个包管理器：声明了就听声明的，否则从这个目录往上（到项目根为止）找锁文件，都没有就 npm
fn package_manager(dir: &Path, root: &Path, declared: Option<String>) -> String {
    if let Some(d) = declared {
        return d;
    }
    let mut cur = Some(dir);
    while let Some(d) = cur {
        for (lock, pm) in [("pnpm-lock.yaml", "pnpm"), ("yarn.lock", "yarn"), ("bun.lock", "bun"), ("bun.lockb", "bun"), ("package-lock.json", "npm")] {
            if d.join(lock).is_file() {
                return pm.into();
            }
        }
        if d == root {
            break;
        }
        cur = d.parent();
    }
    "npm".into()
}

/// 放进 shell 命令行的一个词。scripts 名字常带 `:`（`build:prod`），那个不用引；带空格、引号之类的才套单引号
fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-:./@+=,%".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

fn read_capped(p: &Path) -> Option<Result<String, String>> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > MAX_BYTES {
        return Some(Err(format!("超过 {} KB，未读取", MAX_BYTES / 1024)));
    }
    Some(std::fs::read_to_string(p).map_err(|e| e.to_string()))
}

/// 列出一个项目的全部任务：`tasks.json` 里的在前，然后根目录的 scripts，再然后一层子目录的（按目录名）。
/// 和 `tasks.json` 里同名的自动任务不列（以文件为准，Q2）
pub fn discover(root: &Path) -> Found {
    let mut found = Found::default();
    match read_capped(&root.join(FILE)) {
        None => {}
        Some(Err(e)) => {
            found.file = true;
            found.problems.push(format!("无法读取 {FILE}：{e}"));
        }
        Some(Ok(text)) => {
            found.file = true;
            let (defs, problems) = parse_file(&text);
            found.defs = defs;
            found.problems = problems;
        }
    }

    // 根目录 + 一层子目录。不往下翻：monorepo 的包一般就在第一层（apps/、packages/ 底下那一层是例外，第一版不管）
    let subs = first_level(root);
    for dir in std::iter::once(root.to_path_buf()).chain(subs.iter().cloned()) {
        let rel = dir.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
        let pj = dir.join("package.json");
        let (scripts, declared) = match read_capped(&pj) {
            None => continue,
            Some(Err(e)) => {
                found.problems.push(format!("无法读取 {}：{e}", shown(&rel, "package.json")));
                continue;
            }
            Some(Ok(text)) => match package_scripts(&text) {
                Ok(x) => x,
                Err(e) => {
                    found.problems.push(format!("无法解析 {}：{e}", shown(&rel, "package.json")));
                    continue;
                }
            },
        };
        if scripts.is_empty() {
            continue;
        }
        let pm = package_manager(&dir, root, declared);
        for s in scripts {
            let name = if rel.is_empty() { s.clone() } else { format!("{rel}/{s}") };
            if found.defs.iter().any(|d| d.name == name) {
                continue;
            }
            found.defs.push(Def {
                name,
                command: format!("{pm} run {}", shell_word(&s)),
                cwd: rel.clone(),
                env: Vec::new(),
                source: Source::Package { dir: rel.clone() },
            });
        }
    }
    pack_tasks(root, &subs, &mut found);
    spring_apps(root, &mut found);
    user_configs(root, &mut found);
    found
}

/// 第一层子目录，按名字排；node_modules、隐藏目录（`.git`、`target` 不算隐藏，但它底下不会有标记文件）不进
fn first_level(root: &Path) -> Vec<PathBuf> {
    let mut subs: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.') && n != "node_modules"))
                .collect()
        })
        .unwrap_or_default();
    subs.sort();
    subs
}

/// 语言包声明的任务（docs/LANGPACK.md 第 7 节）。标记文件在根上就只列根上的一份 —— Cargo 工作区在根上 `cargo test` 就是全部成员，
/// 每个成员 crate 再列一份是噪音；根上没有才看第一层子目录（一个仓库里前后端各一个工程）。同名的让给 tasks.json、scripts
fn pack_tasks(root: &Path, subs: &[PathBuf], found: &mut Found) {
    for p in crate::langs::all_packs().0 {
        let Some(t) = p.tasks else { continue };
        let dirs: Vec<String> = if root.join(&t.marker).is_file() {
            vec![String::new()]
        } else {
            subs.iter().filter(|d| d.join(&t.marker).is_file()).filter_map(|d| d.strip_prefix(root).ok()).map(|r| r.to_string_lossy().into_owned()).collect()
        };
        for rel in dirs {
            for it in &t.items {
                let name = if rel.is_empty() { it.name.clone() } else { format!("{rel}/{}", it.name) };
                if found.defs.iter().any(|d| d.name == name) {
                    continue;
                }
                found.defs.push(Def { name, command: it.command.clone(), cwd: rel.clone(), env: Vec::new(), source: Source::Pack { group: t.group.clone(), dir: rel.clone() } });
            }
        }
    }
}

/// 一个模块目录（相对项目根）→（它在哪个 reactor 下、相对那个 reactor 的路径）。命令要在 reactor 根上跑（`-pl 模块 -am`），
/// 和认出来的 Spring 服务一样。根上有 pom 就整个项目是一个 reactor
fn split_module(root: &Path, module: &str) -> (String, String) {
    let mut rs = crate::maven::reactors(root);
    rs.sort_by_key(|r| std::cmp::Reverse(r.len()));
    for r in rs {
        if r.is_empty() {
            return (String::new(), module.to_string());
        }
        if module == r {
            return (r, String::new());
        }
        if let Some(rest) = module.strip_prefix(&format!("{r}/")) {
            return (r, rest.to_string());
        }
    }
    (String::new(), module.to_string())
}

/// 你自己建的运行配置（docs/TAKEOVER.md R1）→ 任务。拼法全用现成的：application 是 `maven::run_command`（和认出来的 Spring 服务同一个，
/// 调试插 JDWP、热替换带代理认的「`exec java `」形状照旧），maven 是 `goal_command`，都在最前面带上该用的 JDK（`jdk_prefix_for`）。
/// 和已经列出来的同名的不列、说一句：认出来的和 tasks.json 里的名字是别处定的，你建的那份改个名就好
fn user_configs(root: &Path, found: &mut Found) {
    let configs = crate::projectcfg::get(root).configs;
    if !configs.is_empty() {
        user_defs(root, configs, found);
    }
}

/// [`user_configs`] 的身子：配置从参数来（测试不用碰应用那份文件 —— 它的路径是进程里设一次的）
fn user_defs(root: &Path, configs: Vec<crate::projectcfg::UserCfg>, found: &mut Found) {
    let prefs = crate::maven::prefs(root);
    for u in configs {
        if found.defs.iter().any(|d| d.name == u.name) {
            found.problems.push(format!("运行配置「{}」与其他配置重名，未列出，请重命名", u.name));
            continue;
        }
        let (command, cwd, env) = match u.kind.as_str() {
            "application" => {
                let (r, m) = split_module(root, &u.module);
                let app = crate::maven::BootApp { module: m.clone(), main_class: u.main_class.clone() };
                let mvn = crate::maven::invocation(&root.join(&r));
                (format!("{}{}", jdk_prefix_for(root, &r, &m, &u.run.jdk), crate::maven::run_command(&app, &prefs, &mvn, &u.run)), r, Vec::new())
            }
            "maven" => {
                let (r, m) = split_module(root, &u.module);
                let mvn = crate::maven::invocation(&root.join(&r));
                let Some(c) = crate::maven::goal_command(&m, &u.goals, &prefs, &mvn) else {
                    found.problems.push(format!("运行配置「{}」的 Maven 目标包含无效字符，未列出", u.name));
                    continue;
                };
                (format!("{}{c}", jdk_prefix_for(root, &r, &m, &u.run.jdk)), r, u.run.env.clone())
            }
            "shell" => (u.command.clone(), u.cwd.clone(), u.run.env.clone()),
            "compound" => (String::new(), String::new(), Vec::new()),
            // 远程 JVM 调试（docs/TAKEOVER.md G23）：没有要起的程序，起一个只占位、一直不退的任务 —— 运行窗里那一格就是这次挂接，
            // ⌘F2 停它 = 断开（只断开，不停对面的 JVM：java-debug 对 attach 来的会话断线时做的是 JDI 的 dispose）。日志断点那句也打进这一格
            "remote" => (remote_command(&u.host, u.port), String::new(), Vec::new()),
            _ => continue,
        };
        found.defs.push(Def { name: u.name, command, cwd, env, source: Source::User { kind: u.kind, items: u.items } });
    }
}

/// 远程调试那一格的「命令」：先说一句挂在哪、怎么断开，然后一直不退（`tail -f /dev/null` 不占 CPU、不读输入）。
/// host 只认主机名 / IP 的字符（配置里已经校验过），拼进 printf 的参数里也不会被当成命令
fn remote_command(host: &str, port: u32) -> String {
    let h: String = host.chars().filter(|c| c.is_ascii_alphanumeric() || ".-:_[]".contains(*c)).collect();
    format!("printf '正在调试 %s:%s 上的 JVM（⌘F2 断开，不会终止该 JVM）\\n' '{h}' '{port}'; exec tail -f /dev/null")
}

/// 远程调试的配置要挂到哪（`remote` 那种）；不是远程调试 → None。⌃R / ▶ 时 `task_run` 问一句
pub fn remote_target(root: &Path, name: &str) -> Option<(String, u16)> {
    crate::projectcfg::get(root)
        .configs
        .into_iter()
        .find(|u| u.name == name && u.kind == "remote")
        .and_then(|u| Some((u.host, u16::try_from(u.port).ok()?)))
}

/// 组合配置（compound）的那几份；不是组合配置 → None。只读应用自己的那份文件，不扫项目 —— 每次 ⌃R 之前前端问一句，要快
pub fn compound_items(root: &Path, name: &str) -> Option<Vec<String>> {
    crate::projectcfg::get(root).configs.into_iter().find(|u| u.name == name && u.kind == "compound").map(|u| u.items)
}

/// 一个认出来的 Spring Boot 应用：它的任务定义，加上它在哪个 reactor、哪个模块。
/// Maven 工具窗照 IDEA 重做之后不列应用了（IDEA 的 Maven 窗里也没有），Services 工具窗列它们（`services_list`）
pub struct SpringTask {
    pub def: Def,
    pub app: crate::maven::BootApp,
}

/// Maven 项目里的 Spring Boot 应用（`maven.rs`）。名字是主类的简单名（同 IDEA）；两个模块里撞了名就带上模块路径。
/// 任务列表和 Maven 工具窗都从这一处拿，名字两边一样 —— 工具窗里点运行，和 ⌃⌥R 里选它是同一个任务（重跑会先停旧的）
pub fn spring_tasks(root: &Path) -> (Vec<SpringTask>, Vec<String>) {
    let prefs = crate::maven::prefs(root);
    let mut apps: Vec<(String, crate::maven::BootApp)> = Vec::new();
    let mut problems = Vec::new();
    for rel in crate::maven::reactors(root) {
        let r = root.join(&rel);
        let mods = crate::maven::modules(&r);
        let (found_apps, ps) = crate::maven::boot_apps(&r, &mods);
        problems.extend(ps);
        apps.extend(found_apps.into_iter().map(|a| (rel.clone(), a)));
    }
    // Gradle 的（docs/TAKEOVER.md G24）：入口照样按 `@SpringBootApplication` + `main` 认（同一个函数），命令换成 Gradle 的。
    // 记下哪几个是 Gradle 的（构建根, 模块目录）→ Gradle 项目路径
    let mut gradle: Vec<(String, String, String)> = Vec::new();
    for rel in crate::gradle::roots(root) {
        let r = root.join(&rel);
        let mods = crate::gradle::modules(&r);
        let (found_apps, ps) = crate::maven::boot_apps(&r, &mods);
        problems.extend(ps);
        for a in found_apps {
            if let Some(m) = mods.iter().find(|m| m.rel == a.module) {
                gradle.push((rel.clone(), a.module.clone(), m.artifact_id.clone()));
            }
            apps.push((rel.clone(), a));
        }
    }
    let out = apps
        .iter()
        .map(|(rel, app)| {
            let short = app.name();
            let dup = apps.iter().filter(|(_, a)| a.name() == short).count() > 1;
            let module = [rel.as_str(), app.module.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("/");
            let name = if dup { format!("{module}/{short}") } else { short.to_string() };
            // 运行配置（docs/SETTINGS.md 13.6）：按任务名存，Services 和 ⌃⌥R 里看到的就是这个名字
            let run = crate::projectcfg::run(root, &name);
            let launch = match gradle.iter().find(|(r, m, _)| r == rel && *m == app.module) {
                Some((_, _, project)) => crate::gradle::run_command(project, &app.module, &app.main_class, &crate::gradle::invocation(&root.join(rel)), &run)
                    .unwrap_or_else(|| "echo '[lite-ide] 无法写入 Gradle init 脚本（应用数据目录）'; exit 1".into()),
                None => crate::maven::run_command(app, &prefs, &crate::maven::invocation(&root.join(rel)), &run),
            };
            let command = format!("{}{launch}", jdk_prefix_for(root, rel, &app.module, &run.jdk));
            SpringTask {
                def: Def { name, command, cwd: rel.clone(), env: Vec::new(), source: Source::Spring { module } },
                app: app.clone(),
            }
        })
        .collect();
    (out, problems)
}

/// 这个任务名是不是我们拼的 `java` 命令（认出来的 Spring Boot 应用，或者你建的 application 运行配置）；是的话它的主类
/// （调试要：问 jdtls 这个类在哪个项目）。tasks.json 里同名的以文件为准 —— 那就不是我们拼的命令，不算
pub fn java_main(root: &Path, name: &str) -> Option<String> {
    // resolve 也是这个次序：discover 里同名的以文件为准，那就不是我们拼的命令
    match discover(root).defs.into_iter().find(|d| d.name == name)?.source {
        Source::Spring { .. } => spring_tasks(root).0.into_iter().find(|t| t.def.name == name).map(|t| t.app.main_class),
        Source::User { kind, .. } if kind == "application" => crate::projectcfg::get(root).configs.into_iter().find(|u| u.name == name).map(|u| u.main_class),
        _ => None,
    }
}

fn spring_apps(root: &Path, found: &mut Found) {
    let (tasks, problems) = spring_tasks(root);
    found.problems.extend(problems);
    for t in tasks {
        // tasks.json 里同名的以文件为准
        if !found.defs.iter().any(|d| d.name == t.def.name) {
            found.defs.push(t.def);
        }
    }
}

/// Maven 工具窗点的目标：任务名是「artifactId [目标]」（IDEA 运行窗标签的样子）。不进任务列表（9 个阶段 × 每个模块会把列表淹掉），
/// 但 `task_run` 按名字认得它 —— 于是运行窗、⌃R 重跑、⌘F2 停、同名先停旧的全是现成的。目标里每个词都要过 `maven::goal_command` 的字符检查
pub fn maven_goal(root: &Path, name: &str) -> Option<Def> {
    let (artifact, goal) = name.strip_suffix(']')?.rsplit_once(" [")?;
    let Some((rel, m)) = find_module(root, artifact) else { return gradle_goal(root, artifact, goal, name) };
    let prefs = crate::maven::prefs(root);
    // 单测（docs/IDEA.md 第 17 节）：「模块 [test 选择器]」—— 和跑目标同一个名字形状，运行窗、⌃R 重跑、⌘F2 停、同名先停旧的都是现成的
    let mvn = crate::maven::invocation(&root.join(&rel));
    // 「模块 [cover 选择器]」：带覆盖率跑的单测（docs/TAKEOVER.md G15）—— 命令同 test，代理在 task_run 里挂（要知道 .app 里那个 jar 在哪）
    let command = match goal.strip_prefix("test ").or_else(|| goal.strip_prefix("cover ")) {
        Some(sel) => crate::maven::test_command(&m.rel, sel.trim(), &prefs, &mvn)?,
        None => crate::maven::goal_command(&m.rel, goal, &prefs, &mvn)?,
    };
    let command = format!("{}{command}", jdk_prefix(root, &rel, &m.rel));
    Some(Def { name: name.to_string(), command, cwd: rel, env: Vec::new(), source: Source::Goal { module: m.rel } })
}

/// Gradle 模块的「模块 [test 选择器]」（测试旁边的 ▶、⌃⇧R、重跑没过的都是这个名字形状）：`gradlew :模块:test --tests …`。
/// 带覆盖率（`[cover …]`）的命令同单测，代理在 task_run 里挂（`gradle::with_coverage`，同 Maven 那边）
fn gradle_goal(root: &Path, token: &str, goal: &str, name: &str) -> Option<Def> {
    let (rel, m) = find_gradle_module(root, token)?;
    let command = if let Some(sel) = goal.strip_prefix("test ").or_else(|| goal.strip_prefix("cover ")) {
        crate::gradle::test_command(&m.artifact_id, sel.trim(), &crate::gradle::invocation(&root.join(&rel)))?
    } else {
        // Gradle 工具窗里双击的任务（「:模块 [任务 …]」）：任务名只许字母数字和 `-` `_`（同 Maven 目标的字符检查，名字拼进命令行）
        let toks: Vec<&str> = goal.split_whitespace().collect();
        if toks.is_empty() || !toks.iter().all(|t| t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
            return None;
        }
        let tasks: Vec<String> = toks.iter().map(|t| if m.artifact_id == ":" { format!(":{t}") } else { format!("{}:{t}", m.artifact_id) }).collect();
        format!("{} {}", crate::gradle::invocation(&root.join(&rel)), tasks.join(" "))
    };
    Some(Def { name: name.to_string(), command, cwd: rel, env: Vec::new(), source: Source::Goal { module: m.rel } })
}

/// Gradle 模块的单测任务（「模块 [test …]」）调试时用的项目名：Buildship 拿 Gradle 的项目名当 Eclipse 的项目名 —— 子项目是路径的最后一段，
/// 根项目是 `rootProject.name`（没写就是目录名）。不是 Gradle 的单测给 None
pub fn gradle_test_project(root: &Path, name: &str) -> Option<String> {
    let (token, goal) = name.strip_suffix(']')?.rsplit_once(" [")?;
    goal.strip_prefix("test ")?;
    if find_module(root, token).is_some() {
        return None;
    }
    let (rel, m) = find_gradle_module(root, token)?;
    if m.artifact_id != ":" {
        return m.artifact_id.rsplit(':').next().map(str::to_string);
    }
    Some(crate::gradle::root_name(&root.join(&rel)))
}

/// 任务名里的模块那一段 → Gradle 的（构建根相对项目根, 模块）。认模块目录相对项目根的路径（测试旁边的 ▶ 起的）、Gradle 的项目路径（`:a:b`），根项目写 `.`
fn find_gradle_module(root: &Path, token: &str) -> Option<(String, crate::maven::Module)> {
    for rel in crate::gradle::roots(root) {
        let hit = crate::gradle::modules(&root.join(&rel)).into_iter().find(|m| {
            let dir = [rel.as_str(), m.rel.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("/");
            m.artifact_id == token || (!dir.is_empty() && dir == token) || (dir.is_empty() && token == ".")
        });
        if let Some(m) = hit {
            return Some((rel, m));
        }
    }
    None
}

/// 单测任务（「模块 [test 选择器]」）调试时用的项目名：模块的 artifactId（m2e 默认拿它当项目名，求值要）。不是单测任务给 None
pub fn test_project(root: &Path, name: &str) -> Option<String> {
    let (artifact, goal) = name.strip_suffix(']')?.rsplit_once(" [")?;
    goal.strip_prefix("test ").or_else(|| goal.strip_prefix("cover "))?;
    Some(find_module(root, artifact)?.1.artifact_id)
}

/// 任务名里的模块那一段 → （它所在的 reactor 相对项目根的路径, 模块）。认 artifactId（Maven 工具窗起的），也认模块目录相对项目根的路径
/// （测试文件旁边的 ▶ 起的：前端从文件路径就算得出目录，不用先问 Rust 这个模块叫什么）；根模块写 `.`
fn find_module(root: &Path, token: &str) -> Option<(String, crate::maven::Module)> {
    for rel in crate::maven::reactors(root) {
        let r = root.join(&rel);
        let hit = crate::maven::modules(&r).into_iter().find(|m| {
            let dir = [rel.as_str(), m.rel.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("/");
            m.artifact_id == token || (!dir.is_empty() && dir == token) || (dir.is_empty() && token == ".")
        });
        if let Some(m) = hit {
            return Some((rel, m));
        }
    }
    None
}

/// 跑单测那次（「模块 [test 选择器]」）的模块目录（绝对路径）：读它 `target/surefire-reports` 里的结果。不是测试任务给 None
pub fn test_module_dir(root: &Path, name: &str) -> Option<std::path::PathBuf> {
    let (token, goal) = name.strip_suffix(']')?.rsplit_once(" [")?;
    goal.strip_prefix("test ").or_else(|| goal.strip_prefix("cover "))?;
    let (rel, m) = find_module(root, token).or_else(|| find_gradle_module(root, token))?;
    Some(root.join(rel).join(m.rel))
}

/// 按名字找任务：先找列表里的（tasks.json、scripts、Spring Boot），再认 Maven 工具窗的「模块 [目标]」、构建项目
pub fn resolve(root: &Path, name: &str) -> Option<Def> {
    discover(root).defs.into_iter().find(|d| d.name == name).or_else(|| maven_goal(root, name)).or_else(|| build(root, name))
}

/// 我们拼的 Maven 命令前面加的那段：用项目要的那个 JDK（docs/IDEA.md 第 23 节）。`reactor` / `module` 相对项目根 / reactor。
///
/// **写进命令，不走环境变量**：任务跑在登录 shell 里（`-ilc`），你的 `.zshrc` 在我们设的环境变量之后执行，`export JAVA_HOME=…` 一句就盖回去了；
/// 写在命令最前面就是最后生效的。运行窗里那一行命令也看得出用的是哪个 JDK。
/// pom 没写版本：空的（照旧用你的默认，不猜）。写了但这台机器上没有够格的：说一句再照旧跑 —— 不让 Maven 甩一句「不支持发行版本 21」让人猜
pub fn jdk_prefix(root: &Path, reactor: &str, module: &str) -> String {
    jdk_prefix_for(root, reactor, module, "")
}

/// 同 [`jdk_prefix`]，多一个运行配置里选的 JDK（`chosen`，空 = 跟项目）。挑的次序（docs/SETTINGS.md 13.6）：
/// 运行配置里选的 > 项目设置里选的（`projectcfg`）> 按 pom 自动挑。选了的那个不在了（卸了、目录挪了）就说一句、退回自动挑 ——
/// 不让 `JAVA_HOME` 指向一个不存在的目录，那样 mvn 报的是一句看不出原因的「JAVA_HOME is not defined correctly」
pub fn jdk_prefix_for(root: &Path, reactor: &str, module: &str, chosen: &str) -> String {
    let project = crate::projectcfg::get(root).jdk;
    let pick = if chosen.trim().is_empty() { project.trim().to_string() } else { chosen.trim().to_string() };
    let auto = || match crate::maven::module_java(&root.join(reactor), module) {
        Some(want) => jdk_prefix_from(want, &crate::jdks::all()),
        None => String::new(),
    };
    if pick.is_empty() {
        return auto();
    }
    match crate::jdks::at(&pick) {
        Some((_, h)) => export_jdk(&h),
        None => format!("echo {}; {}", crate::maven::word(&format!("[lite-ide] 未找到设置中选择的 JDK {pick}（或不是有效的 JDK），将根据 pom 自动选择")), auto()),
    }
}

fn export_jdk(h: &Path) -> String {
    let h = h.to_string_lossy();
    format!("export JAVA_HOME={} PATH={}:\"$PATH\"; ", crate::maven::word(&h), crate::maven::word(&format!("{h}/bin")))
}

fn jdk_prefix_from(want: u32, jdks: &[(u32, PathBuf)]) -> String {
    match lspsvc::jdtls::pick_jdk(jdks, want) {
        Some((_, h)) => export_jdk(&h),
        None => format!("echo {}; ", crate::maven::word(&format!("[lite-ide] 项目要求 Java {want}，本机未找到满足要求的 JDK，使用默认 JDK"))),
    }
}

/// 构建（⌘F9，docs/IDEA.md 第 22 节）的两个任务名。不进任务列表（同 Maven 目标），`task_run` 按名字认；运行窗、⌃R 重跑、⌘F2 停、同名先停旧的都是现成的
pub const BUILD: &str = "构建项目";
pub const REBUILD: &str = "重新构建项目";

pub fn is_build(name: &str) -> bool {
    name == BUILD || name == REBUILD
}

fn build(root: &Path, name: &str) -> Option<Def> {
    if !is_build(name) {
        return None;
    }
    // 没有 Maven 的 reactor 就看 Gradle 的构建根（docs/TAKEOVER.md G24）
    let command = crate::maven::build_command(&crate::maven::reactors(root), &crate::maven::prefs(root), name == REBUILD, &|r| jdk_prefix(root, r, ""), &|r| crate::maven::invocation(&root.join(r)))
        .or_else(|| crate::gradle::build_command(&crate::gradle::roots(root), name == REBUILD, &|r| crate::gradle::invocation(&root.join(r))))?;
    Some(Def { name: name.to_string(), command, cwd: String::new(), env: Vec::new(), source: Source::Goal { module: String::new() } })
}

fn shown(rel: &str, file: &str) -> String {
    if rel.is_empty() {
        file.to_string()
    } else {
        format!("{rel}/{file}")
    }
}

/// 新建 `tasks.json`（带模板）。已经有了就不动它、返回路径 —— 「新建」点两下不能把人写好的冲掉
pub fn create_file(root: &Path) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let p = root.join(FILE);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // `create_new`：rust.md「std API 会吃掉已有文件」—— `File::create` 会把已有的截成 0 字节
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
        Ok(mut f) => f.write_all(template().as_bytes())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 远程调试的占位命令_主机名里的怪字符拼不进去() {
        assert_eq!(remote_command("10.0.0.5", 5005), "printf '正在调试 %s:%s 上的 JVM（⌘F2 断开，不会终止该 JVM）\\n' '10.0.0.5' '5005'; exec tail -f /dev/null");
        // 校验在配置那头（settingsctl::host_ok）；万一绕过了，这里也只留主机名的字符 —— 单引号、分号进不去
        assert_eq!(
            remote_command("a'; rm -rf ~; '", 1),
            "printf '正在调试 %s:%s 上的 JVM（⌘F2 断开，不会终止该 JVM）\\n' 'arm-rf' '1'; exec tail -f /dev/null",
            "引号、分号、空格、~ 都去掉了：拼进单引号里也闭合不了它"
        );
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lite-ide-taskdefs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn gradle_项目_服务单测构建都走_gradle_的命令_同时有_pom_的照旧按_maven() {
        let root = tmp("gradle");
        crate::gradle::set_init_file(std::env::temp_dir().join(format!("lite-ide-taskdefs-init-{}.gradle", std::process::id())));
        let app = root.join("order-service/src/main/java/com/demo/order");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(root.join("shop-common/src/test/java")).unwrap();
        std::fs::write(root.join("settings.gradle.kts"), "include(\"shop-common\", \"order-service\")\n").unwrap();
        std::fs::write(root.join("build.gradle.kts"), "").unwrap();
        std::fs::write(root.join("gradlew"), "#!/bin/sh\n").unwrap();
        std::fs::write(app.join("OrderApplication.java"), "package com.demo.order;\n@SpringBootApplication\npublic class OrderApplication { public static void main(String[] a) {} }\n").unwrap();

        let (tasks, _) = spring_tasks(&root);
        assert_eq!(tasks.len(), 1, "{:?}", tasks.iter().map(|t| &t.def).collect::<Vec<_>>());
        assert_eq!(tasks[0].def.name, "OrderApplication");
        assert!(tasks[0].def.command.contains(":order-service:liteIdeClasspath && cd order-service && exec java"), "{}", tasks[0].def.command);
        assert!(!tasks[0].def.command.contains("mvn"), "{}", tasks[0].def.command);

        let t = resolve(&root, "shop-common [test com.demo.MoneyTest#ok]").unwrap();
        assert!(t.command.starts_with("sh ./gradlew --console=plain "), "gradlew 没执行位：sh 跑它：{}", t.command);
        assert!(t.command.ends_with(" :shop-common:test --rerun --tests com.demo.MoneyTest.ok"), "{}", t.command);
        let cover = resolve(&root, "shop-common [cover com.demo.MoneyTest]").unwrap().command;
        assert!(cover.contains(":shop-common:test --rerun --tests com.demo.MoneyTest"), "带覆盖率的命令同单测（代理在 task_run 里挂）：{cover}");
        assert_eq!(gradle_test_project(&root, "shop-common [test com.demo.MoneyTest]").as_deref(), Some("shop-common"), "调试时的项目名：Gradle 的项目名");
        std::fs::write(root.join("settings.gradle.kts"), "rootProject.name = \"shopdemo\"\ninclude(\"shop-common\", \"order-service\")\n").unwrap();
        std::fs::create_dir_all(root.join("src/test/java")).unwrap();
        assert_eq!(gradle_test_project(&root, ". [test com.demo.RootTest]").as_deref(), Some("shopdemo"), "根项目：rootProject.name");
        assert_eq!(test_module_dir(&root, "shop-common [test X]").unwrap(), root.join("shop-common"));
        assert_eq!(resolve(&root, BUILD).unwrap().command, "sh ./gradlew --console=plain --continue classes testClasses");
        assert_eq!(resolve(&root, ":order-service [bootJar]").unwrap().command, "sh ./gradlew --console=plain :order-service:bootJar", "Gradle 工具窗双击的任务");
        assert!(resolve(&root, ":order-service [x;rm]").is_none(), "任务名里的怪字符拼不进命令行");

        // 迁移到一半、两个都有：按 Maven（不出两份一样的服务）
        std::fs::write(root.join("pom.xml"), "<project><modules><module>order-service</module></modules></project>").unwrap();
        assert!(crate::gradle::roots(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 模板解析出来是空的_一句问题都没有() {
        assert_eq!(parse_file(template()), (Vec::new(), Vec::new()));
    }

    #[test]
    fn 四个字段_注释和末尾逗号都认() {
        let (defs, problems) = parse_file(
            r#"[
              // 后端
              { "name": "后端", "command": "mvn spring-boot:run", "cwd": "./admin/", "env": { "A": "1" }, },
              { "name": "脚本", "command": "python3 main.py" },
            ]"#,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(defs[0], Def { name: "后端".into(), command: "mvn spring-boot:run".into(), cwd: "admin".into(), env: vec![("A".into(), "1".into())], source: Source::File });
        assert_eq!(defs[1].cwd, "");
    }

    #[test]
    fn 写坏的那一个跳过并说出来_别的照常() {
        let (defs, problems) = parse_file(
            r#"[
              { "name": "好的", "command": "echo ok" },
              { "name": "缺命令" },
              { "name": "出去了", "command": "ls", "cwd": "../别人的" },
              { "name": "绝对路径", "command": "ls", "cwd": "/etc" },
              { "name": "拼错", "cmd": "x", "command": "echo y" },
              { "name": "好的", "command": "echo dup" },
            ]"#,
        );
        assert_eq!(defs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["好的", "拼错"]);
        assert_eq!(defs[0].command, "echo ok", "重名用前面那个");
        let all = problems.join("\n");
        for want in ["第 2 个任务缺少 name 或 command", "「出去了」：cwd", "「绝对路径」：cwd", "未知的键 cmd", "「好的」 名称重复"] {
            assert!(all.contains(want), "少了「{want}」：\n{all}");
        }
    }

    #[test]
    fn 语法错说出第几行() {
        let (defs, problems) = parse_file("[\n  { \"name\": \"a\", \"command\": \"b\" }\n  { \"name\": \"c\" }\n]");
        assert!(defs.is_empty());
        assert!(problems[0].contains("第 3 行"), "{problems:?}");
        let (_, problems) = parse_file("{ \"name\": \"a\" }");
        assert!(problems[0].contains("顶层必须是数组 ["), "{problems:?}");
    }

    /// 这条测试存在的理由：serde_json 的 Map 按键名排序，`dev` 会跑到 `build` 后面
    #[test]
    fn scripts_按文件里的顺序_生命周期钩子不列() {
        let (s, pm) = package_scripts(
            r#"{ "scripts": { "dev": "vite", "build": "vite build", "prebuild": "rm -rf dist", "postinstall": "x",
                 "test:unit": "node t", "preview": "vite preview" }, "packageManager": "pnpm@9.1.0" }"#,
        )
        .unwrap();
        assert_eq!(s, ["dev", "build", "test:unit", "preview"], "preview 不是 view 的钩子（没有 view 这个 script），要留着");
        assert_eq!(pm.as_deref(), Some("pnpm"));
    }

    #[test]
    fn 语言包的任务_根上有标记只列根上的_没有才看子目录_同名让给_tasks_json() {
        let root = tmp("pack-tasks");
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"core\"]\n").unwrap();
        std::fs::create_dir_all(root.join("core")).unwrap();
        std::fs::write(root.join("core/Cargo.toml"), "[package]\nname = \"core\"\n").unwrap();
        std::fs::create_dir_all(root.join(".lite-ide")).unwrap();
        std::fs::write(root.join(FILE), r#"[{ "name": "cargo test", "command": "cargo test --workspace -q" }]"#).unwrap();
        let f = discover(&root);
        let cargo: Vec<&Def> = f.defs.iter().filter(|d| matches!(&d.source, Source::Pack { group, .. } if group == "Cargo")).collect();
        assert_eq!(cargo.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["cargo build", "cargo run", "cargo check", "cargo clippy"], "工作区只列根上一份、成员不重复列；cargo test 让给 tasks.json");
        assert!(cargo.iter().all(|d| d.cwd.is_empty()));
        assert_eq!(f.defs.iter().find(|d| d.name == "cargo test").unwrap().command, "cargo test --workspace -q", "同名的以 tasks.json 为准");

        let two = tmp("pack-tasks-sub");
        for d in ["api", "worker"] {
            std::fs::create_dir_all(two.join(d)).unwrap();
            std::fs::write(two.join(d).join("Cargo.toml"), "[package]\n").unwrap();
        }
        std::fs::create_dir_all(two.join("web")).unwrap();
        let f = discover(&two);
        let run: Vec<(&str, &str)> = f.defs.iter().filter(|d| d.name.ends_with("cargo run")).map(|d| (d.name.as_str(), d.cwd.as_str())).collect();
        assert_eq!(run, [("api/cargo run", "api"), ("worker/cargo run", "worker")], "根上没有：第一层子目录里有的各一份，在那个目录里跑");
        assert!(discover(&tmp("pack-tasks-none")).defs.is_empty(), "没有 Cargo.toml 一条都不列");
    }

    #[test]
    fn 自动认出_包管理器按声明再按锁文件_同名以文件为准() {
        let root = tmp("discover");
        std::fs::write(root.join("package.json"), r#"{ "scripts": { "dev": "vite", "lint": "eslint ." } }"#).unwrap();
        std::fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::create_dir_all(root.join("api")).unwrap();
        std::fs::write(root.join("api/package.json"), r#"{ "scripts": { "start": "node s" }, "packageManager": "yarn@4" }"#).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::write(root.join("node_modules/package.json"), r#"{ "scripts": { "no": "x" } }"#).unwrap();
        std::fs::create_dir_all(root.join(".lite-ide")).unwrap();
        std::fs::write(root.join(FILE), r#"[{ "name": "lint", "command": "pnpm lint --fix" }]"#).unwrap();

        let f = discover(&root);
        assert!(f.file && f.problems.is_empty(), "{:?}", f.problems);
        let got: Vec<(&str, &str, &str)> = f.defs.iter().map(|d| (d.name.as_str(), d.command.as_str(), d.cwd.as_str())).collect();
        assert_eq!(
            got,
            [("lint", "pnpm lint --fix", ""), ("dev", "pnpm run dev", ""), ("api/start", "yarn run start", "api")],
            "文件里的在前；同名的 lint 以文件为准；子目录声明了 yarn 就用 yarn；node_modules 不进"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 没有锁文件就是_npm_名字怪的套引号() {
        let root = tmp("npm");
        std::fs::write(root.join("package.json"), r#"{ "scripts": { "build:prod": "x", "it's": "y" } }"#).unwrap();
        let f = discover(&root);
        assert_eq!(f.defs[0].command, "npm run build:prod");
        assert_eq!(f.defs[1].command, r"npm run 'it'\''s'");
        assert!(!f.file);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Spring Boot 应用自动列出来：撞名的带模块路径、tasks.json 里同名的以文件为准、项目根没有 pom 时找一层子目录
    #[test]
    fn spring_boot_应用自动列出_撞名带模块_同名以文件为准() {
        let d = tmp("spring");
        let w = |p: &str, t: &str| {
            let p = d.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
            std::fs::write(p, t).unwrap();
        };
        w("backend/pom.xml", "<project><artifactId>shop</artifactId><modules><module>order</module><module>user</module><module>admin</module></modules></project>");
        for m in ["order", "user", "admin"] {
            w(&format!("backend/{m}/pom.xml"), &format!("<project><artifactId>{m}</artifactId></project>"));
        }
        let app = |pkg: &str, cls: &str| format!("package {pkg};\n@SpringBootApplication\npublic class {cls} {{ public static void main(String[] a) {{}} }}");
        w("backend/order/src/main/java/o/OrderApplication.java", &app("o", "OrderApplication"));
        w("backend/user/src/main/java/u/Application.java", &app("u", "Application"));
        w("backend/admin/src/main/java/a/Application.java", &app("a", "Application"));
        w(".lite-ide/tasks.json", r#"[{ "name": "OrderApplication", "command": "echo 带 profile 的那个", "cwd": "backend" }]"#);
        let f = discover(&d);
        let got: Vec<(&str, &Source)> = f.defs.iter().map(|x| (x.name.as_str(), &x.source)).collect();
        assert_eq!(
            got,
            [
                ("OrderApplication", &Source::File),
                ("backend/user/Application", &Source::Spring { module: "backend/user".into() }),
                ("backend/admin/Application", &Source::Spring { module: "backend/admin".into() }),
            ],
            "{:?}",
            f.problems
        );
        let user = f.defs.iter().find(|x| x.name == "backend/user/Application").unwrap();
        assert_eq!(user.cwd, "backend", "在 reactor 根上跑");
        assert!(user.command.contains("-pl user -am") && user.command.ends_with(" u.Application"), "{}", user.command);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 你建的运行配置（docs/TAKEOVER.md R1）：四种各拼成什么样；application 和认出来的 Spring 服务同一种命令（调试插得进 JDWP），
    /// 在 reactor 根上跑；目标里有 shell 特殊字符的不列、说一句；和已有的同名的不列、说一句
    #[test]
    fn 你建的运行配置_四种各拼成什么样() {
        use crate::projectcfg::{RunCfg, UserCfg};
        let d = tmp("user");
        let w = |p: &str, t: &str| {
            let p = d.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
            std::fs::write(p, t).unwrap();
        };
        w("backend/pom.xml", "<project><artifactId>shop</artifactId><modules><module>order</module></modules></project>");
        w("backend/order/pom.xml", "<project><artifactId>order</artifactId></project>");
        let u = |name: &str, kind: &str| UserCfg { name: name.into(), kind: kind.into(), ..Default::default() };
        let configs = vec![
            UserCfg { main_class: "o.Tool".into(), module: "backend/order".into(), run: RunCfg { vm_options: "-Xmx1g".into(), program_args: "--x".into(), ..Default::default() }, ..u("Tool", "application") },
            UserCfg { module: "backend/order".into(), goals: "clean package".into(), ..u("打包", "maven") },
            UserCfg { command: "pnpm dev".into(), cwd: "web".into(), run: RunCfg { env: vec![("A".into(), "1".into())], ..Default::default() }, ..u("前端", "shell") },
            UserCfg { items: vec!["Tool".into(), "前端".into()], ..u("全部", "compound") },
            UserCfg { module: "backend/order".into(), goals: "clean; rm -rf ~".into(), ..u("坏目标", "maven") },
            UserCfg { command: "echo 撞了".into(), ..u("已有的", "shell") },
        ];
        let mut f = Found { defs: vec![Def { name: "已有的".into(), command: "x".into(), cwd: String::new(), env: Vec::new(), source: Source::File }], ..Default::default() };
        user_defs(&d, configs, &mut f);
        let by = |n: &str| f.defs.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("没有 {n}：{:?}", f.problems));
        let tool = by("Tool");
        assert_eq!(tool.cwd, "backend", "在 reactor 根上跑");
        assert!(tool.command.contains("-pl order -am") && tool.command.contains("exec java -Xmx1g -cp") && tool.command.ends_with(" o.Tool --x"), "{}", tool.command);
        assert!(crate::debug::with_jdwp(&tool.command, None).is_some(), "调试插得进 JDWP（和 Spring 服务同一个形状）");
        let pack = by("打包");
        assert!(pack.cwd == "backend" && pack.command.contains("-pl order") && pack.command.contains("clean package"), "{} @ {}", pack.command, pack.cwd);
        let front = by("前端");
        assert_eq!((front.command.as_str(), front.cwd.as_str(), front.env.clone()), ("pnpm dev", "web", vec![("A".to_string(), "1".to_string())]));
        assert_eq!(by("全部").source, Source::User { kind: "compound".into(), items: vec!["Tool".into(), "前端".into()] });
        assert!(!f.defs.iter().any(|x| x.name == "坏目标") && f.problems.iter().any(|p| p.contains("坏目标")), "{:?}", f.problems);
        assert_eq!(f.defs.iter().filter(|x| x.name == "已有的").count(), 1, "同名的不列第二份");
        assert!(f.problems.iter().any(|p| p.contains("「已有的」与其他配置重名")), "{:?}", f.problems);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Maven 工具窗的「模块 [目标]」：task_run 认得、任务列表里不列；不认识的模块、带 shell 特殊字符的目标都不给
    #[test]
    fn 用项目要的_jdk_写进命令最前面_没有够格的说一句() {
        let jdks = vec![(17, PathBuf::from("/L/temurin-17.jdk/Contents/Home")), (21, PathBuf::from("/L/my jdk 21/Contents/Home"))];
        assert_eq!(jdk_prefix_from(17, &jdks), "export JAVA_HOME=/L/temurin-17.jdk/Contents/Home PATH=/L/temurin-17.jdk/Contents/Home/bin:\"$PATH\"; ");
        assert_eq!(jdk_prefix_from(21, &jdks), "export JAVA_HOME='/L/my jdk 21/Contents/Home' PATH='/L/my jdk 21/Contents/Home/bin':\"$PATH\"; ", "带空格的路径要引号");
        assert!(jdk_prefix_from(25, &jdks).starts_with("echo '[lite-ide] 项目要求 Java 25"), "没有够格的：说一句");
        // pom 没写版本：不加任何东西（taskdefs 的其余测试的 pom 都没写，命令照旧）
        let d = tmp("jdkprefix");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        std::fs::write(d.join("pom.xml"), "<project><artifactId>a</artifactId></project>").unwrap();
        assert_eq!(jdk_prefix(&d, "", ""), "");
    }

    #[test]
    fn maven_目标按名字认得_不进列表() {
        let d = tmp("goal");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
        let w = |p: &str, t: &str| {
            let p = d.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        w("pom.xml", "<project><artifactId>shop</artifactId><modules><module>order</module></modules></project>");
        w("order/pom.xml", "<project><artifactId>order-service</artifactId></project>");
        let def = resolve(&d, "order-service [package]").expect("认得");
        assert_eq!((def.command.as_str(), def.cwd.as_str()), ("mvn -B -ntp -pl order -am package", ""));
        assert_eq!(resolve(&d, "shop [clean]").unwrap().command, "mvn -B -ntp clean", "根模块的目标对整个 reactor");
        assert!(resolve(&d, "order-service [deploy]").is_some(), "deploy 是 IDEA 那 9 个阶段之一");
        assert!(resolve(&d, "order-service [package; rm -rf ~]").is_none(), "拼得进第二条命令的不给");
        assert!(resolve(&d, "nobody [package]").is_none(), "没有这个模块");
        assert!(discover(&d).defs.is_empty(), "不进任务列表");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 单测（docs/IDEA.md 第 17 节）：「模块 [test 选择器]」—— 模块那一段认 artifactId、认目录、根模块认 `.`；找得回模块目录读报告
    #[test]
    fn 单测按名字认得_模块认_artifactid_目录和根() {
        let d = tmp("tests");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
        let w = |p: &str, t: &str| {
            let p = d.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        w("pom.xml", "<project><artifactId>shop</artifactId><modules><module>order</module></modules></project>");
        w("order/pom.xml", "<project><artifactId>order-service</artifactId></project>");
        let by_dir = resolve(&d, "order [test com.demo.OrderTest#adds]").expect("按目录认得");
        assert!(by_dir.command.contains("-Dtest='com.demo.OrderTest#adds'") && by_dir.command.contains("-pl order -am test"), "{}", by_dir.command);
        let by_id = resolve(&d, "order-service [test com.demo.OrderTest]").unwrap().command;
        assert!(by_id.contains("-Dtest=com.demo.OrderTest ") && by_id.contains("-pl order -am test"), "按 artifactId 也认得：{by_id}");
        assert!(resolve(&d, ". [test com.demo.RootTest]").unwrap().command.ends_with(" test") && !resolve(&d, ". [test com.demo.RootTest]").unwrap().command.contains("-pl"), "根模块写 .");
        assert!(resolve(&d, "order [test A; rm -rf ~]").is_none(), "选择器拼得进第二条命令的不给");
        assert_eq!(test_module_dir(&d, "order [test com.demo.OrderTest]"), Some(d.join("order")));
        assert_eq!(test_module_dir(&d, "order [package]"), None, "不是测试任务");
        // 调试单测时的项目名：按目录起的也给 artifactId（m2e 的项目名是它，不是目录名）
        assert_eq!(test_project(&d, "order [test com.demo.OrderTest#adds]").as_deref(), Some("order-service"));
        assert_eq!(test_project(&d, "order [package]"), None, "不是测试任务");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 新建不冲掉已有的() {
        let root = tmp("create");
        let p = create_file(&root).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), template());
        std::fs::write(&p, "[]").unwrap();
        create_file(&root).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[]", "第二次新建把写好的冲掉了");
        let _ = std::fs::remove_dir_all(&root);
    }
}
