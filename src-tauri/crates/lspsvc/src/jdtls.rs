//! jdtls 怎么起：找 JDK 21、找 jdtls、找 Lombok、拼参数。数字和理由在 docs/IDEA.md 第 7 节（第 0 步实测），改参数之前先看那一节。
//!
//! 三个必须带的参数都是实测出来的，不是照抄 vscode-java：
//!
//! - `-Djava.import.generatesMetadataFilesAtProjectRoot=false`：不带的话 jdtls 往**每个模块**写 `.project` / `.classpath` / `.settings/` /
//!   `.factorypath`，用户的 `.gitignore` 没盖住就是 `git status` 里几十个陌生文件。**只认 JVM 系统属性**：放进 initialize 的 settings 实测照写；
//!   反汇编 `JLSFsUtils.generatesMetadataFilesAtProjectRoot()` 看到它只读 `System.getProperty`、没设就当 true。
//! - `-javaagent:<lombok.jar>`：不挂的话用 Lombok 的项目满屏「The method getName() is undefined」，跳转、悬停都是空。
//! - `-data <应用数据目录下按项目分的目录>`：缓存在这里，第二次起 1.2 秒能答（第一次 3.5 秒）。
//!
//! 这里全是纯函数（目录、文件内容都是参数），测试不用真装 JDK。

use crate::server::Spec;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// jdtls 自己要的 JDK 版本下限（它编译的项目可以是 8–25，跑它的 JVM 要 21+）
pub const MIN_JAVA: u32 = 21;

/// JDK 的 `release` 文件里的 `JAVA_VERSION="21.0.12.1"` → 21；老格式 `"1.8.0_402"` → 8
pub fn java_version(release: &str) -> Option<u32> {
    let line = release.lines().find_map(|l| l.strip_prefix("JAVA_VERSION="))?;
    let v = line.trim().trim_matches('"');
    let mut parts = v.split(['.', '_', '+', '-']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        return parts.next()?.parse().ok();
    }
    Some(first)
}

/// 在这些目录里找 `*/Contents/Home`，挑版本 ≥ `min` 里最高的那个。读 `release` 文件判版本，不起 `java -version`：
/// 一个目录一次读文件，不起子进程；还顺带认得 IDEA 下载到 `~/Library/Java/JavaVirtualMachines` 的那些
pub fn find_jdk(dirs: &[PathBuf], min: u32) -> Option<(u32, PathBuf)> {
    let mut best: Option<(u32, PathBuf)> = None;
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let home = e.path().join("Contents/Home");
            let Ok(text) = std::fs::read_to_string(home.join("release")) else { continue };
            let Some(v) = java_version(&text) else { continue };
            if v >= min && !home.join("bin/java").is_file() {
                continue;
            }
            if v >= min && best.as_ref().is_none_or(|(b, _)| v > *b) {
                best = Some((v, home));
            }
        }
    }
    best
}

/// macOS 上 JDK 装在哪：系统那份（pkg / Homebrew cask）、用户自己的（IDEA 下载的）、sdkman、Homebrew 的 formula（`openjdk@17`，
/// Apple 芯片在 `/opt/homebrew/opt`、Intel 在 `/usr/local/opt`）、asdf、jenv、mise。原来只扫前三处，后面几种装的找不到
/// （docs/SETTINGS.md 13.1）；找不到的还能在设置里手动加（`java.jdks`，[`jdk_at`]）
pub fn default_jdk_dirs(user_home: &Path) -> Vec<PathBuf> {
    vec![
        "/Library/Java/JavaVirtualMachines".into(),
        user_home.join("Library/Java/JavaVirtualMachines"),
        user_home.join(".sdkman/candidates/java"),
        "/opt/homebrew/opt".into(),
        "/usr/local/opt".into(),
        user_home.join(".asdf/installs/java"),
        user_home.join(".jenv/versions"),
        user_home.join(".local/share/mise/installs/java"),
    ]
}

/// 一个目录是不是 JDK：本身就是 JAVA_HOME，或者是 macOS 的包（`Contents/Home`），或者是 Homebrew formula 的目录
/// （`libexec/openjdk.jdk/Contents/Home`）。按 `release` 文件判版本、要有 `bin/java`（同 [`find_jdk`]）。手动加的也走它
pub fn jdk_at(dir: &Path) -> Option<(u32, PathBuf)> {
    for home in [dir.join("Contents/Home"), dir.join("libexec/openjdk.jdk/Contents/Home"), dir.to_path_buf()] {
        let Ok(text) = std::fs::read_to_string(home.join("release")) else { continue };
        if let (Some(v), true) = (java_version(&text), home.join("bin/java").is_file()) {
            return Some((v, home));
        }
    }
    None
}

