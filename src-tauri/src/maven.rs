//! Maven 项目和 Spring Boot 应用（docs/IDEA.md 第 1 步）：模块树怎么读、哪些类是 Spring Boot 的入口、怎么照 IDEA 那样起它。
//!
//! 纯函数 + 读盘，不碰进程（起和停是任务那套，`tasksvc`）。放在主 crate、挨着 `taskdefs.rs`：它产出的就是任务定义。
//!
//! # 认出来，不猜
//!
//! 入口 = 模块 `src/main/java` 里带 `@SpringBootApplication`、又有 `static void main` 的类 —— 作者写下的声明，同 package.json 的 scripts。
//! 不带 profile 起（IDEA 的默认也不带；用户的配置全在 yml 里）。要带 profile、环境变量的，在 tasks.json 里写一个同名的任务顶掉它（同名以文件为准）。
//!
//! # 怎么起：照 IDEA，不走 spring-boot:run
//!
//! ```text
//! mvn -B -ntp -pl <模块> -am -DskipTests compile dependency:build-classpath -Dmdep.outputFile=target/.lite-ide-cp.txt
//!   && cd <模块> && exec java -cp "target/classes:$(cat target/.lite-ide-cp.txt)" <主类>
//! ```
//!
//! `-am` 让兄弟模块（xxx-common）一起编译，classpath 里它们是各自的 `target/classes` —— 改了公共模块再起就是新的，不用 install。
//! `mvn -pl x spring-boot:run` 实测 `Could not find artifact …:shop-common`（不带 -am 时兄弟模块要从本地仓库找；install 过的话跑的是旧 jar，还不报错）。
//! 实测数字和排除的路在 docs/IDEA.md 第 8 节。`exec java`：shell 换成 java 本身，停任务时信号直接到 JVM（Spring 的关闭钩子跑得到）。
//!
//! # pom 怎么读
//!
//! 不引 XML 库（加依赖要先问；也用不着）：要的只是 `<modules>` 和 `<artifactId>` 两样。先去掉注释，再把**不属于这一层**的段落整段挖掉
//! （`<profiles>` 里的 modules 默认不生效、`<parent>` / `<dependencies>` 里的 artifactId 不是自己的），剩下的第一个就是。

use std::path::{Path, PathBuf};

/// 一份 pom 最多读多少：人写的几 KB 到几十 KB，超过的多半不是 pom
const MAX_POM: u64 = 2 << 20;
/// 找入口类最多看多少个 .java（单个模块）：大模块几千个，翻到上限就停，说出来
const MAX_SOURCES: usize = 20_000;
/// 单个 .java 超过这个大小不读（生成的代码），入口类不会这么大
const MAX_SOURCE_BYTES: u64 = 512 * 1024;
/// 算出来的 classpath 写在模块的 target/ 里（构建产物的目录，本来就被 git 忽略、mvn clean 会清）
pub const CP_FILE: &str = "target/.lite-ide-cp.txt";

/// Maven 的一个模块
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// 相对 reactor 根（根模块是 ""）
    pub rel: String,
    pub artifact_id: String,
    /// 在哪个模块的 `<modules>` 里（相对 reactor 根）；根模块是 None。Maven 工具窗照 IDEA 把子模块挂在它底下
    pub parent: Option<String>,
}

/// 一个 Spring Boot 入口
#[derive(Debug, Clone, PartialEq)]
pub struct BootApp {
    /// 所在模块，相对 reactor 根（单模块项目是 ""）
    pub module: String,
    /// 全限定类名
    pub main_class: String,
}

impl BootApp {
    /// 任务列表里的名字：主类的简单名，同 IDEA 的运行配置
    pub fn name(&self) -> &str {
        self.main_class.rsplit('.').next().unwrap_or(&self.main_class)
    }
}

