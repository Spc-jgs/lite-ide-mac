//! Java 代码风格照 IDEA 来（docs/TAKEOVER.md G22）：⌥⌘L 排出来的续行缩进、行宽，自动 import 和 ⌃⌥O 的 import 次序、什么时候合成 `.*`，
//! 和你在 IDEA 里一样。**不另设一套**：从 IDEA 自己存的地方读，读不到用 IDEA 的默认值。
//!
//! 照 IDEA 选方案的规矩找（`.idea/codeStyles/codeStyleConfig.xml` 的两个选项）：
//! 1. 项目用自己的方案（`USE_PER_PROJECT_SETTINGS` = true）→ `.idea/codeStyles/Project.xml`，团队一般把它提交进仓库
//! 2. 不然用 IDE 级的方案：项目指定的（`PREFERRED_PROJECT_CODE_STYLE`），没指定就是 IDEA 当前选的（`options/code.style.schemes.xml` 的
//!    `CURRENT_SCHEME_NAME`，没写是 `Default`）→ `~/Library/Application Support/JetBrains/IntelliJIdea<版本>/codestyles/<名字>.xml`
//! 3. 都没有（没装过 IDEA、或者从没改过默认方案）→ IDEA 的默认值
//!
//! **IDEA 只写和默认不一样的项**，所以读到的每一项都叠在默认值上。默认值读 IDEA 2026.1 的字节码确认的（`javap`，只解析不执行）：
//! 缩进 4、续行 8、Tab 4、不用 Tab 字符（`CommonCodeStyleSettings$IndentOptions`）、行宽 120（`CodeStyleSettings.RIGHT_MARGIN`）；
//! import 次序「模块 import、其余、空行、javax、java、空行、static 其余」（`JavaCodeStyleSettings.initImportLayout`）；
//! 同一个包 5 个类合成 `.*`、static 3 个（`CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND` / `NAMES_COUNT_TO_USE_IMPORT_ON_DEMAND`）。
//! 这台机器上 IDEA 的 Default 方案正好改过这两个阈值 —— 照 IDEA 的默认值来的话，⌃⌥O 会合出你在 IDEA 里从没见过的 `import x.*`。
//!
//! 能转的只有 jdtls（Eclipse 的格式化器）有对应的那几项；别的（换行、空格、大括号放哪）说出来没转，不装作转了。

use crate::ideacfg::{cut, elements};
use crate::junit::attr;
use std::path::{Path, PathBuf};

/// import 次序里的一项（IDEA 的 `PackageEntryTable`：`<package name static withSubpackages module/>` 和 `<emptyLine/>`）
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// 包名前缀；`""` = 其余所有
    Pkg { name: String, statik: bool },
    Blank,
    /// Java 23 的 `import module`（jdtls 不认，转的时候跳过）
    Module,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub indent: u32,
    pub cont: u32,
    pub tab: u32,
    pub tabs: bool,
    pub margin: u32,
    pub imports: Vec<Entry>,
    /// 同一个包几个类合成 `.*`（static 是同一个类几个成员）
    pub star: u32,
    pub static_star: u32,
    /// 从哪来的，一句人话（设置页里给你看）
    pub origin: String,
    /// 读到了、没转的（jdtls 没有对应的）：选项名
    pub skipped: Vec<String>,
}

/// IDEA 的默认值（理由和出处在文件头）
pub fn idea_default() -> Style {
    let pkg = |n: &str, s: bool| Entry::Pkg { name: n.to_string(), statik: s };
    Style {
        indent: 4,
        cont: 8,
        tab: 4,
        tabs: false,
        margin: 120,
        imports: vec![Entry::Module, pkg("", false), Entry::Blank, pkg("javax", false), pkg("java", false), Entry::Blank, pkg("", true)],
        star: 5,
        static_star: 3,
        origin: "IDEA 的默认值".to_string(),
        skipped: vec![],
    }
}

fn opt_name(tag: &str) -> String {
    attr(tag, "name").unwrap_or_default()
}

fn opt_u32(tag: &str) -> Option<u32> {
    attr(tag, "value").and_then(|v| v.trim().parse().ok())
}