/// 这些目录里全部的 JDK：(主版本, JAVA_HOME)，按版本排。**按真实路径去重**：sdkman 的 `current`、jenv 的版本别名、
/// Homebrew 的 `openjdk` 和 `openjdk@21` 都是指到同一个 JDK 的软链，不去重的话一个 JDK 列好几遍
pub fn list_jdks(dirs: &[PathBuf]) -> Vec<(u32, PathBuf)> {
    let mut out: Vec<(u32, PathBuf)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        // 先看不是软链的（真身），软链指过去的那个已经见过就跳过 —— 列出来的路径是真身那条
        entries.sort_by_key(|p| (p.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), p.clone()));
        for e in entries {
            let Some((v, home)) = jdk_at(&e) else { continue };
            if seen.insert(home.canonicalize().unwrap_or_else(|_| home.clone())) {
                out.push((v, home));
            }
        }
    }
    out.sort();
    out
}

/// 项目要 Java `want`：主版本一样的优先（多个就挑排在前面的，稳定）；没有就挑比它高、离它最近的（javac 的 `--release` 往下兼容）；
/// 都没有给 None（只装了更老的）。IDEA 是让人选项目 SDK；我们按 pom 说的挑，同 Lithe 的做法
pub fn pick_jdk(jdks: &[(u32, PathBuf)], want: u32) -> Option<(u32, PathBuf)> {
    jdks.iter().find(|(v, _)| *v == want).or_else(|| jdks.iter().filter(|(v, _)| *v > want).min_by_key(|(v, _)| *v)).cloned()
}

/// 一个 jdtls 的目录（解开的发行包，里面有 `plugins/` 和 `config_*`）
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub home: PathBuf,
    pub launcher: PathBuf,
    /// 这个架构的 `config.ini` 所在目录（`config_mac_arm` / `config_mac`）
    pub config: PathBuf,
}

/// 认一个 jdtls 目录。认不出来时说清缺什么（用户在设置里填错路径时看得懂）
pub fn layout(home: &Path) -> Result<Layout, String> {
    let plugins = home.join("plugins");
    let rd = std::fs::read_dir(&plugins).map_err(|_| format!("{} 里没有 plugins 目录，不像是解开的 jdtls", home.display()))?;
    let mut launchers: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("org.eclipse.equinox.launcher_") && n.ends_with(".jar"))
        })
        .collect();
    launchers.sort();
    let launcher = launchers.pop().ok_or_else(|| format!("{} 里没有 org.eclipse.equinox.launcher_*.jar", plugins.display()))?;
    let arch = if cfg!(target_arch = "aarch64") { "config_mac_arm" } else { "config_mac" };
    let config = home.join(arch);
    if !config.join("config.ini").is_file() {
        return Err(format!("{} 里没有 {arch}/config.ini", home.display()));
    }
    Ok(Layout { home: home.to_path_buf(), launcher, config })
}

