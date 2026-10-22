//! 跑单测的结果（docs/IDEA.md 第 17 节）：读 surefire 写的 `target/surefire-reports/TEST-*.xml`，出结果树。
//!
//! **不靠语言服务器**（第 2 节第 10 条定的）：测试就是 `mvn -Dtest=类#方法 test`（`maven::test_command`），跑在任务里 ——
//! 运行窗、⌃R 重跑、⌘F2 停、同名先停旧的全是现成的；结果是 surefire 自己写的报告，和 CI 上看到的同一份。
//!
//! 报告是 JUnit 的 XML 格式（surefire、Gradle、CI 都认的那种），结构只有三层：`testsuite` → `testcase` → 失败时一个 `failure` / `error` / `skipped`。
//! 不为它加 XML 依赖（pom 那边也是自己读的，`maven.rs`）：只认这几个标签，属性值和正文按 XML 的五个实体和 `&#N;` 解码，正文可能包在 CDATA 里。

use std::path::Path;
use std::time::SystemTime;

/// 失败原因（堆栈）最多留多少字：一个 Spring 上下文起不来的堆栈能有几百 KB，界面上只看得下开头
pub const DETAIL_MAX: usize = 16_000;
/// 一次最多列多少条结果（整个模块几千条测试时，列表再长也没人看；数字照实报）
pub const CASES_MAX: usize = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    /// 断言没过（`failure`）
    Failed,
    /// 测试自己抛了异常（`error`）—— IDEA 也分这两种，颜色一样、图标不同
    Error,
    Skipped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    /// 类的全名（`classname`）
    pub class: String,
    pub name: String,
    pub ms: u64,
    pub outcome: Outcome,
    /// 一句话的原因（`message` 属性）
    pub message: String,
    /// 堆栈（标签正文），截在 [`DETAIL_MAX`] 字
    pub detail: String,
}

/// XML 的五个实体和数字实体 → 字符。不认的 `&xxx;` 原样留着（surefire 不会写别的）。`ideacfg` 读 IDEA 的 workspace.xml 也用它
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let ch = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 一个开始标签里某个属性的值（`name="…"` 或 `name='…'`），已解码。按「前面是空白」认属性名，免得 `classname` 被当成 `name`
pub(crate) fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = tag[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        if !tag[..at].ends_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let rest = tag[at + name.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let q = rest.chars().next()?;
        if q != '"' && q != '\'' {
            continue;
        }
        let body = &rest[1..];
        return body.find(q).map(|e| unescape(&body[..e]));
    }
    None
}

/// 标签正文：CDATA 原样、别的按实体解码；截在 [`DETAIL_MAX`] 字（切在字符边界上）
fn text_of(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while !rest.is_empty() {
        match rest.find("<![CDATA[") {
            Some(i) => {
                out.push_str(&unescape(&rest[..i]));
                let after = &rest[i + 9..];
                let end = after.find("]]>").unwrap_or(after.len());
                out.push_str(&after[..end]);
                rest = after.get(end + 3..).unwrap_or("");
            }
            None => {
                out.push_str(&unescape(rest));
                break;
            }
        }
    }
    let out = out.trim().to_string();
    match out.char_indices().nth(DETAIL_MAX) {
        Some((i, _)) => format!("{}…（截掉了后面 {} 字）", &out[..i], out[i..].chars().count()),
        None => out,
    }
}

/// 一份报告里的全部用例，按报告里的次序
pub fn parse_report(xml: &str) -> Vec<Case> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<testcase") {
        let after = &rest[i..];
        let Some(gt) = after.find('>') else { break };
        let open = &after[..gt];
        let self_closing = open.ends_with('/');
        let (body, next) = if self_closing {
            ("", &after[gt + 1..])
        } else {
            match after[gt + 1..].find("</testcase>") {
                Some(e) => (&after[gt + 1..gt + 1 + e], &after[gt + 1 + e + 11..]),
                None => (&after[gt + 1..], ""),
            }
        };
        let secs: f64 = attr(open, "time").and_then(|t| t.replace(',', "").parse().ok()).unwrap_or(0.0);
        let mut case = Case {
            class: attr(open, "classname").unwrap_or_default(),
            name: attr(open, "name").unwrap_or_default(),
            ms: (secs * 1000.0).round() as u64,
            outcome: Outcome::Passed,
            message: String::new(),
            detail: String::new(),
        };
        for (tag, outcome) in [("failure", Outcome::Failed), ("error", Outcome::Error), ("skipped", Outcome::Skipped)] {
            let Some(j) = body.find(&format!("<{tag}")) else { continue };
            // `<failure` 后面紧跟的必须是空白、`>` 或 `/`：别把 `<failures>` 之类当成它
            if !body[j + tag.len() + 1..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
                continue;
            }
            let t = &body[j..];
            let tgt = t.find('>').unwrap_or(t.len());
            let topen = &t[..tgt];
            case.outcome = outcome;
            case.message = attr(topen, "message").unwrap_or_default();
            if !topen.ends_with('/') {
                let inner = &t[(tgt + 1).min(t.len())..];
                let end = inner.find(&format!("</{tag}>")).unwrap_or(inner.len());
                case.detail = text_of(&inner[..end]);
            }
            break;
        }
        out.push(case);
        rest = next;
    }
    out
}

