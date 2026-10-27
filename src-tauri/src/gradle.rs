//! Gradle 项目（docs/TAKEOVER.md G24，MVP-7）：认出来、起里面的 Spring Boot 应用、跑单测、⌘F9 构建。和 `maven.rs` 同一个形状，
//! 任务、Services、运行窗、单测结果树、构建的错误树都是现成的那一套，这里只管「Gradle 的命令长什么样」。
//!
//! 起 Spring Boot 应用**不用 `bootRun`**，和 Maven 不用 `spring-boot:run` 同一个理由：
//!
//! ```text
//! ./gradlew -q --init-script <应用数据目录>/lite-ide-init.gradle :order-service:liteIdeClasspath
//!   && cd order-service && exec java -cp "$(cat build/.lite-ide-cp.txt)" com.demo.order.OrderApplication
//! ```
//!
//! `exec java` 这个形状不变，调试插 JDWP（`debug::with_jdwp`）、热替换挂代理认的都是它。classpath 由一段 init 脚本算（不写进项目）：
//! 给每个 Java 模块加一个任务，依赖 `runtimeClasspath`（于是兄弟模块的 jar、本模块的 classes 先编好），把它写进模块的 `build/`。
//!
//! 第 0 步实测（2026-10-26，Gradle 9.8.1、Spring Boot 4.0.1、两个模块）：
//! - 应用的 JVM 是 **Gradle daemon 起的**（父进程是 daemon、进程组也是 daemon 的），不在我们任务的进程组里。
//!   给 `gradlew` 那一组发 SIGINT / SIGTERM，Gradle 自己取消构建、把应用停掉（0.42 秒）；SIGKILL 掉 `gradlew`，daemon 发现客户端没了也停（0.03 秒）。
//!   我们的「整组先软后硬」靠的是 Gradle 的取消，**不是**整组杀到了它 —— 用 `exec java` 起就没有这个问题（应用是我们的子进程）
//! - 单测报告是 JUnit XML，写在 `build/test-results/test/`，surefire 那个解析器原样认（`junit.rs`）
//! - javac 的错误是 `路径:行: 错误: 消息`（没有列，同一段在失败摘要里多缩进两格再出现一遍），`javac.rs` 认
//! - 冷启动（daemon 还没起）跑一个模块的单测 39 秒，热的 1 秒；`--version` 第一次要下发行包（165 MB，26 秒）

use crate::maven::{word, Module};
use std::path::{Path, PathBuf};

/// init 脚本算出来的 classpath 写在模块的 build/ 里（构建产物的目录，同 Maven 的 `target/`）
pub const CP_FILE: &str = "build/.lite-ide-cp.txt";
/// 单测报告（Gradle 的 `test` 任务默认写这儿）
pub const REPORTS: &str = "build/test-results/test";
/// init 脚本里那个任务的名字
const CP_TASK: &str = "liteIdeClasspath";

const SETTINGS: [&str; 2] = ["settings.gradle.kts", "settings.gradle"];
const BUILD: [&str; 2] = ["build.gradle.kts", "build.gradle"];

/// 给每个用了 java 插件的模块加一个算 classpath 的任务（`plugins.withId`：没用 java 插件的模块不加，不报错）。
/// 不碰 `project`（Gradle 9 的配置缓存不许在执行阶段碰它），要的东西在配置阶段拿成 provider / 文件集
const INIT_SCRIPT: &str = r#"// lite-ide 加的（docs/TAKEOVER.md G24）：算一个模块的运行 classpath，写进它的 build/.lite-ide-cp.txt；
// 调试单测时把调试参数交给测试的 JVM（-PliteIdeJdwp=…）。不写进你的项目
allprojects {
    def jdwp = providers.gradleProperty('liteIdeJdwp')
    tasks.withType(Test).configureEach {
        if (jdwp.isPresent()) {
            jvmArgs(jdwp.get())
        }
    }
    plugins.withId('java') {
        def cp = sourceSets.main.runtimeClasspath
        def out = layout.buildDirectory.file('.lite-ide-cp.txt')
        tasks.register('liteIdeClasspath') {
            dependsOn cp
            outputs.upToDateWhen { false }
            doLast {
                def f = out.get().asFile
                f.parentFile.mkdirs()
                f.text = cp.asPath
            }
        }
    }
}
"#;

