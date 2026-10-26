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
const INIT_SCRIPT: &str = r#"// lite-ide 加的（docs/TAKEOVER.md G24）：算一个模块的运行 classpath，写进它的 build/.lite-ide-cp.txt。不写进你的项目
allprojects {
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
        let tmp = f.with_extension("gradle.tmp");
        std::fs::write(&tmp, INIT_SCRIPT).ok()?;
        std::fs::rename(&tmp, f).ok()?;
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
/// `--tests`：一个类一个，一个方法一个（`A.x`）。没带包名的类名（Maven 工具窗那种）前面加 `*.`。选择器有别的字符给 None
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
    Some(format!("{gradle} {}{tests}", task(project, "test")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_里的_include_两种写法都认_注释和拼变量的不认() {
        let kts = "rootProject.name = \"shop\"\ninclude(\"shop-common\", \"order-service\")\ninclude(\":services:pay\")\n// include(\"old\")\ninclude(\"x-$v\")\n";
        assert_eq!(includes(kts), [":shop-common", ":order-service", ":services:pay"]);
        let groovy = "include 'a', ':b'\nincludeBuild('../lib')\n";
        assert_eq!(includes(groovy), [":a", ":b"], "includeBuild 不是 include");
    }

    #[test]
    fn 单测选择器翻成_tests_一个类一个_一个方法一个_没包名的加通配() {
        assert_eq!(test_command(":shop-common", "com.demo.MoneyTest", "./gradlew --console=plain").unwrap(), "./gradlew --console=plain :shop-common:test --tests com.demo.MoneyTest");
        assert_eq!(test_command(":", "A#x+y,b.B#z", "g").unwrap(), "g :test --tests '*.A.x' --tests '*.A.y' --tests b.B.z");
        assert!(test_command(":a", "A; rm -rf ~", "g").is_none());
    }

    #[test]
    fn 起应用_先算_classpath_再_exec_java_调试插得进() {
        let d = std::env::temp_dir().join(format!("lite-gradle-init-{}", std::process::id()));
        set_init_file(d.join("lite-ide-init.gradle"));
        let run = crate::projectcfg::RunCfg { profiles: "dev".into(), vm_options: "-Xmx256m".into(), ..Default::default() };
        let c = run_command(":order-service", "order-service", "com.demo.order.OrderApplication", "./gradlew --console=plain", &run).unwrap();
        assert!(c.starts_with("./gradlew --console=plain -q --init-script "), "{c}");
        assert!(c.contains(" :order-service:liteIdeClasspath && cd order-service && exec java -Xmx256m -Dspring.profiles.active=dev -cp \"$(cat build/.lite-ide-cp.txt)\" com.demo.order.OrderApplication"), "{c}");
        assert_eq!(std::fs::read_to_string(d.join("lite-ide-init.gradle")).unwrap(), INIT_SCRIPT, "init 脚本写进了应用数据目录");
        let dbg = crate::debug::with_jdwp(&c, None).unwrap();
        assert!(dbg.contains(&format!("exec java {} -Xmx256m", crate::debug::JDWP)), "{dbg}");
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