/// 一份方案（`<code_scheme>…</code_scheme>`，Project.xml 里外面还包着一层 component）叠到 `base` 上
pub fn apply_scheme(xml: &str, mut s: Style) -> Style {
    let body = elements(xml, "code_scheme").into_iter().next().map(|(_, b)| b).unwrap_or(xml);
    let (top, sections) = cut(body, &["JavaCodeStyleSettings", "codeStyleSettings"]);
    // 方案级的行宽（各语言没单独写时用它）
    for (tag, _) in elements(&top, "option") {
        if opt_name(tag) == "RIGHT_MARGIN" {
            if let Some(v) = opt_u32(tag) {
                s.margin = v;
            }
        }
    }
    let mut lang_margin = None;
    for (name, inner) in sections {
        if name == "JavaCodeStyleSettings" {
            for (tag, val) in elements(inner, "option") {
                match opt_name(tag).as_str() {
                    "CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND" => s.star = opt_u32(tag).unwrap_or(s.star),
                    "NAMES_COUNT_TO_USE_IMPORT_ON_DEMAND" => s.static_star = opt_u32(tag).unwrap_or(s.static_star),
                    "IMPORT_LAYOUT_TABLE" => s.imports = layout(val),
                    other => s.skipped.push(other.to_string()),
                }
            }
            continue;
        }
        // codeStyleSettings 有好几种语言，只看 JAVA 的。它是 `<codeStyleSettings language="JAVA">` 的正文
        let Some((tag, _)) = elements(body, "codeStyleSettings").into_iter().find(|(_, b)| std::ptr::eq(*b, inner)) else { continue };
        if attr(tag, "language").as_deref() != Some("JAVA") {
            continue;
        }
        let (rest, ind) = cut(inner, &["indentOptions"]);
        for (_, ib) in ind {
            for (tag, _) in elements(ib, "option") {
                let v = opt_u32(tag);
                match opt_name(tag).as_str() {
                    "INDENT_SIZE" => s.indent = v.unwrap_or(s.indent),
                    "CONTINUATION_INDENT_SIZE" => s.cont = v.unwrap_or(s.cont),
                    "TAB_SIZE" => s.tab = v.unwrap_or(s.tab),
                    "USE_TAB_CHARACTER" => s.tabs = attr(tag, "value").as_deref() == Some("true"),
                    other => s.skipped.push(other.to_string()),
                }
            }
        }
        for (tag, _) in elements(&rest, "option") {
            match opt_name(tag).as_str() {
                "RIGHT_MARGIN" => lang_margin = opt_u32(tag),
                other => s.skipped.push(other.to_string()),
            }
        }
    }
    if let Some(m) = lang_margin {
        s.margin = m;
    }
    s
}

/// `IMPORT_LAYOUT_TABLE` 的 `<value>` 里那一串
fn layout(val: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut rest = val;
    // 按出现的次序认 `<package …/>` 和 `<emptyLine/>`（两种混在一起，`elements` 一次只认一种）
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
        let tag = &rest[..end];
        if tag.starts_with("<emptyLine") {
            out.push(Entry::Blank);
        } else if tag.starts_with("<package") {
            if attr(tag, "module").as_deref() == Some("true") {
                out.push(Entry::Module);
            } else {
                out.push(Entry::Pkg { name: attr(tag, "name").unwrap_or_default(), statik: attr(tag, "static").as_deref() == Some("true") });
            }
        }
        rest = &rest[end..];
    }
    out
}

