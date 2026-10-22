//! 按项目的设置（docs/SETTINGS.md 13.4）：项目用哪个 JDK、每个 Spring 服务的运行配置。
//!
//! **默认只有你自己的**，存在应用数据目录的 `project-settings.json`（按项目根），不往项目里写 —— 同 IDEA 运行配置默认存在
//! `.idea/workspace.xml`：VM 参数里常有本机路径、数据库密码，不该一不小心就提交上去。IDEA 里配过的，运行配置对话框里「从 IDEA 导入」（`ideacfg`）；
//! 和同事共享（IDEA 的 `.run/*.run.xml`）暂不做 —— 真文件里一份都没有，理由在 13.8。
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
    /// 你自己建的运行配置（docs/TAKEOVER.md R1）：认出来的（Spring、package.json）和 tasks.json 之外的第四种来源。按建的次序
    pub configs: Vec<UserCfg>,
}

/// 你自己建的一份运行配置（docs/TAKEOVER.md R1，照 IDEA 的 Run/Debug Configurations 里那几种常用的）。
/// 一个结构装四种：用不上的字段空着（serde 默认值，老文件照读）。命令都由 `taskdefs` 用现成的拼法拼 ——
/// `application` 和认出来的 Spring 服务走同一个 `maven::run_command`，调试、热替换、选 JDK 那几处照常
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserCfg {
    pub name: String,
    /// `application`（一个 main 方法）/ `maven`（几个目标）/ `shell`（一行命令）/ `compound`（几份一起起）/ `remote`（挂到别处起的 JVM 上调试）
    pub kind: String,
    /// application：主类全名（`com.demo.Tool`）
    pub main_class: String,
    /// application / maven：模块目录，相对项目根（"" = 根上那个 pom）
    pub module: String,
    /// maven：目标，空格隔开（`clean package`）
    pub goals: String,
    /// shell：一整行命令，交给登录 shell（同 tasks.json）
    pub command: String,
    /// shell：工作目录，相对项目根
    pub cwd: String,
    /// compound：一起起的那几份的名字（认出来的、tasks.json 里的、你建的都行）
    pub items: Vec<String>,
    /// VM 参数、程序参数、环境变量、JDK、profile：application 全用；maven、shell 用环境变量（maven 还用 JDK）
    pub run: RunCfg,
    /// remote：要挂上去的那个 JVM 在哪（它起的时候带着 `-agentlib:jdwp=transport=dt_socket,server=y,…`；IDEA 的 Remote JVM Debug）
    pub host: String,
    pub port: u32,
}

pub const KINDS: [&str; 5] = ["application", "maven", "shell", "compound", "remote"];

/// 一个主类全名像不像（`a.b.C`、`$` 和下划线也算）：进命令之前挡掉明显写错的，别让 java 报一句「找不到或无法加载主类」让人猜
pub fn main_class_ok(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|p| {
            let mut c = p.chars();
            c.next().is_some_and(|f| f.is_alphabetic() || f == '_' || f == '$') && c.all(|x| x.is_alphanumeric() || x == '_' || x == '$')
        })
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