static INIT_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// 启动时设：init 脚本放在应用数据目录里
pub fn set_init_file(p: PathBuf) {
    let _ = INIT_FILE.set(p);
}

/// init 脚本的路径；盘上没有（或者是旧的）就写一份。写不了（没设过路径：单元测试）给 None
#[allow(clippy::disallowed_methods)] // 应用自己的 init 脚本：写临时文件再改名盖过去（同 maven-prefs.json）
fn init_script() -> Option<PathBuf> {
    let f = INIT_FILE.get()?;
    if std::fs::read_to_string(f).ok().as_deref() != Some(INIT_SCRIPT) {
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).ok()?;
        }
        // 临时文件名每次一个：两个窗口同时起 Gradle 任务（测试里是两条并行的测试）会同时走到这里，
        // 共用一个固定的临时名时，后改名的那个发现临时文件已经被前一个挪走、整个返回 None —— 命令拼不出来（2026-10-27 并行测试撞出来的）
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = f.with_extension(format!("gradle.{}-{n}.tmp", std::process::id()));
        std::fs::write(&tmp, INIT_SCRIPT).ok()?;
        if std::fs::rename(&tmp, f).is_err() {
            let _ = std::fs::remove_file(&tmp);
            // 改名失败但盘上已经是对的（别的调用写好了）：照样能用
            if std::fs::read_to_string(f).ok().as_deref() != Some(INIT_SCRIPT) {
                return None;
            }
        }
    }
    Some(f.clone())
}

/// 这个目录是不是一个 Gradle 构建的根：有 settings 文件，或者有 build 文件（单模块项目可以没有 settings）
pub fn is_root(dir: &Path) -> bool {
    SETTINGS.iter().chain(BUILD.iter()).any(|f| dir.join(f).is_file())
}

/// 项目里的 Gradle 构建根（相对项目根）：项目根就是就用它；不是就找一层子目录（同 `maven::reactors`）。
/// **有 pom.xml 的目录不算**：两个都有的项目（迁移到一半）照旧按 Maven 走，不出两份一样的服务
pub fn roots(root: &Path) -> Vec<String> {
    let ok = |d: &Path| is_root(d) && !d.join("pom.xml").is_file();
    if ok(root) {
        return vec![String::new()];
    }
    if root.join("pom.xml").is_file() {
        return Vec::new();
    }
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut subs: Vec<String> = rd
        .flatten()
        .filter(|e| ok(&e.path()))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| !n.starts_with('.'))
        .collect();
    subs.sort();
    subs
}

/// 构建根显示的名字：settings 里的 `rootProject.name`，没写就是目录名（同 Gradle）
pub fn root_name(dir: &Path) -> String {
    let declared = SETTINGS.iter().find_map(|f| std::fs::read_to_string(dir.join(f)).ok()).and_then(|t| {
        t.lines().find_map(|l| l.trim().strip_prefix("rootProject.name").map(|r| r.trim_start_matches([' ', '=']).trim().trim_matches(['"', '\'']).to_string()))
    });
    declared.filter(|n| !n.is_empty()).unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// settings 文件里 `include` 的项目路径（`:a`、`:a:b`，没写冒号的补上）。认 Kotlin 和 Groovy 两种写法：
/// `include("a", ":b:c")`、`include 'a', 'b'`、一行一个 `include("x")`。字符串里拼变量、`project(":x").projectDir = …` 改目录的不认（照默认目录）
pub fn includes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.split("//").next().unwrap_or("").trim();
        let Some(rest) = l.strip_prefix("include") else { continue };
        if !rest.starts_with(['(', ' ', '\t']) {
            continue;
        }
        // 一行里所有引号括起来的
        let mut s = rest;
        while let Some(i) = s.find(['"', '\'']) {
            let q = s.as_bytes()[i] as char;
            let after = &s[i + 1..];
            let Some(j) = after.find(q) else { break };
            let p = &after[..j];
            if !p.is_empty() && !p.contains('$') {
                out.push(if p.starts_with(':') { p.to_string() } else { format!(":{p}") });
            }
            s = &after[j + 1..];
        }
    }
    out
}