/// 去掉 `<!-- … -->`
fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        match rest[i..].find("-->") {
            Some(j) => rest = &rest[i + j + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// 把 `<tag>…</tag>` 整段挖掉（可能有好几段）
fn drop_sections(s: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(&open) {
        out.push_str(&rest[..i]);
        match rest[i..].find(&close) {
            Some(j) => rest = &rest[i + j + close.len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `<tag>值</tag>` 的所有值（去掉两头空白）
fn values<'a>(s: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(&close) else { break };
        out.push(after[..j].trim());
        rest = &after[j + close.len()..];
    }
    out
}

/// 项目要的 Java 版本（按项目挑 JDK 用，docs/IDEA.md 第 23 节）：按这个次序找第一个写了的 ——
/// `maven.compiler.release` > 编译插件的 `<release>` > `maven.compiler.target` > `maven.compiler.source` > `java.version`（Spring Boot 的父 pom 拿它去设前面几个）。
/// `1.8` 是 8；`${名字}` 在同一份 pom 里解一次。profile 里的不算（默认不生效）。没写给 None —— 那就照旧用你的默认 JDK，不猜
pub fn java_release(text: &str) -> Option<u32> {
    let s = drop_sections(&strip_comments(text), "profiles");
    let num = |v: &str| -> Option<u32> {
        let v = v.trim();
        let v = v.strip_prefix("1.").unwrap_or(v);
        v.split('.').next()?.parse().ok()
    };
    for tag in ["maven.compiler.release", "release", "maven.compiler.target", "maven.compiler.source", "java.version"] {
        let Some(v) = values(&s, tag).into_iter().find(|v| !v.is_empty()) else { continue };
        if let Some(name) = v.strip_prefix("${").and_then(|x| x.strip_suffix('}')) {
            if let Some(n) = values(&s, name).first().and_then(|x| num(x)) {
                return Some(n);
            }
            continue;
        }
        if let Some(n) = num(v) {
            return Some(n);
        }
    }
    None
}

/// 一个模块要的 Java 版本：先看模块自己的 pom，再看 reactor 根的（子模块一般从根继承）
pub fn module_java(reactor: &Path, module_rel: &str) -> Option<u32> {
    let own = if module_rel.is_empty() { None } else { read_pom(&reactor.join(module_rel).join("pom.xml")).and_then(|t| java_release(&t)) };
    own.or_else(|| read_pom(&reactor.join("pom.xml")).and_then(|t| java_release(&t)))
}

/// 一份 pom 自己这一层的 artifactId 和默认生效的子模块
pub fn parse_pom(text: &str) -> (Option<String>, Vec<String>) {
    let mut s = strip_comments(text);
    for tag in ["profiles", "parent", "dependencies", "dependencyManagement", "build", "reporting", "pluginRepositories", "repositories", "distributionManagement"] {
        s = drop_sections(&s, tag);
    }
    let artifact = values(&s, "artifactId").first().map(|v| v.to_string());
    let modules = values(&s, "module").into_iter().filter(|m| !m.is_empty()).map(String::from).collect();
    (artifact, modules)
}

fn read_pom(p: &Path) -> Option<String> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > MAX_POM {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// 从 reactor 根（有 pom.xml 的目录）把模块树摊平：根自己在第一个，子模块按 pom 里的先后、深度优先。
/// 指向不存在的目录、或者转圈（a 写 b、b 又写 a）的跳过
pub fn modules(root: &Path) -> Vec<Module> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    walk(root, root, None, &mut out, &mut seen);
    out
}

fn walk(root: &Path, dir: &Path, parent: Option<String>, out: &mut Vec<Module>, seen: &mut std::collections::HashSet<PathBuf>) {
    let Ok(canon) = dir.canonicalize() else { return };
    if !seen.insert(canon) {
        return;
    }
    let Some(text) = read_pom(&dir.join("pom.xml")) else { return };
    let (artifact, subs) = parse_pom(&text);
    let rel = dir.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
    out.push(Module { rel: rel.clone(), artifact_id: artifact.unwrap_or_default(), parent });
    for m in subs {
        // `<module>` 也可以直接写成 `x/pom.xml`
        let d = dir.join(m.trim_end_matches("/pom.xml"));
        walk(root, &d, Some(rel.clone()), out, seen);
    }
}

/// 一个 .java 文件是不是 Spring Boot 入口，是的话返回全限定类名
fn boot_main(text: &str, file_stem: &str) -> Option<String> {
    if !text.contains("@SpringBootApplication") || !text.contains("static void main") {
        return None;
    }
    let pkg = text
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("package ").map(|p| p.trim_end_matches(';').trim().to_string()));
    Some(match pkg {
        Some(p) if !p.is_empty() => format!("{p}.{file_stem}"),
        _ => file_stem.to_string(),
    })
}

/// 在各模块的 `src/main/java` 里找 Spring Boot 入口。返回（入口们，问题）—— 翻到上限的模块要说出来，不然少列了一个服务没人知道
pub fn boot_apps(root: &Path, mods: &[Module]) -> (Vec<BootApp>, Vec<String>) {
    let mut apps = Vec::new();
    let mut problems = Vec::new();
    for m in mods {
        let src = root.join(&m.rel).join("src/main/java");
        if !src.is_dir() {
            continue;
        }
        let mut stack = vec![src];
        let mut seen = 0usize;
        let mut found: Vec<String> = Vec::new();
        'walk: while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            let mut entries: Vec<_> = rd.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let p = e.path();
                let Ok(t) = e.file_type() else { continue };
                if t.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "java") {
                    seen += 1;
                    if seen > MAX_SOURCES {
                        problems.push(format!("{} 的 Java 文件超过 {MAX_SOURCES} 个，没翻完：可能少列了入口", if m.rel.is_empty() { "根模块" } else { &m.rel }));
                        break 'walk;
                    }
                    if e.metadata().is_ok_and(|md| md.len() > MAX_SOURCE_BYTES) {
                        continue;
                    }
                    let Ok(text) = std::fs::read_to_string(&p) else { continue };
                    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(c) = boot_main(&text, &stem) {
                        found.push(c);
                    }
                }
            }
        }
        found.sort();
        apps.extend(found.into_iter().map(|main_class| BootApp { module: m.rel.clone(), main_class }));
    }
    (apps, problems)
}

/// Maven 工具窗 Lifecycle 底下的阶段：IDEA 默认显示的那 9 个（它的「只显示基本阶段」）
pub const PHASES: &[&str] = &["clean", "validate", "compile", "test", "package", "verify", "install", "site", "deploy"];

