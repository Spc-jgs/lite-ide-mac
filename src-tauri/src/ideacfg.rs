//! 从 IDEA 带运行配置过来（docs/SETTINGS.md 13.8）：读项目里 `.idea/workspace.xml` 的 RunManager，把 Spring Boot 运行配置
//! （和社区版只能用的 Application 运行配置）转成我们的 [`RunCfg`]，运行配置对话框里点一下填进表单、你点「确定」才存。只读，不往 `.idea` 里写。
//!
//! 字段名、默认值、怎么进命令，照 IDEA 2026.1 核对过，不是照印象（核对的过程在 JOURNAL 2026-10-22）：
//! - `VM_PARAMETERS` / `MAIN_CLASS_NAME` / `ALTERNATIVE_JRE_PATH(_ENABLED)`：`JvmConfigurationOptions.kt`；
//!   `PROGRAM_PARAMETERS` / `WORKING_DIRECTORY` / `PASS_PARENT_ENVS` / `<envs><env name value/>`：`JvmMainMethodRunConfigurationOptions.kt`
//! - `SPRING_BOOT_MAIN_CLASS` / `ACTIVE_PROFILES` / `DEBUG_MODE` / `HIDE_BANNER` / `<additionalParameters><param>`：Spring Boot 插件的字节码
//!   （`SpringBootApplicationRunConfigurationOptions`，插件不开源）。`param` 勾上的进 VM 参数：值不空是 `-D名=值`、空的是 `-D名`；
//!   `DEBUG_MODE` 是 `-Ddebug`；profile 是 `-Dspring.profiles.active=…` —— 都读 `SpringBootApplicationRunConfiguration` 的字节码确认
//! - IDEA 不写默认值：没写 = 默认。`DEBUG_MODE` / `HIDE_BANNER` / `enabled` 默认关；`ENABLE_LAUNCH_OPTIMIZATION` 默认**开**
//!   （每次起都带 `-XX:TieredStopAtLevel=1`）—— 那是 IDEA 自己的启动加速，不是你配的，不带过来
//!
//! **参数串不能原样搬进 shell。** IDEA 照自己的规则拆（`ParametersListUtil.parse`：只认双引号、`\"` 是转义、单引号是普通字符），
//! 再把每个参数里的 `${名字}` 换成环境变量（`ParametersList.expandMacros`）；我们的 VM 参数一栏是照 shell 的写法放进命令的。
//! 所以先照 IDEA 的规则拆成一个个参数，再一个个套 shell 的单引号（`maven::word`），`${名字}` 留在外面让 shell 展开。
//!
//! **路径要还原。** IDEA 存盘时把值里出现的项目目录、家目录换成 `$PROJECT_DIR$` / `$USER_HOME$`（PathMacroManager），读的时候换回来；
//! 别的 `$名字$` 是你在 IDEA 里定义的路径变量，这里没有那张表，原样留着、说一句

use crate::junit::attr;
use crate::projectcfg::RunCfg;
use std::path::{Path, PathBuf};

/// workspace.xml 超过这么大不读：几十 KB 是常态，大的是 IDEA 往里攒了别的组件（本地历史、断点），RunManager 只是其中一段
const MAX_BYTES: u64 = 16 << 20;

/// IDEA 里的一份运行配置，转好了
#[derive(Clone, Debug, PartialEq)]
pub struct IdeaRun {
    /// IDEA 里起的名字（`ShopApplication`、`ShopApplication (dev)`）
    pub name: String,
    pub main_class: String,
    /// IDEA 的模块名（一般是 artifactId）。多个服务同一个主类时拿它分
    pub module: String,
    pub cfg: RunCfg,
    /// 带不过来、或者带过来意思会变的那几处，一条一句人话
    pub notes: Vec<String>,
}