/// IDEA 的配置目录：`~/Library/Application Support/JetBrains/` 下版本最新的那个 IntelliJIdea / IdeaIC（旗舰版、社区版）
pub fn idea_config_dir(home: &Path) -> Option<PathBuf> {
    let base = home.join("Library/Application Support/JetBrains");
    let mut best: Option<((u32, u32), PathBuf)> = None;
    for e in std::fs::read_dir(&base).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(ver) = name.strip_prefix("IntelliJIdea").or_else(|| name.strip_prefix("IdeaIC")) else { continue };
        let Some((y, m)) = ver.split_once('.').and_then(|(y, m)| Some((y.parse().ok()?, m.parse().ok()?))) else { continue };
        if best.as_ref().is_none_or(|(v, _)| (y, m) > *v) {
            best = Some(((y, m), e.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// 项目的 `.idea` 不比这个大：方案文件几 KB，大的不是它
const MAX_BYTES: u64 = 1 << 20;

fn read_small(p: &Path) -> Option<String> {
    let m = std::fs::metadata(p).ok()?;
    if m.len() > MAX_BYTES {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

fn option_value(xml: &str, name: &str) -> Option<String> {
    elements(xml, "option").into_iter().find(|(t, _)| opt_name(t) == name).and_then(|(t, _)| attr(t, "value"))
}

/// 这个项目在 IDEA 里用的是哪份方案（规矩在文件头）。`idea_dir` 是 IDEA 的配置目录（没装就 None）
pub fn resolve(root: &Path, idea_dir: Option<&Path>) -> Style {
    let cs = root.join(".idea/codeStyles");
    let cfg = read_small(&cs.join("codeStyleConfig.xml")).unwrap_or_default();
    if option_value(&cfg, "USE_PER_PROJECT_SETTINGS").as_deref() == Some("true") {
        if let Some(x) = read_small(&cs.join("Project.xml")) {
            let mut s = apply_scheme(&x, idea_default());
            s.origin = "这个项目的 .idea/codeStyles/Project.xml".to_string();
            return s;
        }
    }
    let Some(dir) = idea_dir else { return idea_default() };
    let name = option_value(&cfg, "PREFERRED_PROJECT_CODE_STYLE")
        .or_else(|| read_small(&dir.join("options/code.style.schemes.xml")).and_then(|x| option_value(&x, "CURRENT_SCHEME_NAME")))
        .unwrap_or_else(|| "Default".to_string());
    // 方案名里有 / 这种字符就不去拼路径了：名字是文件里读来的
    if name.contains('/') || name.contains("..") {
        return idea_default();
    }
    match read_small(&dir.join("codestyles").join(format!("{name}.xml"))) {
        Some(x) => {
            let mut s = apply_scheme(&x, idea_default());
            s.origin = format!("IDEA 的「{name}」方案（{}）", dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            s
        }
        None => idea_default(),
    }
}

/// 交给 jdtls 格式化请求的 `FormattingOptions` 里的那几项（Eclipse 格式化器的键）。jdtls 把请求里每一个不为空的选项原样放进格式化选项
/// （读 1.61 `FormatterHandler.getOptions` 的字节码：`FormattingOptions.entrySet()` 过滤掉空值、`putAll`）—— 每次格式化都带，不用重启 jdtls、
/// 按项目各是各的。缩进和 Tab 不在这里：那两样跟着编辑器对这个文件的判断走（`tabSize` / `insertSpaces`）
pub fn format_options(s: &Style) -> Vec<(&'static str, String)> {
    // Eclipse 的续行缩进按「几个缩进」算，IDEA 按空格数算：8 / 4 = 2
    let cont = (s.cont as f32 / s.indent.max(1) as f32).round().max(0.0) as u32;
    vec![
        ("org.eclipse.jdt.core.formatter.continuation_indentation", cont.to_string()),
        ("org.eclipse.jdt.core.formatter.continuation_indentation_for_array_initializer", cont.to_string()),
        ("org.eclipse.jdt.core.formatter.lineSplit", s.margin.to_string()),
        ("org.eclipse.jdt.core.formatter.comment.line_length", s.margin.to_string()),
    ]
}

/// jdtls 的 `java.completion.importOrder`：包名前缀的次序，`""` = 其余、`#` 打头的是 static。空行和模块 import 它没有，跳过
pub fn import_order(s: &Style) -> Vec<String> {
    s.imports
        .iter()
        .filter_map(|e| match e {
            Entry::Pkg { name, statik } => Some(if *statik { format!("#{name}") } else { name.clone() }),
            _ => None,
        })
        .collect()
}

/// 转不了的，一条一句人话（设置页「此项目」里列出来）
pub fn notes(s: &Style) -> Vec<String> {
    let mut out = Vec::new();
    // Eclipse 每两组之间都空一行；IDEA 里两组之间没写空行的（默认的 javax 和 java）转过去会多一行
    let pkgs: Vec<usize> = s.imports.iter().enumerate().filter(|(_, e)| matches!(e, Entry::Pkg { .. })).map(|(i, _)| i).collect();
    if pkgs.windows(2).any(|w| !s.imports[w[0]..w[1]].contains(&Entry::Blank)) {
        out.push("import 各分组之间将空一行（Eclipse 规则），IDEA 中相邻的分组（如 javax 和 java）之间会多出一个空行".to_string());
    }
    if !s.skipped.is_empty() {
        let mut names = s.skipped.clone();
        names.sort();
        names.dedup();
        let shown: Vec<&str> = names.iter().take(6).map(String::as_str).collect();
        let more = if names.len() > 6 { format!(" 等 {} 项", names.len()) } else { String::new() };
        out.push(format!("以下各项未转换（jdtls 格式化器不支持，将使用其默认设置）：{}{more}", shown.join("、")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = r#"<component name="ProjectCodeStyleConfiguration">
  <code_scheme name="Project" version="173">
    <option name="RIGHT_MARGIN" value="140" />
    <JavaCodeStyleSettings>
      <option name="CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND" value="999" />
      <option name="NAMES_COUNT_TO_USE_IMPORT_ON_DEMAND" value="999" />
      <option name="IMPORT_LAYOUT_TABLE">
        <value>
          <package name="" withSubpackages="true" static="true" />
          <emptyLine />
          <package name="com.acme" withSubpackages="true" static="false" />
          <emptyLine />
          <package name="" withSubpackages="true" static="false" />
        </value>
      </option>
      <option name="ALIGN_MULTILINE_ANNOTATION_PARAMETERS" value="true" />
    </JavaCodeStyleSettings>
    <codeStyleSettings language="JAVA">
      <option name="RIGHT_MARGIN" value="100" />
      <option name="KEEP_BLANK_LINES_IN_CODE" value="1" />
      <indentOptions>
        <option name="INDENT_SIZE" value="2" />
        <option name="CONTINUATION_INDENT_SIZE" value="4" />
        <option name="TAB_SIZE" value="2" />
      </indentOptions>
    </codeStyleSettings>
    <codeStyleSettings language="XML">
      <indentOptions>
        <option name="INDENT_SIZE" value="7" />
        <option name="USE_TAB_CHARACTER" value="true" />
      </indentOptions>
    </codeStyleSettings>
  </code_scheme>
</component>"#;

    #[test]
    fn 项目的方案_叠在默认上_只看_java_的那份() {
        let s = apply_scheme(PROJECT, idea_default());
        assert_eq!((s.indent, s.cont, s.tab, s.tabs), (2, 4, 2, false), "排在后面的 XML 那份（缩进 7、用 Tab）不该读进来");
        assert_eq!(s.margin, 100, "JAVA 自己的行宽盖过方案级的 140");
        assert_eq!((s.star, s.static_star), (999, 999));
        assert_eq!(import_order(&s), vec!["#", "com.acme", ""]);
        assert!(s.skipped.contains(&"KEEP_BLANK_LINES_IN_CODE".to_string()) && s.skipped.contains(&"ALIGN_MULTILINE_ANNOTATION_PARAMETERS".to_string()), "{:?}", s.skipped);
    }

    #[test]
    fn 只写了阈值的方案_别的都是默认值() {
        // 这台机器上 IDEA 的 Default 方案就是这个形状（值另写）：只改了两个阈值
        let x = r#"<code_scheme name="Default" version="173">
  <JavaCodeStyleSettings>
    <option name="CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND" value="99" />
    <option name="NAMES_COUNT_TO_USE_IMPORT_ON_DEMAND" value="99" />
  </JavaCodeStyleSettings>
</code_scheme>"#;
        let s = apply_scheme(x, idea_default());
        assert_eq!((s.star, s.static_star, s.indent, s.cont, s.margin), (99, 99, 4, 8, 120));
        assert_eq!(import_order(&s), vec!["", "javax", "java", "#"], "IDEA 默认的次序（模块 import 跳过）");
        assert!(s.skipped.is_empty());
    }

    #[test]
    fn 换成_jdtls_的_续行按几个缩进算() {
        let s = apply_scheme(PROJECT, idea_default());
        let o = format_options(&s);
        assert!(o.contains(&("org.eclipse.jdt.core.formatter.continuation_indentation", "2".into())), "4 / 2 = 2：{o:?}");
        assert!(o.contains(&("org.eclipse.jdt.core.formatter.lineSplit", "100".into())));
        let d = format_options(&idea_default());
        assert!(d.contains(&("org.eclipse.jdt.core.formatter.continuation_indentation", "2".into())), "默认 8 / 4 = 2");
    }

    #[test]
    fn 说清楚转不了的() {
        let n = notes(&idea_default());
        assert_eq!(n.len(), 1, "默认的 javax 和 java 挨着，Eclipse 会在中间空一行：{n:?}");
        let n = notes(&apply_scheme(PROJECT, idea_default()));
        assert!(n.iter().any(|x| x.contains("KEEP_BLANK_LINES_IN_CODE")), "{n:?}");
        assert!(!n.iter().any(|x| x.contains("空一行")), "这份每组之间都写了空行：{n:?}");
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("codestyle-{name}-{}-{:?}", std::process::id(), std::time::Instant::now()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // 测试夹具：往自己建的临时目录里写
    fn 照_idea_的规矩找方案() {
        let root = tmp("root");
        let idea = tmp("idea");
        std::fs::create_dir_all(idea.join("codestyles")).unwrap();
        std::fs::create_dir_all(idea.join("options")).unwrap();
        std::fs::write(idea.join("codestyles/Default.xml"), r#"<code_scheme name="Default"><JavaCodeStyleSettings><option name="CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND" value="50" /></JavaCodeStyleSettings></code_scheme>"#).unwrap();
        std::fs::write(idea.join("codestyles/Team.xml"), r#"<code_scheme name="Team"><JavaCodeStyleSettings><option name="CLASS_COUNT_TO_USE_IMPORT_ON_DEMAND" value="60" /></JavaCodeStyleSettings></code_scheme>"#).unwrap();
        // 没有 .idea：IDE 当前的方案，没写当前是哪个 = Default
        assert_eq!(resolve(&root, Some(&idea)).star, 50);
        // IDE 当前选了 Team
        std::fs::write(idea.join("options/code.style.schemes.xml"), r#"<application><component name="CodeStyleSchemeSettings"><option name="CURRENT_SCHEME_NAME" value="Team" /></component></application>"#).unwrap();
        assert_eq!(resolve(&root, Some(&idea)).star, 60);
        // 项目有自己的方案、但没打开「用项目的」：照样用 IDE 的
        std::fs::create_dir_all(root.join(".idea/codeStyles")).unwrap();
        std::fs::write(root.join(".idea/codeStyles/Project.xml"), PROJECT).unwrap();
        assert_eq!(resolve(&root, Some(&idea)).star, 60);
        // 打开了：用项目的
        std::fs::write(root.join(".idea/codeStyles/codeStyleConfig.xml"), r#"<component name="ProjectCodeStyleConfiguration"><state><option name="USE_PER_PROJECT_SETTINGS" value="true" /></state></component>"#).unwrap();
        let s = resolve(&root, Some(&idea));
        assert_eq!((s.star, s.indent), (999, 2));
        assert!(s.origin.contains("Project.xml"));
        // 项目指定了 IDE 的哪份方案
        std::fs::write(root.join(".idea/codeStyles/codeStyleConfig.xml"), r#"<component><state><option name="PREFERRED_PROJECT_CODE_STYLE" value="Default" /></state></component>"#).unwrap();
        assert_eq!(resolve(&root, Some(&idea)).star, 50);
        // 方案名带路径的不去拼
        std::fs::write(root.join(".idea/codeStyles/codeStyleConfig.xml"), r#"<component><state><option name="PREFERRED_PROJECT_CODE_STYLE" value="../../x" /></state></component>"#).unwrap();
        assert_eq!(resolve(&root, Some(&idea)), idea_default());
        // 没装 IDEA
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&idea);
        assert_eq!(resolve(&root, None), idea_default());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // 测试夹具：自己建的临时目录
    fn idea_的配置目录挑版本最新的() {
        let home = tmp("home");
        let jb = home.join("Library/Application Support/JetBrains");
        for d in ["IntelliJIdea2025.3", "IntelliJIdea2026.1", "IdeaIC2025.2", "DataGrip2026.2", "IntelliJIdea2026.1.backup"] {
            std::fs::create_dir_all(jb.join(d)).unwrap();
        }
        assert_eq!(idea_config_dir(&home).and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).as_deref(), Some("IntelliJIdea2026.1"));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(idea_config_dir(&home), None);
    }
}
