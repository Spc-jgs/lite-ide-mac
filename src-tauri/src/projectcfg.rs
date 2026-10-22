//! 按项目的设置（docs/SETTINGS.md 13.4）：项目用哪个 JDK、每个 Spring 服务的运行配置。
//!
//! **默认只有你自己的**，存在应用数据目录的 `project-settings.json`（按项目根），不往项目里写 —— 同 IDEA 运行配置默认存在
//! `.idea/workspace.xml`：VM 参数里常有本机路径、数据库密码，不该一不小心就提交上去。要和同事共享的，第二步做成 IDEA 的 `.run/*.run.xml`。
//!
//! 读写的样子照 `maven-prefs.json`（`maven::prefs`）：几十字节到几 KB 的文件，每次现读，不缓存 —— 和「两个窗口各改一处」比，不值得

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 一个项目的设置。全是空的就是「什么都没改」
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectCfg {
    /// 项目用的 JDK（JAVA_HOME 目录）。空 = 按 pom 写的版本自动挑（`taskdefs::jdk_prefix`）
    pub jdk: String,
    /// 运行配置：任务名（Spring 服务的主类简单名）→ 配置
    pub runs: BTreeMap<String, RunCfg>,
}

/// 一个 Spring 服务的运行配置（IDEA 的 Spring Boot 运行配置里常用的那几项）
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RunCfg {
    /// Spring profile，逗号或空格隔开（`dev, local`）→ `-Dspring.profiles.active=dev,local`
    pub profiles: String,
    /// JVM 参数，照 shell 的写法原样放进命令（`-Xmx512m -Dfoo="a b"`）
    pub vm_options: String,
    /// 程序参数，同上，放在主类后面
    pub program_args: String,
    /// 环境变量，按次序
    pub env: Vec<(String, String)>,
    /// 这个服务用的 JDK。空 = 跟项目
    pub jdk: String,
}

impl RunCfg {
    pub fn is_empty(&self) -> bool {
        *self == RunCfg::default()
    }
}

static FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// 启动时设一次（`lib.rs` 的 setup）。没设（单元测试）就当什么都没存
pub fn set_file(p: PathBuf) {
    let _ = FILE.set(p);
}

fn load_all() -> BTreeMap<String, ProjectCfg> {
    FILE.get().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// 这个项目（项目根）的设置
pub fn get(root: &Path) -> ProjectCfg {
    load_all().remove(&root.to_string_lossy().into_owned()).unwrap_or_default()
}

/// 这个服务的运行配置
pub fn run(root: &Path, name: &str) -> RunCfg {
    get(root).runs.remove(name).unwrap_or_default()
}

/// 记下这个项目的设置。空的运行配置不留，整个项目都是空的就删掉这一条
#[allow(clippy::disallowed_methods)] // 应用自己的 project-settings.json：写临时文件再改名盖过去（同 maven-prefs.json）
pub fn set(root: &Path, mut cfg: ProjectCfg) -> std::io::Result<()> {
    let Some(file) = FILE.get() else { return Ok(()) };
    cfg.runs.retain(|_, r| !r.is_empty());
    let mut all = load_all();
    let key = root.to_string_lossy().into_owned();
    if cfg == ProjectCfg::default() {
        all.remove(&key);
    } else {
        all.insert(key, cfg);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&all).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, file)
}

/// 环境变量的名字能不能放进 `export`：字母或下划线开头，后面字母数字下划线。别的（`A-B`、`1X`、带空格的）进 shell 会变成别的意思
pub fn env_name_ok(k: &str) -> bool {
    let mut c = k.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// Spring profile 那一栏拆成名字（逗号、空格都算分隔）
pub fn profile_names(s: &str) -> Vec<&str> {
    s.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()).collect()
}