/// 一个模块这次跑出来的结果：`target/surefire-reports/TEST-*.xml` 里**改动时间不早于 `since`** 的那些（旧报告不算 —— 上次跑过、这次没跑到的类
/// 还躺在那儿，算进来就是一份看着完整、其实混着上次结果的东西）。返回 `(前 CASES_MAX 条, 真条数)`
pub fn results(module_dir: &Path, since: SystemTime) -> (Vec<Case>, usize) {
    let dir = module_dir.join("target/surefire-reports");
    let Ok(rd) = std::fs::read_dir(&dir) else { return (Vec::new(), 0) };
    let mut files: Vec<_> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("TEST-") && n.ends_with(".xml") && e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= since)
        })
        .map(|e| e.path())
        .collect();
    files.sort();
    let mut all = Vec::new();
    for f in files {
        if let Ok(t) = std::fs::read_to_string(&f) {
            all.extend(parse_report(&t));
        }
    }
    let total = all.len();
    all.truncate(CASES_MAX);
    (all, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    // surefire 3.5 实际写出来的样子（一个通过、一个断言失败、一个异常、一个跳过；失败的正文是实体编码的，异常的是 CDATA）
    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" name="com.demo.OrderTest" time="0.052" tests="4" errors="1" skipped="1" failures="1">
  <properties><property name="java.version" value="21"/></properties>
  <testcase name="adds" classname="com.demo.OrderTest" time="0.011"/>
  <testcase name="totals" classname="com.demo.OrderTest" time="1,234.5">
    <failure message="expected: &lt;3&gt; but was: &lt;2&gt;" type="org.opentest4j.AssertionFailedError">org.opentest4j.AssertionFailedError: expected: &lt;3&gt; but was: &lt;2&gt;
	at com.demo.OrderTest.totals(OrderTest.java:21)</failure>
    <system-out><![CDATA[打印的东西]]></system-out>
  </testcase>
  <testcase name="boom" classname="com.demo.OrderTest" time="0.002">
    <error message="甲 &amp; 乙" type="java.lang.IllegalStateException"><![CDATA[java.lang.IllegalStateException: 甲 & 乙 <原样>
	at com.demo.OrderTest.boom(OrderTest.java:27)]]></error>
  </testcase>
  <testcase name="later" classname="com.demo.OrderTest" time="0">
    <skipped message="还没写完"/>
  </testcase>
</testsuite>"#;

    #[test]
    fn 报告_四种结果_原因和堆栈解码_classname_不当成_name() {
        let c = parse_report(REPORT);
        assert_eq!(c.iter().map(|x| (x.name.as_str(), x.outcome)).collect::<Vec<_>>(), vec![
            ("adds", Outcome::Passed),
            ("totals", Outcome::Failed),
            ("boom", Outcome::Error),
            ("later", Outcome::Skipped),
        ]);
        assert!(c.iter().all(|x| x.class == "com.demo.OrderTest"), "classname 和 name 分得开");
        assert_eq!(c[0].ms, 11);
        assert_eq!(c[1].ms, 1_234_500, "时间里的千分位逗号（有的 locale 下 surefire 会写）");
        assert_eq!(c[1].message, "expected: <3> but was: <2>");
        assert!(c[1].detail.starts_with("org.opentest4j.AssertionFailedError: expected: <3>") && c[1].detail.contains("OrderTest.java:21"), "{}", c[1].detail);
        assert_eq!(c[2].message, "甲 & 乙");
        assert!(c[2].detail.contains("甲 & 乙 <原样>"), "CDATA 原样：{}", c[2].detail);
        assert_eq!((c[3].message.as_str(), c[3].detail.as_str()), ("还没写完", ""));
    }

    #[test]
    fn 报告_classname_写在前面也分得开_cdata_里的实体字样原样() {
        // Gradle 这类写的是 classname 在前：按字符串找第一个 "name" 会落在 classname 里
        let xml = r#"<testcase classname="com.demo.A" name="b" time="0.001"><failure message="m"><![CDATA[期望 &lt;b&gt; 是 HTML 原文]]></failure></testcase>"#;
        let c = &parse_report(xml)[0];
        assert_eq!((c.class.as_str(), c.name.as_str()), ("com.demo.A", "b"));
        assert_eq!(c.detail, "期望 &lt;b&gt; 是 HTML 原文", "CDATA 里不解码：那是断言里比对的原文");
    }

    #[test]
    fn 报告_堆栈太长截断_说出截了多少() {
        let long = "x".repeat(DETAIL_MAX + 7);
        let xml = format!(r#"<testcase name="a" classname="C"><error message="m"><![CDATA[{long}]]></error></testcase>"#);
        let d = &parse_report(&xml)[0].detail;
        assert!(d.starts_with(&"x".repeat(DETAIL_MAX)) && d.ends_with("…（截掉了后面 7 字）"), "{}", &d[d.len() - 40..]);
    }

    #[test]
    fn 结果_只认这次跑之后写的报告() {
        let d = std::env::temp_dir().join(format!("junit-{}", std::process::id()));
        let reports = d.join("target/surefire-reports");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(reports.join("TEST-com.demo.Old.xml"), r#"<testcase name="stale" classname="com.demo.Old"/>"#).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let since = SystemTime::now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(reports.join("TEST-com.demo.New.xml"), r#"<testcase name="fresh" classname="com.demo.New"/>"#).unwrap();
        std::fs::write(reports.join("com.demo.New.txt"), "surefire 的文字报告不读").unwrap();
        let (cases, total) = results(&d, since);
        assert_eq!((cases.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), total), (vec!["fresh"], 1), "上次的报告不算");
        let _ = std::fs::remove_dir_all(&d);
    }
}