/// Maven 本地仓库在哪：用户的 `settings.xml` 里写了 `<localRepository>` 就用它（这台机器就是 `~/repository`），否则默认那个。
/// 只读用户那份 settings.xml，不读 Maven 安装目录里的全局那份 —— 那要先找到 mvn 装在哪；全局那份改本地仓库路径的很少见。
/// 读错了的后果有限：只影响找 Lombok（找不到就不挂 agent，状态栏会说），jdtls 自己的 m2e 照样按 Maven 的规矩读
pub fn local_repo(user_home: &Path, settings_xml: Option<&str>) -> PathBuf {
    if let Some(s) = settings_xml {
        let s = strip_comments(s);
        if let Some(a) = s.find("<localRepository>") {
            let rest = &s[a + "<localRepository>".len()..];
            if let Some(b) = rest.find("</localRepository>") {
                let v = rest[..b].trim().replace("${user.home}", &user_home.to_string_lossy());
                if !v.is_empty() {
                    return PathBuf::from(v);
                }
            }
        }
    }
    user_home.join(".m2/repository")
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find("<!--") {
        out.push_str(&rest[..a]);
        match rest[a..].find("-->") {
            Some(b) => rest = &rest[a + b + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// 本地仓库里版本最高的那个 Lombok jar。挑最高的而不是项目依赖的那个：Lombok 的 agent 向后兼容
/// （新版认得老版的注解），而「项目依赖哪个版本」要等 m2e 解析完才知道 —— 那时 jdtls 已经起了，agent 只能在起的时候挂
pub fn lombok_jar(repo: &Path) -> Option<PathBuf> {
    let dir = repo.join("org/projectlombok/lombok");
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        let ver = e.file_name().to_string_lossy().into_owned();
        // 只认纯数字的正式版：edge-SNAPSHOT 这类不挑
        let Some(key) = ver.split('.').map(|p| p.parse::<u32>().ok()).collect::<Option<Vec<u32>>>() else { continue };
        let jar = e.path().join(format!("lombok-{ver}.jar"));
        if jar.is_file() && best.as_ref().is_none_or(|(b, _)| key > *b) {
            best = Some((key, jar));
        }
    }
    best.map(|(_, p)| p)
}

/// 这个项目的 jdtls 缓存放哪：`<base>/<目录名>-<路径的哈希>`。目录名给人看（翻应用数据目录时认得出），哈希防同名
pub fn workspace_dir(base: &Path, root: &Path) -> PathBuf {
    let name: String = root
        .file_name()
        .map(|n| n.to_string_lossy().chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').take(40).collect())
        .unwrap_or_default();
    base.join(format!("{}-{:016x}", if name.is_empty() { "root" } else { &name }, fnv1a(root.to_string_lossy().as_bytes())))
}

/// FNV-1a 64。不用 std 的 `DefaultHasher`：文档写明它的算法不保证跨 Rust 版本不变，换了编译器缓存目录就全换一遍
fn fnv1a(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub struct Launch<'a> {
    pub java_home: &'a Path,
    pub layout: &'a Layout,
    /// [`workspace_dir`] 给的那个；里面放 `config/`（jdtls 的 OSGi 缓存，按项目一份：两个实例写同一份会打架）和 `ws/`
    pub workspace: &'a Path,
    pub lombok: Option<&'a Path>,
    /// 例如 `"1G"`
    pub xmx: &'a str,
    pub root: &'a Path,
}

/// 拼起 jdtls 的命令行。`config/` 由调用方先用 [`prepare_config`] 准备好
pub fn spec(l: &Launch) -> Spec {
    let mut args = vec![
        "-Declipse.application=org.eclipse.jdt.ls.core.id1".to_string(),
        "-Dosgi.bundles.defaultStartLevel=4".into(),
        "-Declipse.product=org.eclipse.jdt.ls.core.product".into(),
        "-Djava.import.generatesMetadataFilesAtProjectRoot=false".into(),
        format!("-Xmx{}", l.xmx),
        // 并行 GC + 闲了把堆还回去：jdtls 导入时吃到 1G、闲下来不用那么多（vscode-java 默认也是这一组）
        "-XX:+UseParallelGC".into(),
        "-XX:GCTimeRatio=4".into(),
        "-XX:AdaptiveSizePolicyWeight=90".into(),
        "-Dsun.zip.disableMemoryMapping=true".into(),
    ];
    if let Some(j) = l.lombok {
        args.push(format!("-javaagent:{}", j.display()));
    }
    args.extend([
        "--add-modules=ALL-SYSTEM".into(),
        "--add-opens".into(),
        "java.base/java.util=ALL-UNNAMED".into(),
        "--add-opens".into(),
        "java.base/java.lang=ALL-UNNAMED".into(),
        "-jar".into(),
        l.layout.launcher.display().to_string(),
        "-configuration".into(),
        l.workspace.join("config").display().to_string(),
        "-data".into(),
        l.workspace.join("ws").display().to_string(),
    ]);
    Spec { program: l.java_home.join("bin/java"), args, cwd: l.root.to_path_buf(), env: vec![] }
}

/// 把发行包里的 `config.ini` 抄进这个项目自己的 `config/`（8KB；已经有就不动 —— 里面还会有 jdtls 自己写的 OSGi 缓存）
pub fn prepare_config(layout: &Layout, workspace: &Path) -> std::io::Result<()> {
    let dir = workspace.join("config");
    std::fs::create_dir_all(&dir)?;
    let dst = dir.join("config.ini");
    if dst.symlink_metadata().is_err() {
        std::fs::copy(layout.config.join("config.ini"), dst)?;
    }
    Ok(())
}

/// 我们声明支持什么。只声明真用得上的：声明了 `workDoneProgress`，jdtls 就会先发 `window/workDoneProgress/create` 来问
pub fn capabilities() -> Value {
    json!({
        // 改名（第 3 步）：documentChanges + 改文件名 —— 改 public 类名时 jdtls 要连 .java 文件一起改名，不声明的话它只改类名、文件名对不上就是编译错误
        "workspace": { "configuration": true, "workspaceFolders": true, "workspaceEdit": { "documentChanges": true, "resourceOperations": ["create", "rename", "delete"] } },
        "textDocument": {
            "synchronization": { "didSave": true },
            "publishDiagnostics": { "relatedInformation": false, "tagSupport": { "valueSet": [1, 2] } },
            "definition": { "linkSupport": false },
            "references": {},
            // 同名高亮（docs/TAKEOVER.md G11）
            "documentHighlight": {},
            // 内嵌提示（G11）：参数名
            "inlayHint": {},
            "rename": { "prepareSupport": true },
            // 快速修复（⌥↵）：要声明认「字面量形式」，不然 jdtls 一项都不回（探针实测 0 项）；resolve 先给标题、选中了再要改动
            "codeAction": {
                "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"] } },
                "isPreferredSupport": true,
                "dataSupport": true,
                "resolveSupport": { "properties": ["edit"] },
            },
            // 参数提示：参数在签名里的位置给偏移（不给的话只给参数的文字，同名的两个参数分不清亮哪个）
            "signatureHelp": { "signatureInformation": { "documentationFormat": ["plaintext"], "parameterInformation": { "labelOffsetSupport": true }, "activeParameterSupport": true }, "contextSupport": true },
            "formatting": {},
            "rangeFormatting": {},
            "hover": { "contentFormat": ["plaintext", "markdown"] },
            "completion": {
                "completionItem": {
                    "snippetSupport": true,
                    "documentationFormat": ["plaintext", "markdown"],
                    "labelDetailsSupport": true,
                    "resolveSupport": { "properties": ["documentation", "detail", "additionalTextEdits"] },
                },
                "contextSupport": true,
            },
        },
    })
}

/// jdtls 自己的 initializationOptions。`classFileContentsSupport`：跳进 jar 里的类时给 `jdt://` 的地址，内容另外用
/// `java/classFileContents` 要（有源码 jar 就是源码，没有就反编译）
///
/// `runtimes`：这台机器上的 JDK（`jdks::all()`），配成 `java.configuration.runtimes`，每个主版本一个。
/// 2026-10-15 配过一份又删了：jdtls 自己会跑 `/usr/libexec/java_home -X`（Eclipse 的 `MacInstalledJREs`），验红时清空照样认得出 JDK 17 ——
/// 当时的结论是「配了等于重复」。**这个结论只对装在那两个系统目录里的 JDK 成立**：`java_home -X` 不认 sdkman、Homebrew formula、asdf 装的，
/// 而运行和构建是认的（docs/SETTINGS.md 13.1：两边看到的不一样）。所以还是配上，和运行用同一份列表。
/// `maven_settings`：设置里指定的 settings.xml（`maven.settingsFile`），m2e 导入时按它找仓库 —— 不然 jdtls 用的是 `~/.m2/settings.xml`，和跑 mvn 的不是一份
pub fn init_options(runtimes: &[(u32, PathBuf)], maven_settings: Option<&str>) -> Value {
    let mut seen = std::collections::HashSet::new();
    let rts: Vec<Value> = runtimes
        .iter()
        .filter(|(v, _)| seen.insert(*v))
        .map(|(v, h)| json!({ "name": if *v <= 8 { format!("JavaSE-1.{v}") } else { format!("JavaSE-{v}") }, "path": h.to_string_lossy() }))
        .collect();
    let mut o = init_options_base();
    o["settings"]["java"]["configuration"] = json!({ "runtimes": rts });
    if let Some(s) = maven_settings.filter(|s| !s.trim().is_empty()) {
        o["settings"]["java"]["configuration"]["maven"] = json!({ "userSettings": s.trim() });
    }
    o
}

fn init_options_base() -> Value {
    json!({
        "extendedClientCapabilities": { "classFileContentsSupport": true, "progressReportProvider": false },
        "settings": { "java": {
            "import": { "maven": { "enabled": true }, "gradle": { "enabled": true } },
            // **不自动构建**（2026-10-20，构建 ⌘F9 的验收撞出来的）：开着的话 jdtls 每次改动都往 `target/classes` 里写 class ——
            // 和 Maven 写的是同一个目录。代码有错时 Eclipse 编译器照样出 class，出错的方法换成一句
            // `throw new Error("Unresolved compilation problem: …")`（javap 实测），而且比源文件新，于是 `mvn compile` 说「Nothing to compile」：
            // 构建报成功、运行（`mvn compile && java`）跑的是这个坏 class、一调到那个方法就抛。关了之后 `target/classes` 只有 Maven 一家写。
            // 编辑器里的实时报错是 jdtls 对开着的文件做的（reconcile），不靠构建；补全、跳转、改名也不靠（java.sh 全过）
            "autobuild": { "enabled": false },
            // 不下源码 jar：开着的话 m2e 导入完就在后台给**每个依赖**下 sources + javadoc —— 往用户的本地仓库里下东西没问过他，
            // 而且收尾时 jdtls 卡在这个下载任务上（实测冷缓存那次 shutdown + exit 之后 20 秒不退，日志最后一句
            // `An internal error occurred during: "Download sources and javadoc"`）。本地仓库里已经有的 sources jar m2e 照样自动挂上；
            // 没有的反编译（IDEA 也是先给反编译、点「Download Sources」才下）
            "maven": { "downloadSources": false },
            // 参数提示：jdtls 默认关着，不开的话 signatureHelp 一个签名都不回（探针实测 `{"signatures":[]}`）
            "signatureHelp": { "enabled": true },
        } },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个假 JDK：`release` 文件 + `bin/java`
    fn fake_jdk(home: &Path, ver: &str) {
        std::fs::create_dir_all(home.join("bin")).unwrap();
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        std::fs::write(home.join("release"), format!("JAVA_VERSION=\"{ver}\"\n")).unwrap();
        #[allow(clippy::disallowed_methods)] // 同上
        std::fs::write(home.join("bin/java"), "").unwrap();
    }

    #[test]
    fn jdtls_的_runtimes_每个主版本一个_8叫1点8_settings_xml_有才配() {
        let o = init_options(&[(8, "/j8".into()), (17, "/j17a".into()), (17, "/j17b".into()), (21, "/j21".into())], Some("/w/settings.xml"));
        let c = &o["settings"]["java"]["configuration"];
        assert_eq!(c["runtimes"], json!([{ "name": "JavaSE-1.8", "path": "/j8" }, { "name": "JavaSE-17", "path": "/j17a" }, { "name": "JavaSE-21", "path": "/j21" }]));
        assert_eq!(c["maven"]["userSettings"], "/w/settings.xml");
        assert_eq!(o["settings"]["java"]["autobuild"]["enabled"], false, "原来那几项都还在");
        assert!(init_options(&[], None)["settings"]["java"]["configuration"].get("maven").is_none());
    }

    #[test]
    fn jdk_三种长相都认_软链指同一个的只算一次_不是jdk的跳过() {
        let d = std::env::temp_dir().join(format!("lite-jdks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        fake_jdk(&d.join("pkg/temurin-17.jdk/Contents/Home"), "17.0.9");
        fake_jdk(&d.join("brew/openjdk@21/libexec/openjdk.jdk/Contents/Home"), "21.0.2");
        fake_jdk(&d.join("asdf/zulu-11.0.20"), "11.0.20");
        std::fs::create_dir_all(d.join("brew/ripgrep/bin")).unwrap();
        std::os::unix::fs::symlink(d.join("brew/openjdk@21"), d.join("brew/openjdk")).unwrap();
        std::os::unix::fs::symlink(d.join("asdf/zulu-11.0.20"), d.join("asdf/current")).unwrap();
        let got = list_jdks(&[d.join("pkg"), d.join("brew"), d.join("asdf")]);
        let names: Vec<(u32, String)> = got.iter().map(|(v, h)| (*v, h.strip_prefix(&d).unwrap().display().to_string())).collect();
        assert_eq!(
            names,
            vec![
                (11, "asdf/zulu-11.0.20".into()),
                (17, "pkg/temurin-17.jdk/Contents/Home".into()),
                (21, "brew/openjdk@21/libexec/openjdk.jdk/Contents/Home".into()),
            ],
            "软链（openjdk、current）不重复列，列的是真身"
        );
        assert_eq!(jdk_at(&d.join("pkg/temurin-17.jdk")).map(|(v, _)| v), Some(17), "手动加的给包目录也认");
        assert_eq!(jdk_at(&d.join("brew/ripgrep")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lspsvc-{name}-{}-{:?}", std::process::id(), std::time::Instant::now()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn jdk(dir: &Path, name: &str, version: &str, has_java: bool) {
        let home = dir.join(name).join("Contents/Home");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        std::fs::write(home.join("release"), format!("IMPLEMENTOR=\"x\"\nJAVA_VERSION=\"{version}\"\n")).unwrap();
        if has_java {
            #[allow(clippy::disallowed_methods)] // 同上
            std::fs::write(home.join("bin/java"), "").unwrap();
        }
    }

    #[test]
    fn 不自动构建_target_classes_只归_maven_写() {
        // 开着的话 jdtls 把有错的代码编成「一调就抛」的 class 写进 target/classes，mvn 还当它是新的（见 init_options 里那段）
        assert_eq!(init_options(&[], None)["settings"]["java"]["autobuild"]["enabled"], false);
    }

    #[test]
    fn 按项目挑_jdk_同版本优先_没有就挑最近的更高版本_sdkman_的也认() {
        let d = tmp("pick");
        let mac = d.join("mac");
        jdk(&mac, "temurin-17.jdk", "17.0.19", true);
        jdk(&mac, "temurin-21.jdk", "21.0.12.1", true);
        jdk(&mac, "zulu-11.jdk", "11.0.2", false);
        // sdkman：没有 Contents/Home，名字底下直接是 JDK；current 是软链不算
        let sdk = d.join("sdk");
        let h = sdk.join("8.0.402-tem");
        std::fs::create_dir_all(h.join("bin")).unwrap();
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        {
            std::fs::write(h.join("release"), "JAVA_VERSION=\"1.8.0_402\"\n").unwrap();
            std::fs::write(h.join("bin/java"), "").unwrap();
        }
        std::os::unix::fs::symlink(&h, sdk.join("current")).unwrap();
        let all = list_jdks(&[mac.clone(), sdk.clone()]);
        assert_eq!(all.iter().map(|(v, _)| *v).collect::<Vec<_>>(), vec![8, 17, 21], "没有 java 的 11 不算、current 不重复算");
        assert_eq!(pick_jdk(&all, 17).unwrap().0, 17, "同版本优先（不是挑最高的 21）");
        assert_eq!(pick_jdk(&all, 11).unwrap().0, 17, "没有 11：挑比它高、最近的");
        assert_eq!(pick_jdk(&all, 8).unwrap().1, h, "sdkman 的 JAVA_HOME 是目录自己");
        assert_eq!(pick_jdk(&all, 25), None, "只有更老的：不硬挑");
    }

    #[test]
    fn 读_release_里的版本() {
        assert_eq!(java_version("IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"21.0.12.1\"\n"), Some(21));
        assert_eq!(java_version("JAVA_VERSION=\"1.8.0_402\""), Some(8));
        assert_eq!(java_version("JAVA_VERSION=\"25\""), Some(25));
        assert_eq!(java_version("JAVA_RUNTIME_VERSION=\"21\""), None);
    }

    #[test]
    fn 找_jdk_挑够格里最高的_两个目录都看_没有_java_的不算() {
        let sys = tmp("sys");
        let user = tmp("user");
        jdk(&sys, "temurin-17.jdk", "17.0.19", true);
        jdk(&sys, "temurin-21.jdk", "21.0.12.1", true);
        jdk(&user, "corretto-23.jdk", "23.0.1", true);
        jdk(&user, "broken-25.jdk", "25", false);
        let (v, home) = find_jdk(&[sys.clone(), user.clone()], MIN_JAVA).unwrap();
        assert_eq!(v, 23);
        assert!(home.starts_with(&user) && home.ends_with("corretto-23.jdk/Contents/Home"));
        assert!(find_jdk(&[sys.join("nope")], MIN_JAVA).is_none());
        let only17 = tmp("only17");
        jdk(&only17, "temurin-17.jdk", "17.0.19", true);
        assert!(find_jdk(&[only17], MIN_JAVA).is_none(), "只有 17 不能当 jdtls 的 JDK");
    }

    #[test]
    fn 本地仓库_读用户_settings_里写的_注释掉的不算() {
        let home = Path::new("/Users/u");
        assert_eq!(local_repo(home, None), PathBuf::from("/Users/u/.m2/repository"));
        let s = "<settings>\n  <localRepository>/Users/u/repository</localRepository>\n</settings>";
        assert_eq!(local_repo(home, Some(s)), PathBuf::from("/Users/u/repository"));
        let s = "<settings><localRepository>${user.home}/m2repo</localRepository></settings>";
        assert_eq!(local_repo(home, Some(s)), PathBuf::from("/Users/u/m2repo"));
        let s = "<settings><!-- <localRepository>/path/to/local/repo</localRepository> --></settings>";
        assert_eq!(local_repo(home, Some(s)), PathBuf::from("/Users/u/.m2/repository"), "Maven 自带的样板里那句是注释");
    }

    #[test]
    fn lombok_挑最高的正式版_按数字比不按字典序() {
        let repo = tmp("repo");
        for v in ["1.18.38", "1.18.9", "1.18.46", "edge-SNAPSHOT"] {
            let d = repo.join("org/projectlombok/lombok").join(v);
            std::fs::create_dir_all(&d).unwrap();
            #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
            std::fs::write(d.join(format!("lombok-{v}.jar")), "").unwrap();
        }
        // 只有 pom、没有 jar 的版本（只解析过依赖、没下载）不算
        std::fs::create_dir_all(repo.join("org/projectlombok/lombok/1.18.50")).unwrap();
        assert_eq!(lombok_jar(&repo).unwrap().file_name().unwrap(), "lombok-1.18.46.jar");
        assert!(lombok_jar(&tmp("empty")).is_none());
    }

    #[test]
    fn 缓存目录_名字认得出_不同路径同名不撞_同路径每次一样() {
        let base = Path::new("/data");
        let a = workspace_dir(base, Path::new("/work/order-service"));
        let b = workspace_dir(base, Path::new("/other/order-service"));
        assert!(a.file_name().unwrap().to_string_lossy().starts_with("order-service-"));
        assert_ne!(a, b);
        assert_eq!(a, workspace_dir(base, Path::new("/work/order-service")));
        // 钉住哈希：换了算法，所有人的缓存都作废、第一次导入要重来
        assert_eq!(fnv1a(b"/work/order-service"), 0x840cae5f3bbd3ca6);
    }

    #[test]
    fn 命令行带着实测定下的三样() {
        let home = tmp("jdtls");
        std::fs::create_dir_all(home.join("plugins")).unwrap();
        std::fs::create_dir_all(home.join("config_mac_arm")).unwrap();
        std::fs::create_dir_all(home.join("config_mac")).unwrap();
        for f in ["plugins/org.eclipse.equinox.launcher_1.7.0.v2026.jar", "config_mac_arm/config.ini", "config_mac/config.ini"] {
            #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
            std::fs::write(home.join(f), "").unwrap();
        }
        let lay = layout(&home).unwrap();
        let ws = tmp("ws");
        prepare_config(&lay, &ws).unwrap();
        assert!(ws.join("config/config.ini").is_file());
        let lombok = PathBuf::from("/repo/lombok-1.18.46.jar");
        let s = spec(&Launch { java_home: Path::new("/jdk21"), layout: &lay, workspace: &ws, lombok: Some(&lombok), xmx: "1G", root: Path::new("/p") });
        assert_eq!(s.program, PathBuf::from("/jdk21/bin/java"));
        assert!(s.args.contains(&"-Djava.import.generatesMetadataFilesAtProjectRoot=false".to_string()));
        assert!(s.args.contains(&"-javaagent:/repo/lombok-1.18.46.jar".to_string()));
        let data = s.args.iter().position(|a| a == "-data").unwrap();
        assert_eq!(s.args[data + 1], ws.join("ws").display().to_string());
        // agent 要在 -jar 之前：-jar 后面的是给程序的参数，JVM 不认
        assert!(s.args.iter().position(|a| a.starts_with("-javaagent")).unwrap() < s.args.iter().position(|a| a == "-jar").unwrap());
        assert!(layout(&ws).is_err());
    }
}
