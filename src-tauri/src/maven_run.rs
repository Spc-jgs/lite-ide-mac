//! Maven 工具窗的 Plugins / Dependencies 两个节点要的数据（IDEA 用它内置的 Maven 算，我们跑一次用户自己的 mvn 算）。
//! 解析在 `maven.rs`（纯函数、有测试），这里只管「跑、读文件、读插件 jar」。
//!
//! **一次 mvn 算两样**：`help:effective-pom`（每个模块真用的插件）+ `dependency:tree`（每个模块的依赖树），同一个 JVM 里跑完 ——
//! JVM 起来、读 pom、解析 reactor 是大头，分两次跑就付两次。`-q` 只留报错；两样都写文件不走 stdout（有效 POM 一个模块几十 KB）。
//! 依赖树写进各模块自己的 `target/`（构建产物的目录，同 `maven::CP_FILE`）。
//!
//! **经登录 shell 跑**（同任务，docs/TASKS.md 第 10 节）：从 Finder 起的应用 PATH 里没有 mvn。

use crate::maven::{self, Dep, Prefs};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

/// 依赖树写在各模块的 target/ 里
pub const DEPS_FILE: &str = "target/.lite-ide-deps.txt";
/// mvn 的 stdout（`-q` 下只有报错）最多留多少
const OUT_CAP: usize = 256 << 10;
/// 插件描述最多读多少：大插件（spring-boot、surefire）几百 KB
const PLUGIN_XML_CAP: usize = 8 << 20;

/// 一个插件：坐标 + 前缀 + 目标（读插件 jar 里的描述；本地仓库里没有 jar 时目标是空的）
#[derive(Debug, Clone)]
pub struct Plugin {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub prefix: String,
    pub goals: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub artifact_id: String,
    pub plugins: Vec<Plugin>,
    pub deps: Vec<Dep>,
}

/// 同一个 reactor 同时只算一份：连点两下刷新，两条 mvn 往同一批文件里写
static BUSY: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 插件描述读一次就记住（坐标带版本，同一版本的插件内容不会变）
static PLUGIN_CACHE: Mutex<Option<HashMap<String, (String, Vec<String>)>>> = Mutex::new(None);

/// 跑一次 mvn，算出这个 reactor 里每个模块的插件和依赖。`shell` 是任务用的那个（设置里的 `terminal.shell`，空 = `$SHELL`）；
/// `repo` 是 Maven 本地仓库（找插件 jar）；`scratch` 放有效 POM 的临时文件（应用数据目录里）。会阻塞到 mvn 跑完
pub fn resolve(reactor: &Path, prefs: &Prefs, shell: &str, repo: &Path, scratch: &Path) -> Result<Vec<Resolved>, String> {
    let key = reactor.to_string_lossy().into_owned();
    {
        let mut b = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        if b.contains(&key) {
            return Err("正在解析，等这一次跑完".into());
        }
        b.push(key.clone());
    }
    let r = run(reactor, prefs, shell, repo, scratch);
    BUSY.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| *k != key);
    r
}

fn run(reactor: &Path, prefs: &Prefs, shell: &str, repo: &Path, scratch: &Path) -> Result<Vec<Resolved>, String> {
    std::fs::create_dir_all(scratch.parent().unwrap_or(scratch)).map_err(|e| format!("建不了临时目录：{e}"))?;
    let cmd = maven::resolve_command(prefs, scratch, DEPS_FILE, &maven::invocation(reactor));
    let shell = if shell.is_empty() { std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()) } else { shell.to_string() };
    let mut c = Command::new(&shell);
    c.args(["-ilc", &cmd]).current_dir(reactor);
    let out = procutil::run_capped(&mut c, OUT_CAP, procutil::ERR_CAP, procutil::Overflow::Drain).map_err(|e| format!("起不了 mvn：{e}"))?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        // -q 下 stdout 只剩报错；挑 [ERROR] 那几行，没有就整段（多半是 shell 找不到 mvn）
        let errs: Vec<&str> = text.lines().filter(|l| l.contains("[ERROR]")).take(8).collect();
        let why = if errs.is_empty() { format!("{}{}", text.trim(), String::from_utf8_lossy(&out.stderr).trim()) } else { errs.join("\n") };
        return Err(if why.is_empty() { format!("mvn 失败了（{}）", out.status) } else { why });
    }
    let eff = std::fs::read_to_string(scratch).map_err(|e| format!("读不到有效 POM：{e}"))?;
    let projects = maven::parse_effective(&eff);
    let mods = maven::modules(reactor);
    let mut res = Vec::new();
    for m in &mods {
        let plugins = projects
            .iter()
            .find(|(a, _)| *a == m.artifact_id)
            .map(|(_, ps)| ps.iter().map(|p| describe(repo, p)).collect())
            .unwrap_or_default();
        let deps = std::fs::read_to_string(reactor.join(&m.rel).join(DEPS_FILE)).map(|t| maven::parse_dep_tree(&t)).unwrap_or_default();
        res.push(Resolved { artifact_id: m.artifact_id.clone(), plugins, deps });
    }
    Ok(res)
}

/// 插件的前缀和目标：本地仓库里那个插件 jar 的 `META-INF/maven/plugin.xml`（IDEA 也是这么列的）。用系统的 unzip 读一个文件，不加 zip 的依赖
fn describe(repo: &Path, p: &maven::PluginRef) -> Plugin {
    let key = format!("{}:{}:{}", p.group_id, p.artifact_id, p.version);
    let cached = PLUGIN_CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&key).cloned());
    let (prefix, goals) = cached.unwrap_or_else(|| {
        let jar = repo.join(p.group_id.replace('.', "/")).join(&p.artifact_id).join(&p.version).join(format!("{}-{}.jar", p.artifact_id, p.version));
        let got = if jar.is_file() {
            let mut c = Command::new("/usr/bin/unzip");
            c.arg("-p").arg(&jar).arg("META-INF/maven/plugin.xml");
            procutil::run_capped(&mut c, PLUGIN_XML_CAP, procutil::ERR_CAP, procutil::Overflow::Kill)
                .ok()
                .filter(|o| o.status.success() && !o.truncated)
                .map(|o| maven::parse_plugin_xml(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or_default()
        } else {
            Default::default()
        };
        PLUGIN_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key, got.clone());
        got
    });
    // 没有前缀的（jar 不在本地仓库）按 Maven 的约定猜：xxx-maven-plugin → xxx、maven-xxx-plugin → xxx
    let prefix = if prefix.is_empty() { maven::default_prefix(&p.artifact_id) } else { prefix };
    Plugin { group_id: p.group_id.clone(), artifact_id: p.artifact_id.clone(), version: p.version.clone(), prefix, goals }
}