/// Maven 工具窗工具栏上的那几个开关（IDEA 的离线、跳过测试）和勾上的 profile。按项目记在应用数据目录（`maven_run.rs`），
/// **跑目标和起 Spring Boot 服务都带上** —— 勾了 dev profile、起服务却没带，是最难查的那种「怎么跟 IDEA 里跑的不一样」
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    pub offline: bool,
    pub skip_tests: bool,
    pub profiles: Vec<String>,
}

impl Prefs {
    /// 开在每条 mvn 后面的那几个：`-o`、`-P a,b`（profile 名不安全的丢掉 —— 它们要进 shell 命令行）
    fn flags(&self) -> String {
        let mut f = String::new();
        if self.offline {
            f.push_str(" -o");
        }
        let ps: Vec<&str> = self.profiles.iter().map(String::as_str).filter(|p| safe_token(p)).collect();
        if !ps.is_empty() {
            f.push_str(&format!(" -P {}", ps.join(",")));
        }
        f
    }
}

/// `maven-prefs.json` 在哪（启动时设一次）。没设（单元测试、还没起完）就当全是默认：不离线、不跳过测试、不带 profile
static PREFS_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn set_prefs_file(p: PathBuf) {
    let _ = PREFS_FILE.set(p);
}

fn load_all() -> std::collections::BTreeMap<String, Prefs> {
    PREFS_FILE.get().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// 这个项目（项目根）的开关。每次现读：几十字节的文件，和「两个窗口各改一个开关」比，不值得缓存
pub fn prefs(root: &Path) -> Prefs {
    load_all().remove(&root.to_string_lossy().into_owned()).unwrap_or_default()
}

/// 记下这个项目的开关。全默认的那条删掉，不留一堆空记录
#[allow(clippy::disallowed_methods)] // 应用自己的 maven-prefs.json：写临时文件再改名盖过去（同 trust_store）
pub fn set_prefs(root: &Path, p: &Prefs) -> std::io::Result<()> {
    let Some(file) = PREFS_FILE.get() else { return Ok(()) };
    let mut all = load_all();
    let key = root.to_string_lossy().into_owned();
    if *p == Prefs::default() {
        all.remove(&key);
    } else {
        all.insert(key, p.clone());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&all).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, file)
}

/// 能原样放进 mvn 命令行的一个词：阶段、插件目标（`spring-boot:run`、`g:a:v:goal`）、`-Dx=y`、profile 名。
/// 只许这些字符 —— 没有空格、引号、`;` `$` `|` `&` 这类 shell 认的东西，拼不出第二条命令。原来是 7 个目标的白名单，
/// 工具窗照 IDEA 做了插件目标和「执行 Maven 目标」（自己敲）之后白名单不够用，换成按字符卡
fn safe_token(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:=,@/+%".contains(c))
}

/// 跑一个模块的目标（在 reactor 根上跑）。`goals` 可以是几个词（「执行 Maven 目标」里敲的 `clean install -DskipTests`），每个词都要过 [`safe_token`]。
/// 根模块：整个 reactor。别的模块：`-pl 模块 -am`，连它依赖的兄弟模块一起 ——
/// **不是** `-f 模块/pom.xml`：那样只构建这一个模块，兄弟模块要从本地仓库找，没 install 过就
/// `Could not resolve dependencies … shop-common`（2026-10-14 在样例上实测；第一版就是这么写的，单元测试还把错的命令锁住了）。
/// IDEA 的 Maven 工具窗能用 `-f`，是因为它的 runner 带「resolve workspace artifacts」—— 那是 IDEA 自己的扩展，普通的 mvn 没有。
/// 只有 `clean` 的时候例外只带 `-pl`：清这个模块，别把兄弟模块也清了。代价如实说：`test` 带 `-am` 会连兄弟模块的测试一起跑。
/// 有不安全的词给 None
pub fn goal_command(module: &str, goals: &str, prefs: &Prefs) -> Option<String> {
    let toks: Vec<&str> = goals.split_whitespace().collect();
    if toks.is_empty() || !toks.iter().all(|t| safe_token(t)) {
        return None;
    }
    let g = toks.join(" ");
    let skip = if prefs.skip_tests { " -DskipTests" } else { "" };
    let base = format!("mvn -B -ntp{}{skip}", prefs.flags());
    Some(if module.is_empty() {
        format!("{base} {g}")
    } else if toks == ["clean"] {
        format!("{base} -pl {} clean", word(module))
    } else {
        format!("{base} -pl {} -am {g}", word(module))
    })
}

/// 跑单测的选择器能不能放进命令行：`com.demo.OrderTest`、`com.demo.OrderTest#adds`、几个用 `,` / `+` 连起来（重跑失败的：`A#x+y,B#z`）、
/// 内部类的 `$`（放进命令时加引号）、通配的 `*`。别的字符一律不认
fn safe_selector(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_.#$*+,".contains(c))
}