/// 一个构建根里的模块：根项目（有 build 文件或者 src/ 才算）+ settings 里 include 的。`rel` 是目录（`:a:b` → `a/b`，Gradle 的默认），
/// `artifact_id` 放 Gradle 的项目路径（`:a:b`；根是 `:`）—— 任务名、`gradlew :a:b:test` 都用它
pub fn modules(dir: &Path) -> Vec<Module> {
    let text = SETTINGS.iter().find_map(|f| std::fs::read_to_string(dir.join(f)).ok()).unwrap_or_default();
    let mut out = Vec::new();
    if BUILD.iter().any(|f| dir.join(f).is_file()) || dir.join("src").is_dir() {
        out.push(Module { rel: String::new(), artifact_id: ":".into(), parent: None });
    }
    for p in includes(&text) {
        let rel = p.trim_start_matches(':').replace(':', "/");
        if out.iter().any(|m: &Module| m.artifact_id == p) {
            continue;
        }
        out.push(Module { rel, artifact_id: p, parent: Some(String::new()) });
    }
    out
}

/// Gradle 怎么开头：有 `gradlew` 用它（项目要的版本写在 `gradle/wrapper/` 里；没执行位的用 `sh`），没有用 PATH 里的 `gradle`。
/// 带 `--console=plain`：运行窗不是终端，富控制台那些回车刷新的进度会糊成一片
pub fn invocation(dir: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let exe = match std::fs::metadata(dir.join("gradlew")) {
        Ok(m) if m.is_file() => {
            if m.permissions().mode() & 0o111 != 0 { "./gradlew" } else { "sh ./gradlew" }
        }
        _ => "gradle",
    };
    format!("{exe} --console=plain")
}

/// `:a:b` + 任务名 → `:a:b:任务`；根项目（`:`）→ `:任务`
fn task(project: &str, name: &str) -> String {
    if project == ":" { format!(":{name}") } else { format!("{project}:{name}") }
}