/// 这个项目里 IDEA 存的运行配置：项目根和每个 reactor 目录下的 `.idea/workspace.xml`（IDEA 的项目根不一定是你打开的那一层）。
/// 第二个是读不了的文件，一条一句
pub fn read(root: &Path) -> (Vec<IdeaRun>, Vec<String>) {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs = vec![root.to_path_buf()];
    for rel in crate::maven::reactors(root) {
        let d = root.join(rel);
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    let (mut runs, mut problems) = (Vec::new(), Vec::new());
    for dir in dirs {
        let file = dir.join(".idea").join("workspace.xml");
        let Ok(meta) = std::fs::metadata(&file) else { continue };
        if meta.len() > MAX_BYTES {
            problems.push(format!("{} 有 {} MB，超过 {} MB 没读", file.display(), meta.len() >> 20, MAX_BYTES >> 20));
            continue;
        }
        match std::fs::read(&file) {
            Ok(b) => runs.extend(parse(&String::from_utf8_lossy(&b), &dir, &home)),
            Err(e) => problems.push(format!("{} 读不了：{e}", file.display())),
        }
    }
    (runs, problems)
}

/// 一份 workspace.xml 里能带过来的运行配置。`project_dir` 是 `.idea` 所在的目录（`$PROJECT_DIR$`）。
/// 什么都带不过来的（IDEA 认出主类自动建的那种：只有主类和模块）不列 —— 那些我们自己也认得
pub fn parse(xml: &str, project_dir: &Path, home: &Path) -> Vec<IdeaRun> {
    let mut out = Vec::new();
    let Some(mgr) = elements(xml, "component").into_iter().find(|(tag, _)| attr(tag, "name").as_deref() == Some("RunManager")) else {
        return out;
    };
    for (tag, body) in elements(mgr.1, "configuration") {
        // default="true" 是模板（新建配置时的初始值），不是一份配置
        if attr(tag, "default").as_deref() == Some("true") {
            continue;
        }
        let spring = match attr(tag, "type").as_deref() {
            Some("SpringBootApplicationConfigurationType") => true,
            Some("Application") => false,
            _ => continue,
        };
        if let Some(r) = convert(tag, body, spring, &Macros { project_dir, home }) {
            out.push(r);
        }
    }
    out
}

struct Macros<'a> {
    project_dir: &'a Path,
    home: &'a Path,
}

impl Macros<'_> {
    /// `$PROJECT_DIR$` / `$USER_HOME$` 换回真路径；别的 `$名字$` 原样留着，记一句
    fn expand(&self, s: &str, notes: &mut Vec<String>) -> String {
        let s = s.replace("$PROJECT_DIR$", &self.project_dir.to_string_lossy()).replace("$USER_HOME$", &self.home.to_string_lossy());
        for m in path_macros(&s) {
            let n = format!("{m} 是 IDEA 里定义的路径变量，这里认不出，原样留着");
            if !notes.contains(&n) {
                notes.push(n);
            }
        }
        s
    }
}