/// 跑单测（docs/IDEA.md 第 17 节）：`-Dtest=类#方法`，在 reactor 根上跑 `-pl 模块 -am test`（兄弟模块不用 install，同 [`goal_command`]）。
/// `-am` 会把上游模块也跑一遍 test 阶段 —— 它们里面没有这个类，surefire 默认「指定了测试却一个都没跑」就失败，
/// 所以带 `-Dsurefire.failIfNoSpecifiedTests=false`。带 profile 和离线（同跑目标），**不带跳过测试**（就是来跑测试的）。选择器不安全给 None
pub fn test_command(module: &str, selector: &str, prefs: &Prefs) -> Option<String> {
    if !safe_selector(selector) {
        return None;
    }
    let base = format!("mvn -B -ntp{} -Dtest={} -Dsurefire.failIfNoSpecifiedTests=false", prefs.flags(), word(selector));
    Some(if module.is_empty() { format!("{base} test") } else { format!("{base} -pl {} -am test", word(module)) })
}

/// 构建项目（⌘F9，docs/IDEA.md 第 22 节）：每个 reactor 跑一遍 `test-compile` —— IDEA 的 Build 连测试一起编；`clean` 是「重新构建」。
/// 用 Maven 不用 jdtls 的增量构建：「运行」走的是 `mvn compile`，Build 说编得过、运行却编不过就白问了（第 22 节有对照）。
/// 带 profile 和离线；不带跳过测试（`-DskipTests` 本来也不跳过编译测试）。几个 reactor 都跑完再说结果（IDEA 也是全编完才列错误），
/// 有一个没过整个算失败。没有 reactor（不是 Maven 项目）给 None
/// `jdk`：每个 reactor 前面加的那段（「用项目要的那个 JDK」，见 `taskdefs::jdk_prefix`；没有就是空的）
pub fn build_command(reactors: &[String], prefs: &Prefs, clean: bool, jdk: &dyn Fn(&str) -> String) -> Option<String> {
    let goal = if clean { "clean test-compile" } else { "test-compile" };
    let one = format!("mvn -B -ntp{} {goal}", prefs.flags());
    match reactors {
        [] => None,
        [r] if r.is_empty() => Some(format!("{}{one}", jdk(r))),
        rs => Some(format!(
            "rc=0; {}exit $rc",
            rs.iter().map(|r| format!("(cd {} && {}{one}) || rc=1; ", word(if r.is_empty() { "." } else { r }), jdk(r))).collect::<String>()
        )),
    }
}

/// 算插件和依赖的那一条（`maven_run.rs`）：有效 POM 写到 `scratch`（绝对路径），依赖树写进各模块的 `deps_file`（相对模块）。
/// 带离线和 profile（profile 会改插件和依赖），不带跳过测试（这两个目标不跑测试）
pub fn resolve_command(prefs: &Prefs, scratch: &Path, deps_file: &str) -> String {
    format!(
        "mvn -B -ntp -q{} help:effective-pom -Doutput={} dependency:tree -DoutputFile={}",
        prefs.flags(),
        word(&scratch.to_string_lossy()),
        word(deps_file)
    )
}

/// 插件 jar 不在本地仓库、读不到描述时，按 Maven 的命名约定猜前缀：`xxx-maven-plugin` → `xxx`，`maven-xxx-plugin` → `xxx`
pub fn default_prefix(artifact_id: &str) -> String {
    if let Some(x) = artifact_id.strip_suffix("-maven-plugin") {
        return x.to_string();
    }
    if let Some(x) = artifact_id.strip_prefix("maven-").and_then(|x| x.strip_suffix("-plugin")) {
        return x.to_string();
    }
    artifact_id.to_string()
}

/// 一份 pom 里声明的 profile：`(id, activeByDefault)`。只看这一层的 `<profiles>`
pub fn pom_profiles(text: &str) -> Vec<(String, bool)> {
    let s = strip_comments(text);
    let mut out = Vec::new();
    let Some(a) = s.find("<profiles>") else { return out };
    let Some(b) = s[a..].find("</profiles>") else { return out };
    let block = &s[a..a + b];
    let mut rest = block;
    while let Some(i) = rest.find("<profile>") {
        let after = &rest[i + "<profile>".len()..];
        let Some(j) = after.find("</profile>") else { break };
        let p = &after[..j];
        // profile 里还能有 build / dependencies，id 只取这一层的
        let mut own = p.to_string();
        for tag in ["build", "dependencies", "dependencyManagement", "modules", "properties", "repositories", "pluginRepositories", "reporting"] {
            own = drop_sections(&own, tag);
        }
        if let Some(id) = values(&own, "id").first() {
            let on = values(&own, "activeByDefault").first().is_some_and(|v| *v == "true");
            out.push((id.to_string(), on));
        }
        rest = &after[j + "</profile>".len()..];
    }
    out
}

/// 有效 POM（`help:effective-pom` 的输出）里一个项目用到的插件
#[derive(Debug, Clone, PartialEq)]
pub struct PluginRef {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
}