/// 起这个入口的那一行（在构建根上跑）：先让 init 脚本那个任务算好 classpath（顺带编好本模块和它依赖的兄弟模块），再 `exec java`。
/// 运行配置（VM 参数、profile、环境变量、程序参数）照 Maven 那条拼 —— 形状一样，调试、热替换认得
pub fn run_command(project: &str, module_rel: &str, main_class: &str, gradle: &str, run: &crate::projectcfg::RunCfg) -> Option<String> {
    let init = init_script()?;
    let build = format!("{gradle} -q --init-script {} {}", word(&init.to_string_lossy()), task(project, CP_TASK));
    let cd = if module_rel.is_empty() { String::new() } else { format!("cd {} && ", word(module_rel)) };
    let env: Vec<String> = run.env.iter().filter(|(k, _)| crate::projectcfg::env_name_ok(k)).map(|(k, v)| format!("{k}={}", word(v))).collect();
    let env = if env.is_empty() { String::new() } else { format!("export {} && ", env.join(" ")) };
    let vm = if run.vm_options.trim().is_empty() { String::new() } else { format!(" {}", run.vm_options.trim()) };
    let profiles = crate::projectcfg::profile_names(&run.profiles);
    let profiles = if profiles.is_empty() { String::new() } else { format!(" {}", word(&format!("-Dspring.profiles.active={}", profiles.join(",")))) };
    let args = if run.program_args.trim().is_empty() { String::new() } else { format!(" {}", run.program_args.trim()) };
    Some(format!(r#"{build} && {cd}{env}exec java{vm}{profiles} -cp "$(cat {CP_FILE})" {}{args}"#, word(main_class)))
}

/// 跑单测：选择器是 Maven 那种（`com.demo.OrderTest`、`A#x+y,B#z`，测试旁边的 ▶ 和「重跑没过的」给的就是它），翻成 Gradle 的
/// `--tests`：一个类一个，一个方法一个（`A.x`）。没带包名的类名（Maven 工具窗那种）前面加 `*.`。选择器有别的字符给 None。
///
/// **带 `--rerun`**：上次过了、这次什么都没改，Gradle 说 `test UP-TO-DATE` 就不跑了（2026-10-26 实测）—— 人按 ⌃⇧R 就是要它跑，
/// 不跑的话没有新报告、结果树是空的，调试时更是等不来那个 JVM。带 init 脚本：调试时它把调试参数交给测试的 JVM（[`with_debug`]）
pub fn test_command(project: &str, selector: &str, gradle: &str) -> Option<String> {
    if selector.is_empty() || !selector.chars().all(|c| c.is_ascii_alphanumeric() || "_.#$*+,".contains(c)) {
        return None;
    }
    let mut filters = Vec::new();
    for part in selector.split(',').filter(|p| !p.is_empty()) {
        let (class, methods) = part.split_once('#').unwrap_or((part, ""));
        let class = if class.contains('.') || class.starts_with('*') { class.to_string() } else { format!("*.{class}") };
        if methods.is_empty() {
            filters.push(class);
        } else {
            filters.extend(methods.split('+').filter(|m| !m.is_empty()).map(|m| format!("{class}.{m}")));
        }
    }
    let tests: String = filters.iter().map(|f| format!(" --tests {}", word(f))).collect();
    let init = init_script().map(|p| format!(" --init-script {}", word(&p.to_string_lossy()))).unwrap_or_default();
    Some(format!("{gradle}{init} {} --rerun{tests}", task(project, "test")))
}

/// 调试单测：测试的 JVM 是 Gradle daemon 起的，不在我们的进程组里（第 0 步实测），输出里也等不到「Listening for transport」那一行
/// （Gradle 的 `--debug-jvm` 实测：JVM 在 5005 上听着了，客户端的输出停在 testClasses）。所以端口由我们定、交给 init 脚本，调试器就等这个端口。
/// 不是我们拼的单测命令（没有 init 脚本）给 None
pub fn with_debug(command: &str, port: u16) -> Option<String> {
    if !command.contains(" --init-script ") || !command.contains(" --rerun") {
        return None;
    }
    let jdwp = format!("-PliteIdeJdwp=-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}");
    Some(format!("{command} {}", word(&jdwp)))
}

/// 构建项目（⌘F9）：每个构建根 `classes testClasses`（IDEA 的 Build 连测试一起编）；重新构建前面加 `clean`。`--continue`：一个模块编不过，
/// 别的模块照样编完，错误一次列全（同 Maven 那边几个 reactor 都跑完再说）。没有构建根给 None
pub fn build_command(roots: &[String], clean: bool, gradle: &dyn Fn(&str) -> String) -> Option<String> {
    let tasks = if clean { "clean classes testClasses" } else { "classes testClasses" };
    let one = |r: &str| format!("{} --continue {tasks}", gradle(r));
    match roots {
        [] => None,
        [r] if r.is_empty() => Some(one(r)),
        rs => Some(format!(
            "rc=0; {}exit $rc",
            rs.iter().map(|r| format!("(cd {} && {}) || rc=1; ", word(if r.is_empty() { "." } else { r }), one(r))).collect::<String>()
        )),
    }
}

/// Gradle 工具窗里的一个任务（`gradlew tasks --all` 列出来的）
#[derive(Clone, Debug, PartialEq)]
pub struct GTask {
    /// Gradle 项目路径：根项目 `:`，子项目 `:order-service`
    pub project: String,
    pub name: String,
    /// 分组（Build、Verification、Application…；没分组的在 Other）
    pub group: String,
    pub desc: String,
}

/// 列任务的那一条：`tasks --all`（子项目的也列上、没分组的也列上）。`-q` 不打构建的进度，报告本身照打（实测热的 0.6 秒）
pub fn tasks_command(gradle: &str) -> String {
    format!("{gradle} -q tasks --all")
}

/// `tasks --all` 的报告 → 任务。认结构：一行标题、下一行同样长的 `-`，之后到空行为止每行 `[项目:]任务[ - 说明]`。
/// 「Tasks runnable from …」那个大标题、「Rules」（规则不是任务：`clean<TaskName>`）不算
pub fn parse_tasks(text: &str) -> Vec<GTask> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        let (title, rule) = (lines[i].trim_end(), lines[i + 1].trim_end());
        let is_head = !title.is_empty() && !rule.is_empty() && rule.chars().all(|c| c == '-') && rule.chars().count() == title.chars().count();
        if !is_head || title.starts_with("Tasks runnable") || title == "Rules" {
            i += 1;
            continue;
        }
        let group = title.strip_suffix(" tasks").unwrap_or(title).to_string();
        i += 2;
        while i < lines.len() && !lines[i].trim().is_empty() {
            let l = lines[i].trim();
            let (name, desc) = l.split_once(" - ").unwrap_or((l, ""));
            let (project, task) = match name.rsplit_once(':') {
                Some((p, t)) => (format!(":{}", p.trim_start_matches(':')), t),
                None => (":".to_string(), name),
            };
            if !task.is_empty() && task.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                out.push(GTask { project, name: task.to_string(), group: group.clone(), desc: desc.trim().to_string() });
            }
            i += 1;
        }
    }
    out
}