/// 串里的 `$大写名字$`（IDEA 路径变量的写法）
fn path_macros(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let j = (i + 1..b.len()).find(|&j| !(b[j].is_ascii_uppercase() || b[j].is_ascii_digit() || b[j] == b'_')).unwrap_or(b.len());
            if j > i + 1 && j < b.len() && b[j] == b'$' {
                out.push(&s[i..=j]);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn convert(tag: &str, body: &str, spring: bool, m: &Macros) -> Option<IdeaRun> {
    let mut notes = Vec::new();
    // 套着的几段各有各的 <option>（`<method>` 里有 `Make`、`<param>` 里有 `name` / `value`），先摘掉，剩下的才是这份配置自己的
    let (top, sections) = cut(body, &["envs", "additionalParameters", "method", "envFilePaths", "extension"]);
    let opt = |name: &str| -> String {
        elements(&top, "option").into_iter().find(|(t, _)| attr(t, "name").as_deref() == Some(name)).and_then(|(t, _)| attr(t, "value")).unwrap_or_default()
    };
    let main_class = opt(if spring { "SPRING_BOOT_MAIN_CLASS" } else { "MAIN_CLASS_NAME" });
    if main_class.is_empty() {
        return None;
    }
    let module = elements(&top, "module").first().and_then(|(t, _)| attr(t, "name")).unwrap_or_default();

    let mut vm = idea_args(&m.expand(&opt("VM_PARAMETERS"), &mut notes));
    if spring {
        for (_, p) in sections.iter().filter(|(n, _)| *n == "additionalParameters").flat_map(|(_, b)| elements(b, "param")) {
            let o = |name: &str| elements(p, "option").into_iter().find(|(t, _)| attr(t, "name").as_deref() == Some(name)).and_then(|(t, _)| attr(t, "value"));
            let (Some(k), true) = (o("name"), o("enabled").as_deref() == Some("true")) else { continue };
            let v = m.expand(&o("value").unwrap_or_default(), &mut notes);
            vm.push(if v.is_empty() { format!("-D{k}") } else { format!("-D{k}={v}") });
        }
        if opt("DEBUG_MODE") == "true" {
            vm.push("-Ddebug".into());
        }
        if opt("HIDE_BANNER") == "true" {
            vm.push("-Dspring.main.banner-mode=OFF".into());
        }
    }
    let program_args = shell_args(&idea_args(&m.expand(&opt("PROGRAM_PARAMETERS"), &mut notes)));
    let profiles = if spring { opt("ACTIVE_PROFILES") } else { String::new() };

    let mut env = Vec::new();
    for (_, b) in sections.iter().filter(|(n, _)| *n == "envs") {
        for (t, _) in elements(b, "env") {
            let (Some(k), v) = (attr(t, "name"), attr(t, "value").unwrap_or_default()) else { continue };
            if crate::projectcfg::env_name_ok(&k) {
                env.push((k, m.expand(&v, &mut notes)));
            } else {
                notes.push(format!("环境变量「{k}」的名字放不进 shell，没带过来"));
            }
        }
    }

    let mut jdk = String::new();
    if opt("ALTERNATIVE_JRE_PATH_ENABLED") == "true" {
        let p = m.expand(&opt("ALTERNATIVE_JRE_PATH"), &mut notes);
        if p == "BUNDLED" {
            notes.push("IDEA 里选的是它自带的运行时，这里跟项目".into());
        } else if p.contains('/') {
            match crate::jdks::at(&p) {
                // 换成 JDK 列表里同一个（按真实路径）的写法：IDEA 存的可能是软链（sdkman 的 current），对话框里那一行要对得上
                Some((_, home)) => {
                    let real = |h: &Path| h.canonicalize().unwrap_or_else(|_| h.to_path_buf());
                    let listed = crate::jdks::all().into_iter().map(|(_, h)| h).find(|h| real(h) == real(&home));
                    jdk = listed.unwrap_or(home).to_string_lossy().into_owned();
                }
                None => notes.push(format!("IDEA 里选的 JDK {p} 不在了、或者不是 JDK，这里跟项目")),
            }
        } else if !p.is_empty() {
            // IDEA 自己的 JDK 表（SDKs）里起的名字，`JavaParametersUtil.createAlternativeJdk` 先按名字查那张表、查不到才当路径
            notes.push(format!("IDEA 里选的 JDK「{p}」是 IDEA 自己起的名字，这里认不出，跟项目"));
        }
    }

    let wd = opt("WORKING_DIRECTORY");
    if !wd.is_empty() && wd != "$MODULE_WORKING_DIR$" && wd != "$MODULE_DIR$" {
        let wd = m.expand(&wd, &mut notes);
        notes.push(format!("IDEA 里工作目录是 {wd}，这里照旧在模块目录下跑"));
    }
    if opt("PASS_PARENT_ENVS") == "false" {
        notes.push("IDEA 里关了「带上系统的环境变量」，这里照旧带".into());
    }
    if sections.iter().any(|(n, _)| *n == "envFilePaths") {
        notes.push("IDEA 里配的 .env 文件没读，要的变量请加在环境变量里".into());
    }

    let cfg = RunCfg { profiles, vm_options: shell_args(&vm), program_args, env, jdk };
    if cfg.is_empty() && notes.is_empty() {
        return None;
    }
    Some(IdeaRun { name: attr(tag, "name").unwrap_or_else(|| main_class.clone()), main_class, module, cfg, notes })
}

/// 照 IDEA 的 `ParametersListUtil.parse(s)`（不留引号、不认单引号）拆成参数：双引号把空白圈进来、自己去掉；`\"` 是一个字面的引号。
/// 空白按 Java 的 `Character.isWhitespace`：和 Rust 的 `is_whitespace` 差三个不断行空格
fn idea_args(s: &str) -> Vec<String> {
    let ws = |c: char| c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}');
    let chars: Vec<char> = s.trim_matches(ws).chars().collect();
    let mut out = Vec::new();
    let mut tok = String::new();
    let (mut in_quotes, mut escaped, mut non_empty) = (false, false, false);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        i += 1;
        if ch == '"' {
            if !escaped {
                in_quotes = !in_quotes;
                non_empty = true;
                continue;
            }
            escaped = false;
        } else if ws(ch) {
            if !in_quotes {
                if !tok.is_empty() || non_empty {
                    out.push(std::mem::take(&mut tok));
                    non_empty = false;
                }
                continue;
            }
        } else if ch == '\\' && chars.get(i) == Some(&'"') {
            escaped = true;
            continue;
        }
        tok.push(ch);
    }
    if !tok.is_empty() || non_empty {
        out.push(tok);
    }
    out
}