/// `help:effective-pom` 的输出 → 每个项目（按 artifactId）的 `<build><plugins>`。多模块时外面套一层 `<projects>`。
/// 不算 `<pluginManagement>`（那是给子模块的默认值，不是这个模块真用的插件）
pub fn parse_effective(text: &str) -> Vec<(String, Vec<PluginRef>)> {
    let s = strip_comments(text);
    let mut out = Vec::new();
    let mut rest = s.as_str();
    loop {
        // `<project ` / `<project>`，不是 `<projects>`
        let Some(i) = rest.find("<project") else { break };
        let tail = &rest[i + "<project".len()..];
        if !(tail.starts_with(' ') || tail.starts_with('>') || tail.starts_with('\n')) {
            rest = tail;
            continue;
        }
        let Some(j) = tail.find("</project>") else { break };
        let body = &tail[..j];
        rest = &tail[j + "</project>".len()..];
        let build = body.find("<build>").and_then(|a| body[a..].find("</build>").map(|b| &body[a..a + b])).unwrap_or("");
        let build = drop_sections(build, "pluginManagement");
        let mut plugins = Vec::new();
        let mut r = build.as_str();
        while let Some(a) = r.find("<plugin>") {
            let after = &r[a + "<plugin>".len()..];
            let Some(b) = after.find("</plugin>") else { break };
            let mut own = after[..b].to_string();
            for tag in ["dependencies", "executions", "configuration"] {
                own = drop_sections(&own, tag);
            }
            let first = |t: &str| values(&own, t).first().map(|v| v.to_string());
            if let Some(artifact_id) = first("artifactId") {
                plugins.push(PluginRef {
                    group_id: first("groupId").unwrap_or_else(|| "org.apache.maven.plugins".into()),
                    artifact_id,
                    version: first("version").unwrap_or_default(),
                });
            }
            r = &after[b + "</plugin>".len()..];
        }
        let mut own = body.to_string();
        for tag in ["parent", "dependencies", "dependencyManagement", "build", "profiles", "reporting", "repositories", "pluginRepositories", "distributionManagement", "licenses", "developers", "scm"] {
            own = drop_sections(&own, tag);
        }
        if let Some(a) = values(&own, "artifactId").first() {
            out.push((a.to_string(), plugins));
        }
    }
    out
}

/// 插件 jar 里的 `META-INF/maven/plugin.xml` → `(goalPrefix, 目标)`。IDEA 的 Plugins 节点就是这么列目标的
pub fn parse_plugin_xml(text: &str) -> (String, Vec<String>) {
    let s = strip_comments(text);
    let prefix = values(&s, "goalPrefix").first().map(|v| v.to_string()).unwrap_or_default();
    let mut goals: Vec<String> = Vec::new();
    for g in values(&s, "goal") {
        if !g.is_empty() && !goals.iter().any(|x| x == g) {
            goals.push(g.to_string());
        }
    }
    (prefix, goals)
}

/// 依赖树里的一项
#[derive(Debug, Clone, PartialEq)]
pub struct Dep {
    /// 0 = 直接依赖
    pub depth: u32,
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub scope: String,
    pub optional: bool,
}

/// `dependency:tree` 的文本输出（`-DoutputFile` 写出来的那份）→ 依赖列表，带深度。第一行是模块自己，跳过。
/// 一行长这样：`|  +- org.springframework:spring-core:jar:7.0.1:compile`；带 classifier 的多一段（6 段）；可选依赖后面跟 ` (optional)`
pub fn parse_dep_tree(text: &str) -> Vec<Dep> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let start = line.find(|c: char| !"|+\\- ".contains(c)).unwrap_or(line.len());
        if start == line.len() {
            continue;
        }
        let depth = (start / 3).saturating_sub(1) as u32;
        let rest = &line[start..];
        let (coord, note) = match rest.find(" (") {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let parts: Vec<&str> = coord.split(':').collect();
        let (g, a, version, scope) = match parts.len() {
            5 => (parts[0], parts[1], parts[3], parts[4]),
            6 => (parts[0], parts[1], parts[4], parts[5]),
            _ => continue,
        };
        out.push(Dep { depth, group_id: g.into(), artifact_id: a.into(), version: version.into(), scope: scope.into(), optional: note.contains("optional") });
    }
    out
}

/// 项目里的 reactor 根：项目根有 pom.xml 就是它；没有就找一层子目录（前后端放一个仓库：`backend/pom.xml`）。相对项目根
pub fn reactors(root: &Path) -> Vec<String> {
    if root.join("pom.xml").is_file() {
        return vec![String::new()];
    }
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut subs: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().join("pom.xml").is_file())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| !n.starts_with('.'))
        .collect();
    subs.sort();
    subs
}

