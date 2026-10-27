//! 覆盖率（docs/TAKEOVER.md G15，IDEA 的 Run with Coverage）：带 JaCoCo 的代理跑单测，跑完出报告，哪几行跑到了、哪几行没跑到。
//!
//! 跑：任务名是「模块 [cover 选择器]」（和「模块 [test 选择器]」同一个形状，命令一样），起的时候在 surefire fork 的测试 JVM 上挂 JaCoCo 的代理。
//! **不走 `-DargLine`**：pom 里给 surefire 配了 `<argLine>` 的项目（Spring 项目常见，加 `-Xmx`、`--add-opens`），命令行上的 `-DargLine` 会被盖掉、
//! 覆盖率悄悄没有。借的是调试单测那条路（`debug::with_surefire_debug`）：`maven.surefire.debug` 的值 surefire 原样加在 fork 的 JVM 参数后面，不管 argLine。
//! 它顺带关掉 surefire 的 fork 超时 —— 调试时就是这样，单测这么跑没有别的差别。
//!
//! 报告：JaCoCo 的命令行（`report … --xml`）读执行记录 + 编译出来的类 + 源码，出 XML；这里只认 `<package>` → `<sourcefile>` → `<line nr mi ci mb cb>`。
//! 一行的三种：有指令跑到、没有分支漏掉 = 跑到了；一条指令都没跑到 = 没跑到；跑到了但有指令或分支没跑到 = 跑到一部分（IDEA 的黄）。
//!
//! 两个 jar 内置在 `.app` 里（`scripts/java-bundle.sh`，钉版本、核 sha256）；开发构建没有就照实说。执行记录和报告放在应用的缓存目录，不往项目里写

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 报告最多列多少个文件（一个大模块几千个类；列表再长也没人看，数字照实报）
pub const FILES_MAX: usize = 2_000;
/// 报告的 XML 最多读多大
const XML_CAP: usize = 64 << 20;

/// 一个源文件的覆盖情况（行从 1 数）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileCov {
    /// 绝对路径；找不到源文件的（生成的代码）不列
    pub path: String,
    pub covered: Vec<u32>,
    pub partial: Vec<u32>,
    pub missed: Vec<u32>,
}

/// 「模块 [cover 选择器]」是不是带覆盖率跑的单测
pub fn is_cover_run(name: &str) -> bool {
    name.strip_suffix(']').and_then(|n| n.rsplit_once(" [")).is_some_and(|(_, g)| g.starts_with("cover "))
}

/// 在 surefire fork 的测试 JVM 上挂代理。路径带空格 / 引号的不挂（surefire 按空格拆参数，拆开了 JVM 起不来）—— 返回 None，调用方说为什么
pub fn with_agent(command: &str, agent: &Path, exec: &Path) -> Option<String> {
    let (a, e) = (agent.to_str()?, exec.to_str()?);
    if [a, e].iter().any(|s| s.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == ',' || c == '=')) {
        return None;
    }
    let at = command.find(" -B -ntp")?;
    Some(format!("{} '-Dmaven.surefire.debug=-javaagent:{a}=destfile={e},append=false' {}", &command[..at], &command[at + 1..]))
}

/// 这次运行的执行记录放哪：应用缓存目录里按「项目 + 任务名」一份（同一个跑第二次就盖掉上一次的）
pub fn exec_path(cache: &Path, root: &str, name: &str) -> PathBuf {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{root}\n{name}").bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    cache.join("coverage").join(format!("{h:016x}.exec"))
}

/// 一个 `<tag a="1" b="2">` 里某个属性的值
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(&tag[i..j])
}

/// JaCoCo 的 XML 报告 → 每个源文件（包路径/文件名）一份。`resolve` 把「包路径/文件名」变成盘上的绝对路径，找不到的丢掉（生成的代码）。
/// 不为它引 XML 库（同 `junit.rs`）：只认这三个标签，名字里不会有实体（Java 的包名、文件名）
pub fn parse_xml(xml: &str, mut resolve: impl FnMut(&str) -> Option<String>) -> (Vec<FileCov>, usize) {
    let mut out: Vec<FileCov> = Vec::new();
    let mut pkg = "";
    let mut cur: Option<FileCov> = None;
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        let end = rest.find('>').map_or(rest.len(), |e| e + 1);
        let tag = &rest[..end];
        rest = &rest[end..];
        if tag.starts_with("<package ") {
            pkg = attr(tag, "name").unwrap_or("");
        } else if tag.starts_with("<sourcefile ") {
            let file = attr(tag, "name").unwrap_or("");
            let rel = if pkg.is_empty() { file.to_string() } else { format!("{pkg}/{file}") };
            cur = resolve(&rel).map(|path| FileCov { path, ..Default::default() });
        } else if tag.starts_with("</sourcefile") {
            if let Some(f) = cur.take().filter(|f| !(f.covered.is_empty() && f.partial.is_empty() && f.missed.is_empty())) {
                out.push(f);
            }
        } else if tag.starts_with("<line ") {
            let Some(f) = cur.as_mut() else { continue };
            let n = |k| attr(tag, k).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
            let (nr, mi, ci, mb) = (n("nr"), n("mi"), n("ci"), n("mb"));
            if nr == 0 {
                continue;
            }
            match (ci > 0, mi > 0 || mb > 0) {
                (true, false) => f.covered.push(nr),
                (true, true) => f.partial.push(nr),
                (false, _) => f.missed.push(nr),
            }
        }
    }
    let total = out.len();
    // 没跑到的多的排前面（IDEA 的覆盖率窗口默认也是把最该补测试的放上面），一样多的按路径
    out.sort_by(|a, b| pct(a).total_cmp(&pct(b)).then_with(|| a.path.cmp(&b.path)));
    out.truncate(FILES_MAX);
    (out, total)
}