/// 拆好的参数 → shell 里的一串词：每个参数自成一个词（`maven::word` 套单引号），`${名字}` 留在引号外面让 shell 展开 ——
/// IDEA 也会展开它（先查 IDEA 的路径变量、再查环境变量，不分大小写；shell 只认环境变量、分大小写，查不到是空的）
fn shell_args(args: &[String]) -> String {
    args.iter().map(|a| shell_arg(a)).collect::<Vec<_>>().join(" ")
}

fn shell_arg(a: &str) -> String {
    let mut out = String::new();
    let mut lit = String::new();
    let mut rest = a;
    while let Some(i) = rest.find("${") {
        let name = rest[i + 2..].find('}').map(|j| &rest[i + 2..i + 2 + j]).filter(|n| crate::projectcfg::env_name_ok(n));
        match name {
            Some(n) => {
                lit.push_str(&rest[..i]);
                if !lit.is_empty() {
                    out.push_str(&crate::maven::word(&std::mem::take(&mut lit)));
                }
                out.push_str(&format!("\"${{{n}}}\""));
                rest = &rest[i + 3 + n.len()..];
            }
            None => {
                lit.push_str(&rest[..i + 2]);
                rest = &rest[i + 2..];
            }
        }
    }
    lit.push_str(rest);
    if !lit.is_empty() || out.is_empty() {
        out.push_str(&crate::maven::word(&lit));
    }
    out
}

/// `<name …>…</name>` 和 `<name …/>`：每处的（开始标签, 正文）。同名的不嵌套（RunManager 里的 configuration、option、env 都是）
fn elements<'a>(s: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = s[from..].find(&open) {
        let at = from + i;
        let after = at + open.len();
        // `<option` 不能认成 `<options`
        if !s[after..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            from = after;
            continue;
        }
        let Some(end) = s[at..].find('>').map(|e| at + e) else { break };
        let tag = &s[at..=end];
        if tag.ends_with("/>") {
            out.push((tag, ""));
            from = end + 1;
        } else {
            let body_end = s[end + 1..].find(&close).map(|e| end + 1 + e).unwrap_or(s.len());
            out.push((tag, &s[end + 1..body_end]));
            from = (body_end + close.len()).min(s.len());
        }
    }
    out
}