/// 放进 shell 命令行的一个词（路径、类名）：普通的原样，带空格之类的套单引号
pub(crate) fn word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./:@+=,%".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// 起这个入口的那一行（在 reactor 根上跑）。单模块项目（入口就在根模块）不用 `-pl -am`。
/// Maven 工具窗勾的 profile、离线开关一起带上（`prefs`）：profile 会改依赖，起服务时不带就和工具窗里看到的不是一回事
pub fn run_command(app: &BootApp, prefs: &Prefs) -> String {
    let f = prefs.flags();
    let build = if app.module.is_empty() {
        format!("mvn -B -ntp{f} -DskipTests compile dependency:build-classpath -Dmdep.outputFile={CP_FILE}")
    } else {
        format!("mvn -B -ntp{f} -pl {} -am -DskipTests compile dependency:build-classpath -Dmdep.outputFile={CP_FILE}", word(&app.module))
    };
    let cd = if app.module.is_empty() { String::new() } else { format!("cd {} && ", word(&app.module)) };
    format!(r#"{build} && {cd}exec java -cp "target/classes:$(cat {CP_FILE})" {}"#, word(&app.main_class))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 项目要的_java_版本_按次序找_1点8_是_8_变量解一次_profile_里的不算() {
        assert_eq!(java_release("<properties><java.version>17</java.version></properties>"), Some(17), "Spring Boot 的写法");
        assert_eq!(java_release("<properties><maven.compiler.source>1.8</maven.compiler.source><maven.compiler.target>1.8</maven.compiler.target></properties>"), Some(8));
        assert_eq!(java_release("<properties><java.version>11</java.version><maven.compiler.release>21</maven.compiler.release></properties>"), Some(21), "release 优先");
        assert_eq!(java_release("<properties><jdk>21</jdk><maven.compiler.release>${jdk}</maven.compiler.release></properties>"), Some(21), "变量");
        assert_eq!(java_release("<build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><configuration><release>17</release></configuration></plugin></plugins></build>"), Some(17));
        assert_eq!(java_release("<profiles><profile><properties><java.version>8</java.version></properties></profile></profiles>"), None, "profile 里的默认不生效");
        assert_eq!(java_release("<!-- <java.version>8</java.version> --><artifactId>a</artifactId>"), None, "注释掉的不算");
    }

    #[test]
    fn 构建_每个_reactor_一遍_test_compile_重新构建先_clean_有一个没过整体失败() {
        let p = Prefs { offline: true, skip_tests: true, profiles: vec!["dev".into()] };
        let none = |_: &str| String::new();
        assert_eq!(build_command(&[String::new()], &p, false, &none).unwrap(), "mvn -B -ntp -o -P dev test-compile", "跳过测试不带（不该跳过编译测试）");
        assert_eq!(build_command(&[String::new()], &p, true, &none).unwrap(), "mvn -B -ntp -o -P dev clean test-compile");
        // 每个 reactor 各自的 JDK 加在它自己那段里
        let jdk = |r: &str| if r == "api" { "export JAVA_HOME=/j17; ".to_string() } else { String::new() };
        let two = build_command(&["api".into(), "my svc".into()], &Prefs::default(), false, &jdk).unwrap();
        assert_eq!(two, "rc=0; (cd api && export JAVA_HOME=/j17; mvn -B -ntp test-compile) || rc=1; (cd 'my svc' && mvn -B -ntp test-compile) || rc=1; exit $rc");
        assert_eq!(build_command(&[], &p, false, &none), None, "不是 Maven 项目");
    }

    #[test]
    fn 跑单测_带选择器和放过上游模块的开关_不带跳过测试_内部类加引号_不安全的不拼() {
        let p = Prefs { offline: true, skip_tests: true, profiles: vec!["dev".into()] };
        let c = test_command("order-service", "com.demo.OrderTest#adds", &p).unwrap();
        // `#` 也加了引号（word 的白名单里没有它）：shell 里词中间的 # 本来不是注释，加上同样对
        assert!(c.contains("-Dtest='com.demo.OrderTest#adds'") && c.contains("-pl order-service -am test") && c.contains("-Dsurefire.failIfNoSpecifiedTests=false"), "{c}");
        assert!(c.contains(" -o") && c.contains("-P dev") && !c.contains("skipTests"), "带离线和 profile、不带跳过测试：{c}");
        assert!(test_command("", "com.demo.OrderTest", &p).unwrap().ends_with("-Dsurefire.failIfNoSpecifiedTests=false test"), "根模块不带 -pl");
        assert!(test_command("m", "com.demo.Outer$Inner", &p).unwrap().contains("-Dtest='com.demo.Outer$Inner'"), "$ 在 shell 里会展开，要加引号");
        assert!(test_command("m", "A#x+y,B#z", &p).is_some(), "重跑失败的：几个方法几个类");
        for bad in ["A; rm -rf ~", "A`id`", "A#x y", ""] {
            assert!(test_command("m", bad, &p).is_none(), "{bad:?} 不该拼进命令");
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lite-ide-maven-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
    fn put(p: PathBuf, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn pom_只认自己这一层_注释和_profile_里的不算() {
        let (a, m) = parse_pom(
            r#"<project>
  <parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId></parent>
  <artifactId> shop </artifactId>
  <!-- <modules><module>old</module></modules> -->
  <modules>
    <module>common</module>
    <module>services/order/pom.xml</module>
  </modules>
  <dependencies><dependency><artifactId>lombok</artifactId></dependency></dependencies>
  <profiles><profile><id>extra</id><modules><module>docker</module></modules></profile></profiles>
</project>"#,
        );
        assert_eq!(a.as_deref(), Some("shop"), "parent 里的 artifactId 不是自己的");
        assert_eq!(m, ["common", "services/order/pom.xml"], "注释里的、profile 里的模块默认不生效");
    }

    #[test]
    fn 模块树摊平_嵌套_指向不存在的跳过_转圈不死循环() {
        let d = tmp("tree");
        put(d.join("pom.xml"), "<project><artifactId>root</artifactId><modules><module>common</module><module>services</module><module>gone</module></modules></project>");
        put(d.join("common/pom.xml"), "<project><artifactId>common</artifactId></project>");
        put(d.join("services/pom.xml"), "<project><artifactId>services</artifactId><modules><module>order</module><module>..</module></modules></project>");
        put(d.join("services/order/pom.xml"), "<project><artifactId>order-service</artifactId></project>");
        let got: Vec<(String, String, Option<String>)> = modules(&d).into_iter().map(|m| (m.rel, m.artifact_id, m.parent)).collect();
        assert_eq!(
            got,
            [
                ("".into(), "root".into(), None),
                ("common".into(), "common".into(), Some("".into())),
                ("services".into(), "services".into(), Some("".into())),
                ("services/order".into(), "order-service".into(), Some("services".into())),
            ],
            "父模块是把它列进 <modules> 的那个（工具窗照 IDEA 嵌着画）"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 认出入口_要注解也要_main() {
        assert_eq!(
            boot_main("package com.demo.order;\n@SpringBootApplication\npublic class OrderApplication {\n public static void main(String[] a) {}\n}", "OrderApplication").as_deref(),
            Some("com.demo.order.OrderApplication")
        );
        assert_eq!(boot_main("@SpringBootApplication\nclass Conf {}", "Conf"), None, "测试里的配置类：有注解没有 main，不是入口");
        assert_eq!(boot_main("public static void main(String[] a) {}", "Tool"), None, "普通 main 不是 Spring Boot 应用");
    }

    #[test]
    fn 各模块找入口_按模块先后() {
        let d = tmp("apps");
        put(d.join("pom.xml"), "<project><artifactId>root</artifactId><modules><module>common</module><module>order</module><module>user</module></modules></project>");
        for m in ["common", "order", "user"] {
            put(d.join(format!("{m}/pom.xml")), &format!("<project><artifactId>{m}</artifactId></project>"));
        }
        put(d.join("common/src/main/java/c/Money.java"), "package c;\npublic record Money(long cents) {}");
        put(d.join("order/src/main/java/o/OrderApplication.java"), "package o;\n@SpringBootApplication\npublic class OrderApplication { public static void main(String[] a) {} }");
        put(d.join("user/src/main/java/u/UserApplication.java"), "package u;\n@SpringBootApplication\npublic class UserApplication { public static void main(String[] a) {} }");
        put(d.join("user/src/test/java/u/TestApp.java"), "package u;\n@SpringBootApplication\npublic class TestApp { public static void main(String[] a) {} }");
        let (apps, problems) = boot_apps(&d, &modules(&d));
        assert!(problems.is_empty());
        let got: Vec<(&str, &str, &str)> = apps.iter().map(|a| (a.module.as_str(), a.main_class.as_str(), a.name())).collect();
        assert_eq!(got, [("order", "o.OrderApplication", "OrderApplication"), ("user", "u.UserApplication", "UserApplication")], "src/test 里的不算");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 这一行在 docs/IDEA.md 第 8 节实测过：兄弟模块是 target/classes、application.yml 读得到。改它就要再测一遍
    #[test]
    fn 目标_模块带兄弟一起_clean_只清自己_拼不进别的命令() {
        let p = Prefs::default();
        assert_eq!(goal_command("", "clean", &p).as_deref(), Some("mvn -B -ntp clean"), "根模块：整个 reactor");
        assert_eq!(goal_command("order", "package", &p).as_deref(), Some("mvn -B -ntp -pl order -am package"), "-f 只构建这一个、兄弟要从本地仓库找（实测失败）");
        assert_eq!(goal_command("order", "clean", &p).as_deref(), Some("mvn -B -ntp -pl order clean"), "clean 不带 -am：别把兄弟模块也清了");
        assert_eq!(goal_command("my svc", "install", &p).as_deref(), Some("mvn -B -ntp -pl 'my svc' -am install"));
        assert_eq!(goal_command("order", "spring-boot:run", &p).as_deref(), Some("mvn -B -ntp -pl order -am spring-boot:run"), "插件目标");
        assert_eq!(goal_command("", "clean install -DskipTests", &p).as_deref(), Some("mvn -B -ntp clean install -DskipTests"), "自己敲的几个词");
        for bad in ["clean; rm -rf /", "package && curl x", "$(id)", "a|b", "'x'", ""] {
            assert_eq!(goal_command("order", bad, &p), None, "{bad:?} 拼得进第二条命令");
        }
    }

    #[test]
    fn 工具栏的开关和勾的_profile_带进每条命令_起服务也带() {
        let p = Prefs { offline: true, skip_tests: true, profiles: vec!["dev".into(), "local-db".into(), "bad;x".into()] };
        assert_eq!(goal_command("order", "package", &p).as_deref(), Some("mvn -B -ntp -o -P dev,local-db -DskipTests -pl order -am package"), "不安全的 profile 名丢掉");
        let run = run_command(&BootApp { module: "order".into(), main_class: "o.App".into() }, &p);
        assert!(run.starts_with("mvn -B -ntp -o -P dev,local-db -pl order -am -DskipTests compile"), "{run}");
    }

    #[test]
    fn profile_只认这一层_带默认开的() {
        let ps = pom_profiles(
            r#"<project><artifactId>x</artifactId>
  <!-- <profiles><profile><id>old</id></profile></profiles> -->
  <profiles>
    <profile><build><plugins><plugin><executions><execution><id>not-a-profile</id></execution></executions></plugin></plugins></build>
      <id>dev</id><activation><activeByDefault>true</activeByDefault></activation></profile>
    <profile><id>prod</id></profile>
  </profiles></project>"#,
        );
        assert_eq!(ps, [("dev".to_string(), true), ("prod".to_string(), false)]);
        assert!(pom_profiles("<project/>").is_empty());
    }

    #[test]
    fn 有效_pom_每个项目的插件_不算_plugin_management_和插件自己的依赖() {
        let text = r#"<?xml version="1.0"?>
<projects>
  <project xmlns="http://maven.apache.org/POM/4.0.0">
    <parent><artifactId>spring-boot-starter-parent</artifactId></parent>
    <artifactId>order-service</artifactId>
    <dependencies><dependency><artifactId>lombok</artifactId></dependency></dependencies>
    <build>
      <pluginManagement><plugins><plugin><artifactId>managed-only</artifactId></plugin></plugins></pluginManagement>
      <plugins>
        <plugin><artifactId>maven-compiler-plugin</artifactId><version>3.14.0</version>
          <dependencies><dependency><artifactId>not-a-plugin</artifactId></dependency></dependencies></plugin>
        <plugin><groupId>org.springframework.boot</groupId><artifactId>spring-boot-maven-plugin</artifactId><version>4.0.1</version>
          <executions><execution><id>repackage</id></execution></executions></plugin>
      </plugins>
    </build>
  </project>
  <project><artifactId>shop-common</artifactId><build><plugins/></build></project>
</projects>"#;
        let got = parse_effective(text);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "order-service", "自己的 artifactId，不是 parent 的");
        let names: Vec<(&str, &str, &str)> = got[0].1.iter().map(|p| (p.group_id.as_str(), p.artifact_id.as_str(), p.version.as_str())).collect();
        assert_eq!(names, [("org.apache.maven.plugins", "maven-compiler-plugin", "3.14.0"), ("org.springframework.boot", "spring-boot-maven-plugin", "4.0.1")]);
        assert_eq!(got[1], ("shop-common".to_string(), vec![]));
    }

    #[test]
    fn 算插件和依赖的命令_带离线和_profile_不带跳过测试_路径套引号() {
        let p = Prefs { offline: true, skip_tests: true, profiles: vec!["dev".into()] };
        assert_eq!(
            resolve_command(&p, Path::new("/data/my app/eff.xml"), "target/.lite-ide-deps.txt"),
            "mvn -B -ntp -q -o -P dev help:effective-pom -Doutput='/data/my app/eff.xml' dependency:tree -DoutputFile=target/.lite-ide-deps.txt"
        );
        assert_eq!(default_prefix("spring-boot-maven-plugin"), "spring-boot");
        assert_eq!(default_prefix("maven-surefire-plugin"), "surefire");
        assert_eq!(default_prefix("exec"), "exec");
    }

    #[test]
    fn 插件描述_前缀和目标() {
        let (prefix, goals) = parse_plugin_xml("<plugin><goalPrefix>spring-boot</goalPrefix><mojos><mojo><goal>run</goal></mojo><mojo><goal>repackage</goal></mojo><mojo><goal>run</goal></mojo></mojos></plugin>");
        assert_eq!(prefix, "spring-boot");
        assert_eq!(goals, ["run", "repackage"]);
    }

    #[test]
    fn 依赖树_深度_带_classifier_的_可选的() {
        let deps = parse_dep_tree(
            "com.demo:order-service:jar:0.0.1-SNAPSHOT\n\
+- com.demo:shop-common:jar:0.0.1-SNAPSHOT:compile\n\
|  \\- org.projectlombok:lombok:jar:1.18.42:compile (optional)\n\
\\- io.netty:netty-resolver-dns-native-macos:jar:osx-aarch_64:4.2.7.Final:runtime\n",
        );
        let got: Vec<(u32, &str, &str, &str, bool)> = deps.iter().map(|d| (d.depth, d.artifact_id.as_str(), d.version.as_str(), d.scope.as_str(), d.optional)).collect();
        assert_eq!(
            got,
            [
                (0, "shop-common", "0.0.1-SNAPSHOT", "compile", false),
                (1, "lombok", "1.18.42", "compile", true),
                (0, "netty-resolver-dns-native-macos", "4.2.7.Final", "runtime", false),
            ]
        );
    }

    #[test]
    fn 起的命令_多模块带_pl_am_单模块不带_怪路径套引号() {
        let multi = run_command(&BootApp { module: "services/order".into(), main_class: "o.OrderApplication".into() }, &Prefs::default());
        assert_eq!(
            multi,
            r#"mvn -B -ntp -pl services/order -am -DskipTests compile dependency:build-classpath -Dmdep.outputFile=target/.lite-ide-cp.txt && cd services/order && exec java -cp "target/classes:$(cat target/.lite-ide-cp.txt)" o.OrderApplication"#
        );
        let single = run_command(&BootApp { module: "".into(), main_class: "a.App".into() }, &Prefs::default());
        assert!(!single.contains("-pl") && !single.contains("cd ") && single.ends_with(" a.App"));
        assert!(run_command(&BootApp { module: "my service".into(), main_class: "a.App".into() }, &Prefs::default()).contains("cd 'my service'"));
    }
}