/// 行覆盖率（跑到一部分的算跑到），0..=1
pub fn pct(f: &FileCov) -> f64 {
    let all = f.covered.len() + f.partial.len() + f.missed.len();
    if all == 0 { 1.0 } else { (f.covered.len() + f.partial.len()) as f64 / all as f64 }
}

/// 出报告：起 JaCoCo 的命令行（内置的 Java 21 跑），读它写的 XML。`modules`：（类目录, 源码目录）—— 这次运行编出来的那几个模块都给，
/// 测试跑到了兄弟模块里的代码也算（IDEA 也是整个项目算）
pub fn report(java: &Path, cli: &Path, exec: &Path, out_xml: &Path, modules: &[(PathBuf, PathBuf)]) -> Result<(Vec<FileCov>, usize), String> {
    if !exec.is_file() {
        return Err("未记录到覆盖率（可能没有测试被执行，请查看输出）".into());
    }
    let mut cmd = std::process::Command::new(java);
    cmd.arg("-jar").arg(cli).arg("report").arg(exec);
    let mut any = false;
    for (classes, src) in modules {
        if classes.is_dir() {
            cmd.arg("--classfiles").arg(classes);
            any = true;
        }
        if src.is_dir() {
            cmd.arg("--sourcefiles").arg(src);
        }
    }
    if !any {
        return Err("未找到编译后的类（target/classes 或 build/classes/java/main），无法生成报告".into());
    }
    cmd.arg("--xml").arg(out_xml).arg("--quiet");
    // `--quiet` 之后 stdout 没东西；出错的话话在 stderr，两边都设闸（rust.md「子进程输出必须设闸」）
    let out = procutil::run_capped(&mut cmd, 64 << 10, 64 << 10, procutil::Overflow::Drain).map_err(|e| format!("无法启动 JaCoCo 命令行：{e}"))?;
    if !out.status.success() {
        return Err(format!("JaCoCo 生成报告失败：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let bytes = std::fs::read(out_xml).map_err(|e| format!("无法读取报告：{e}"))?;
    if bytes.len() > XML_CAP {
        return Err(format!("报告大小为 {} MB，超出上限，未读取", bytes.len() >> 20));
    }
    let xml = String::from_utf8_lossy(&bytes);
    let srcs: Vec<&PathBuf> = modules.iter().map(|(_, s)| s).collect();
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    Ok(parse_xml(&xml, |rel| {
        seen.entry(rel.to_string())
            .or_insert_with(|| srcs.iter().map(|s| s.join(rel)).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned()))
            .clone()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // JaCoCo 0.8 写出来的样子（删掉了 class / method / counter 那些不读的）
    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd"><report name="JaCoCo Coverage Report"><sessioninfo id="x" start="1" dump="2"/><package name="com/demo/order"><class name="com/demo/order/Calc" sourcefilename="Calc.java"><method name="add" desc="(II)I" line="4"><counter type="LINE" missed="0" covered="1"/></method></class><sourcefile name="Calc.java"><line nr="3" mi="3" ci="0" mb="0" cb="0"/><line nr="4" mi="0" ci="4" mb="0" cb="0"/><line nr="6" mi="0" ci="2" mb="1" cb="1"/><counter type="LINE" missed="1" covered="2"/></sourcefile><sourcefile name="Gen.java"><line nr="1" mi="0" ci="1" mb="0" cb="0"/></sourcefile></package><package name="com/demo"><sourcefile name="All.java"><line nr="2" mi="0" ci="5" mb="0" cb="0"/></sourcefile></package></report>"#;

    #[test]
    fn 报告_三种行_找不到源文件的丢掉_没跑到多的排前面() {
        let (files, total) = parse_xml(XML, |rel| (rel != "com/demo/order/Gen.java").then(|| format!("/p/src/{rel}")));
        assert_eq!(total, 2, "Gen.java 找不到源文件（生成的代码），不列");
        assert_eq!(files[0].path, "/p/src/com/demo/order/Calc.java", "覆盖率低的排前面");
        assert_eq!((files[0].covered.clone(), files[0].partial.clone(), files[0].missed.clone()), (vec![4], vec![6], vec![3]));
        assert_eq!(files[1].path, "/p/src/com/demo/All.java");
        assert!((pct(&files[0]) - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn 挂代理_借_surefire_debug_那条路_带空格的路径不挂() {
        let cmd = "mvn -B -ntp -Dtest=A -pl order -am test";
        assert_eq!(
            with_agent(cmd, Path::new("/R/jacocoagent.jar"), Path::new("/C/x.exec")).unwrap(),
            "mvn '-Dmaven.surefire.debug=-javaagent:/R/jacocoagent.jar=destfile=/C/x.exec,append=false' -B -ntp -Dtest=A -pl order -am test"
        );
        assert!(with_agent(cmd, Path::new("/Applications/My Apps/a.jar"), Path::new("/C/x.exec")).is_none());
        assert!(with_agent("gradle test", Path::new("/a.jar"), Path::new("/x.exec")).is_none(), "不是我们拼的 mvn 命令不挂");
    }

    #[test]
    fn 认任务名() {
        assert!(is_cover_run("order [cover com.demo.CalcTest]"));
        assert!(!is_cover_run("order [test com.demo.CalcTest]"));
        assert!(!is_cover_run("cover"));
    }
}
