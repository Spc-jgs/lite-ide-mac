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
    walk(root, root, &mut out, &mut seen);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<Module>, seen: &mut std::collections::HashSet<PathBuf>) {
    let Ok(canon) = dir.canonicalize() else { return };
    if !seen.insert(canon) {
        return;
    }
    let Some(text) = read_pom(&dir.join("pom.xml")) else { return };
    let (artifact, subs) = parse_pom(&text);
    let rel = dir.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
    out.push(Module { rel, artifact_id: artifact.unwrap_or_default() });
    for m in subs {
        // `<module>` 也可以直接写成 `x/pom.xml`
        let d = dir.join(m.trim_end_matches("/pom.xml"));
        walk(root, &d, out, seen);
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

/// 放进 shell 命令行的一个词（路径、类名）：普通的原样，带空格之类的套单引号
fn word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./:@+=,%".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// 起这个入口的那一行（在 reactor 根上跑）。单模块项目（入口就在根模块）不用 `-pl -am`
pub fn run_command(app: &BootApp) -> String {
    let build = if app.module.is_empty() {
        format!("mvn -B -ntp -DskipTests compile dependency:build-classpath -Dmdep.outputFile={CP_FILE}")
    } else {
        format!("mvn -B -ntp -pl {} -am -DskipTests compile dependency:build-classpath -Dmdep.outputFile={CP_FILE}", word(&app.module))
    };
    let cd = if app.module.is_empty() { String::new() } else { format!("cd {} && ", word(&app.module)) };
    format!(r#"{build} && {cd}exec java -cp "target/classes:$(cat {CP_FILE})" {}"#, word(&app.main_class))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let got: Vec<(String, String)> = modules(&d).into_iter().map(|m| (m.rel, m.artifact_id)).collect();
        assert_eq!(
            got,
            [
                ("".into(), "root".into()),
                ("common".into(), "common".into()),
                ("services".into(), "services".into()),
                ("services/order".into(), "order-service".into()),
            ]
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
    fn 起的命令_多模块带_pl_am_单模块不带_怪路径套引号() {
        let multi = run_command(&BootApp { module: "services/order".into(), main_class: "o.OrderApplication".into() });
        assert_eq!(
            multi,
            r#"mvn -B -ntp -pl services/order -am -DskipTests compile dependency:build-classpath -Dmdep.outputFile=target/.lite-ide-cp.txt && cd services/order && exec java -cp "target/classes:$(cat target/.lite-ide-cp.txt)" o.OrderApplication"#
        );
        let single = run_command(&BootApp { module: "".into(), main_class: "a.App".into() });
        assert!(!single.contains("-pl") && !single.contains("cd ") && single.ends_with(" a.App"));
        assert!(run_command(&BootApp { module: "my service".into(), main_class: "a.App".into() }).contains("cd 'my service'"));
    }
}