/// 跑一次 `tasks --all`（在构建根上、登录 shell 里 —— 同 Maven 工具窗算插件那条，PATH、JAVA_HOME 照你的 rc 文件）。
/// 输出设闸（rust.md 第一问）：几十个模块的项目报告也就几十 KB，读前 256 KB；读满了交回 `true`（少列了任务，界面上说一句），不悄悄截
pub fn list_tasks(dir: &Path, shell: &str) -> Result<(Vec<GTask>, bool), String> {
    const CAP: usize = 256 << 10;
    let shell = if shell.is_empty() { std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()) } else { shell.to_string() };
    let mut c = std::process::Command::new(&shell);
    c.args(["-ilc", &tasks_command(&invocation(dir))]).current_dir(dir);
    let out = procutil::run_capped(&mut c, CAP, procutil::ERR_CAP, procutil::Overflow::Drain).map_err(|e| format!("起不了 Gradle：{e}"))?;
    if !out.status.success() {
        // Gradle 失败时最有用的是「What went wrong」那一段，在 stderr 里
        let err = String::from_utf8_lossy(&out.stderr);
        let why: Vec<&str> = err.lines().skip_while(|l| !l.contains("What went wrong")).skip(1).take_while(|l| !l.starts_with("* Try")).filter(|l| !l.trim().is_empty()).take(8).collect();
        return Err(if why.is_empty() { format!("Gradle 失败了（{}）：{}", out.status, err.lines().last().unwrap_or("")) } else { why.join("\n") });
    }
    Ok((parse_tasks(&String::from_utf8_lossy(&out.stdout)), out.truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-26 真 Gradle 9.8.1 的 `tasks --all`，节选（两个子项目、Spring Boot 插件）
    const TASKS: &str = "
------------------------------------------------------------
Tasks runnable from root project 'shopdemo'
------------------------------------------------------------

Application tasks
-----------------
order-service:bootRun - Runs this project as a Spring Boot application.

Build tasks
-----------
order-service:build - Assembles and tests this project.
shop-common:build - Assembles and tests this project.

Build Setup tasks
-----------------
wrapper - Generates Gradle wrapper files.

Verification tasks
------------------
shop-common:test - Runs the test suite.

Other tasks
-----------
prepareKotlinBuildScriptModel
order-service:compileJava - Compiles main Java source.

Rules
-----
Pattern: clean<TaskName>: Cleans the output files of a task.
";

    #[test]
    fn tasks_报告_按组拆_带项目前缀的归子项目_没说明的也认_大标题和规则不算() {
        let t = parse_tasks(TASKS);
        let row = |p: &str, n: &str| t.iter().find(|x| x.project == p && x.name == n).map(|x| (x.group.as_str(), x.desc.as_str()));
        assert_eq!(row(":order-service", "bootRun"), Some(("Application", "Runs this project as a Spring Boot application.")));
        assert_eq!(row(":shop-common", "build").map(|r| r.0), Some("Build"));
        assert_eq!(row(":", "wrapper").map(|r| r.0), Some("Build Setup"), "没前缀的是根项目的");
        assert_eq!(row(":", "prepareKotlinBuildScriptModel"), Some(("Other", "")), "没说明的照列");
        assert_eq!(t.len(), 7, "{t:?}");
        assert!(!t.iter().any(|x| x.name.contains('<') || x.name.starts_with("Pattern")), "规则不是任务");
    }

    #[test]
    fn settings_里的_include_两种写法都认_注释和拼变量的不认() {
        let kts = "rootProject.name = \"shop\"\ninclude(\"shop-common\", \"order-service\")\ninclude(\":services:pay\")\n// include(\"old\")\ninclude(\"x-$v\")\n";
        assert_eq!(includes(kts), [":shop-common", ":order-service", ":services:pay"]);
        let groovy = "include 'a', ':b'\nincludeBuild('../lib')\n";
        assert_eq!(includes(groovy), [":a", ":b"], "includeBuild 不是 include");
    }

    #[test]
    fn 单测选择器翻成_tests_一个类一个_一个方法一个_没包名的加通配() {
        // 没设 init 脚本的路径（这条测试单独跑时）就不带它；设了的样子见下面「起应用」那条
        let c = test_command(":shop-common", "com.demo.MoneyTest", "./gradlew --console=plain").unwrap();
        assert!(c.starts_with("./gradlew --console=plain") && c.ends_with(" :shop-common:test --rerun --tests com.demo.MoneyTest"), "{c}");
        assert!(test_command(":", "A#x+y,b.B#z", "g").unwrap().ends_with(" :test --rerun --tests '*.A.x' --tests '*.A.y' --tests b.B.z"));
        assert!(test_command(":a", "A; rm -rf ~", "g").is_none());
    }

    #[test]
    fn init_脚本_多个线程同时要_都拿得到() {
        let d = std::env::temp_dir().join(format!("lite-gradle-init-race-{}", std::process::id()));
        set_init_file(d.join("lite-ide-init.gradle"));
        let f = INIT_FILE.get().unwrap().clone();
        for _ in 0..20 {
            // 每轮先把盘上的弄旧，逼所有线程都去写
            #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
            let _ = std::fs::write(&f, "old");
            let hs: Vec<_> = (0..8).map(|_| std::thread::spawn(init_script)).collect();
            assert!(hs.into_iter().all(|h| h.join().unwrap().is_some()), "并发写 init 脚本时有一个拿到 None");
        }
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 起应用_先算_classpath_再_exec_java_调试插得进() {
        let d = std::env::temp_dir().join(format!("lite-gradle-init-{}", std::process::id()));
        set_init_file(d.join("lite-ide-init.gradle"));
        let run = crate::projectcfg::RunCfg { profiles: "dev".into(), vm_options: "-Xmx256m".into(), ..Default::default() };
        let c = run_command(":order-service", "order-service", "com.demo.order.OrderApplication", "./gradlew --console=plain", &run).unwrap();
        assert!(c.starts_with("./gradlew --console=plain -q --init-script "), "{c}");
        assert!(c.contains(" :order-service:liteIdeClasspath && cd order-service && exec java -Xmx256m -Dspring.profiles.active=dev -cp \"$(cat build/.lite-ide-cp.txt)\" com.demo.order.OrderApplication"), "{c}");
        // 路径是进程里第一个设的那个（并行跑的别的测试可能先设了）：读真正生效的那个
        assert_eq!(std::fs::read_to_string(INIT_FILE.get().unwrap()).unwrap(), INIT_SCRIPT, "init 脚本写进了应用数据目录");
        let dbg = crate::debug::with_jdwp(&c, None).unwrap();
        assert!(dbg.contains(&format!("exec java {} -Xmx256m", crate::debug::JDWP)), "{dbg}");
        // 调试单测：命令带着 init 脚本和 --rerun，端口是我们定的
        let t = test_command(":shop-common", "com.demo.MoneyTest", "./gradlew").unwrap();
        assert!(t.contains(" --init-script ") && t.contains(" --rerun"), "{t}");
        assert_eq!(with_debug(&t, 50123).unwrap(), format!("{t} -PliteIdeJdwp=-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:50123"));
        assert!(with_debug("./gradlew test", 1).is_none(), "不是我们拼的命令不猜");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 构建_每个根_classes_testClasses_一个编不过别的照编() {
        let g = |r: &str| if r.is_empty() { "./gradlew".to_string() } else { format!("{r}/gw") };
        assert_eq!(build_command(&["".into()], false, &g).unwrap(), "./gradlew --continue classes testClasses");
        assert_eq!(build_command(&["a".into(), "b".into()], true, &g).unwrap(), "rc=0; (cd a && a/gw --continue clean classes testClasses) || rc=1; (cd b && b/gw --continue clean classes testClasses) || rc=1; exit $rc");
        assert!(build_command(&[], false, &g).is_none());
    }
}