/// 把这几种套着的段落从正文里摘出来：返回（剩下的, [(段名, 段的正文)]）
fn cut<'a>(body: &'a str, names: &[&'static str]) -> (String, Vec<(&'static str, &'a str)>) {
    let mut rest = body.to_string();
    let mut sections = Vec::new();
    for &n in names {
        for (tag, inner) in elements(body, n) {
            sections.push((n, inner));
            let whole = if tag.ends_with("/>") { tag.to_string() } else { format!("{tag}{inner}</{n}>") };
            rest = rest.replacen(&whole, "", 1);
        }
    }
    (rest, sections)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::junit::unescape;

    fn p(xml: &str) -> Vec<IdeaRun> {
        parse(xml, Path::new("/w/shop"), Path::new("/Users/me"))
    }

    fn wrap(configs: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ChangeListManager"><option name="VM_PARAMETERS" value="不是这里的" /></component>
  <component name="RunManager" selected="Spring Boot.ShopApplication">
{configs}
    <list><item itemvalue="Spring Boot.ShopApplication" /></list>
  </component>
</project>"#
        )
    }

    /// 照 IDEA 2026.1 存出来的样子（字段名、哪些默认值不写）：你机器上那 11 份都是这种 —— 什么都没配，不列
    #[test]
    fn 自动建的只有主类和模块_不列() {
        let x = wrap(
            r#"    <configuration name="ShopApplication" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot" nameIsGenerated="true">
      <module name="shop-web" />
      <option name="SPRING_BOOT_MAIN_CLASS" value="com.shop.ShopApplication" />
      <method v="2"><option name="Make" enabled="true" /></method>
    </configuration>"#,
        );
        assert_eq!(p(&x), vec![]);
    }

    #[test]
    fn 配了的都带过来_参数照idea的规则拆再照shell的规则包() {
        let x = wrap(
            r#"    <configuration name="ShopApplication (dev)" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot">
      <option name="ACTIVE_PROFILES" value="dev,local" />
      <option name="DEBUG_MODE" value="true" />
      <additionalParameters>
        <param><option name="enabled" value="true" /><option name="name" value="server.port" /><option name="value" value="8081" /></param>
        <param><option name="name" value="没勾的" /><option name="value" value="x" /></param>
        <param><option name="enabled" value="true" /><option name="name" value="flag" /></param>
      </additionalParameters>
      <envs>
        <env name="DB_URL" value="jdbc:mysql://h/db?a=1&amp;b=2" />
        <env name="bad-name" value="x" />
      </envs>
      <module name="shop-web" />
      <option name="PROGRAM_PARAMETERS" value="--name=&quot;a b&quot; it's" />
      <option name="SPRING_BOOT_MAIN_CLASS" value="com.shop.ShopApplication" />
      <option name="VM_PARAMETERS" value="-Xmx512m -Dmsg=&quot;hello world&quot; -Dq=\&quot;x\&quot; -Dlog=$PROJECT_DIR$/logs -Dh=${HOME}/x -Dcost=$5" />
      <method v="2"><option name="Make" enabled="true" /></method>
    </configuration>"#,
        );
        let r = p(&x);
        assert_eq!(r.len(), 1, "{r:?}");
        let r = &r[0];
        assert_eq!((r.name.as_str(), r.main_class.as_str(), r.module.as_str()), ("ShopApplication (dev)", "com.shop.ShopApplication", "shop-web"));
        assert_eq!(r.cfg.profiles, "dev,local");
        // 双引号圈进来的空白是参数的一部分、引号去掉；\" 是字面引号；$5 不是变量（shell 里会被吃掉，所以要套单引号）
        assert_eq!(
            r.cfg.vm_options,
            r#"-Xmx512m '-Dmsg=hello world' '-Dq="x"' -Dlog=/w/shop/logs -Dh="${HOME}"/x '-Dcost=$5' -Dserver.port=8081 -Dflag -Ddebug"#
        );
        // IDEA 里单引号是普通字符：it's 是一个参数
        assert_eq!(r.cfg.program_args, r#"'--name=a b' 'it'\''s'"#);
        assert_eq!(r.cfg.env, vec![("DB_URL".to_string(), "jdbc:mysql://h/db?a=1&b=2".to_string())]);
        assert_eq!(r.notes, vec!["环境变量「bad-name」的名字放不进 shell，没带过来".to_string()]);
    }

    #[test]
    fn 模板和别的类型不算_application类型认main_class_name() {
        let x = wrap(
            r#"    <configuration default="true" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot">
      <option name="SPRING_BOOT_MAIN_CLASS" value="com.shop.ShopApplication" />
      <option name="VM_PARAMETERS" value="-Xmx1g" />
    </configuration>
    <configuration name="test" type="JUnit" factoryName="JUnit"><option name="VM_PARAMETERS" value="-ea" /></configuration>
    <configuration name="Shop (community)" type="Application" factoryName="Application">
      <option name="MAIN_CLASS_NAME" value="com.shop.ShopApplication" />
      <module name="shop-web" />
      <option name="VM_PARAMETERS" value="-Dspring.profiles.active=dev" />
    </configuration>"#,
        );
        let r = p(&x);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!((r[0].name.as_str(), r[0].main_class.as_str(), r[0].cfg.vm_options.as_str()), ("Shop (community)", "com.shop.ShopApplication", "-Dspring.profiles.active=dev"));
    }

    #[test]
    fn jdk_名字认不出_路径不在_工作目录_路径变量_都说一句() {
        let x = wrap(
            r#"    <configuration name="A" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot">
      <option name="ALTERNATIVE_JRE_PATH" value="corretto-17" />
      <option name="ALTERNATIVE_JRE_PATH_ENABLED" value="true" />
      <option name="SPRING_BOOT_MAIN_CLASS" value="a.A" />
      <option name="WORKING_DIRECTORY" value="$USER_HOME$/tmp" />
      <option name="VM_PARAMETERS" value="-Drepo=$MAVEN_REPOSITORY$/x" />
    </configuration>
    <configuration name="B" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot">
      <option name="ALTERNATIVE_JRE_PATH" value="/no/such/jdk" />
      <option name="ALTERNATIVE_JRE_PATH_ENABLED" value="true" />
      <option name="SPRING_BOOT_MAIN_CLASS" value="b.B" />
      <option name="WORKING_DIRECTORY" value="$MODULE_WORKING_DIR$" />
    </configuration>
    <configuration name="C" type="SpringBootApplicationConfigurationType" factoryName="Spring Boot">
      <option name="ALTERNATIVE_JRE_PATH" value="corretto-17" />
      <option name="SPRING_BOOT_MAIN_CLASS" value="c.C" />
    </configuration>"#,
        );
        let r = p(&x);
        assert_eq!(r.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["A", "B"], "C 没勾「用别的 JDK」，什么都没配：{r:?}");
        assert_eq!(
            r[0].notes,
            [
                "$MAVEN_REPOSITORY$ 是 IDEA 里定义的路径变量，这里认不出，原样留着",
                "IDEA 里选的 JDK「corretto-17」是 IDEA 自己起的名字，这里认不出，跟项目",
                "IDEA 里工作目录是 /Users/me/tmp，这里照旧在模块目录下跑",
            ]
        );
        assert_eq!(r[0].cfg.vm_options, "'-Drepo=$MAVEN_REPOSITORY$/x'");
        assert_eq!(r[1].notes, ["IDEA 里选的 JDK /no/such/jdk 不在了、或者不是 JDK，这里跟项目"]);
        assert_eq!(r[1].cfg.jdk, "");
    }

    #[test]
    fn idea的拆法_和shell不一样的那几处() {
        let a = |s: &str| idea_args(s);
        assert_eq!(a(r#"  -a   "b c"  "#), ["-a", "b c"]);
        assert_eq!(a(r#"-D"x y"z"#), ["-Dx yz"], "引号可以在参数中间");
        assert_eq!(a(r#"'a b'"#), ["'a", "b'"], "单引号不圈空白");
        assert_eq!(a(r#""""#), [""], "一对空引号是一个空参数");
        assert_eq!(a(r#"a\"b \x"#), [r#"a"b"#, r"\x"], "反斜杠只转义引号");
        assert_eq!(a(""), Vec::<String>::new());
    }

    #[test]
    fn 变量留给shell展开_别的都套引号() {
        assert_eq!(shell_arg("-Dh=${HOME}/a b"), r#"-Dh="${HOME}"'/a b'"#);
        assert_eq!(shell_arg("${A}${B}"), r#""${A}""${B}""#);
        assert_eq!(shell_arg("${not valid}"), "'${not valid}'");
        assert_eq!(shell_arg("$(rm -rf ~)"), "'$(rm -rf ~)'");
        assert_eq!(shell_arg(""), "''");
    }

    #[test]
    fn 标签名是前缀的不算() {
        let e = elements(r#"<options x="1"/><option name="a" value="b"/>"#, "option");
        assert_eq!(e.len(), 1);
        assert_eq!(attr(e[0].0, "name").as_deref(), Some("a"));
        assert_eq!(unescape("&quot;"), "\"");
    }
}
