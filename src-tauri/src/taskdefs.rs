//! 任务从哪来（#48 第 2 步，docs/TASKS.md Q2）：项目里的 `.lite-ide/tasks.json`，加上自动认出来的 `package.json` scripts。
//!
//! 纯函数 + 读盘，不碰进程（起和停在 `tasksvc`）。放在主 crate 里、挨着 `settings.rs`：`tasks.json` 和 `settings.json` 是同一种 JSONC
//! （注释、末尾逗号），用的是同一个 [`crate::settings::strip`] —— 一种格式一个解析器，不另写一份。
//!
//! **为什么不自动认 Maven / Gradle / Python**：`spring-boot:run` 要不要带 profile、Python 跑哪个入口，猜错比不猜糟（Q2）。
//! package.json 不一样：`scripts` 就是作者自己写下的「这个项目能跑什么」，照着列出来不算猜。

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// 任务定义文件，相对项目根
pub const FILE: &str = ".lite-ide/tasks.json";
/// 读文件的上限：`tasks.json`、`package.json` 都是人写的几 KB，超过这个数多半不是我们要的那种文件，不读进内存
const MAX_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// `.lite-ide/tasks.json` 里写的
    File,
    /// 某个 `package.json` 的 scripts；`dir` 相对项目根（根目录是 ""）
    Package { dir: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    pub name: String,
    /// 一整行，交给登录 shell
    pub command: String,
    /// 相对项目根；"" = 根
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub source: Source,
}

#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub defs: Vec<Def>,
    /// `tasks.json` 在不在（列表底部给「打开 tasks.json」还是「新建 tasks.json」）
    pub file: bool,
    /// 读的时候发现的问题。**都要告诉人**（同 settings.json 的规矩）：静默跳过会让人以为任务写对了
    pub problems: Vec<String>,
}

/// 新建 `tasks.json` 时的模板：三个常见的例子，全注释掉 —— 解析出来是空数组，人照着改
pub fn template() -> &'static str {
    r#"// lite-ide 的任务（docs/USAGE.md「任务」）。⌃⌥R 选一个跑，⌃R 再跑上一个，⌘F2 停。
// 每个任务：name（列表里显示的名字）、command（一整行，交给你的登录 shell，和在终端里敲一样）、
// cwd（相对项目根，可以不写）、env（额外的环境变量，可以不写）。
// package.json 里的 scripts 不用写在这儿，会自动列出来；同名的以这里为准。
[
  // { "name": "后端", "command": "mvn spring-boot:run", "cwd": "admin" },
  // { "name": "前端", "command": "pnpm dev", "cwd": "web" },
  // { "name": "脚本", "command": "python3 main.py", "env": { "APP_ENV": "dev" } },
]
"#
}

/// 解析 `tasks.json` 的文本。坏掉的那一个任务跳过、说一句，别的照常 —— 一处写错不该让整份都不能用
pub fn parse_file(text: &str) -> (Vec<Def>, Vec<String>) {
    let clean = match crate::settings::strip(text) {
        Ok(c) => c,
        Err(p) => return (Vec::new(), vec![format!("{FILE} 写错了：{}", p.text())]),
    };
    if clean.trim().is_empty() {
        return (Vec::new(), Vec::new());
    }
    let value: Value = match serde_json::from_str(&clean) {
        Ok(v) => v,
        Err(e) => {
            let pos = crate::settings::pos_of_line_byte(text, e.line(), e.column());
            let msg = if e.classify() == serde_json::error::Category::Eof {
                "没写完，可能少了 ] 或者引号没收尾".to_string()
            } else {
                e.to_string().split(" at line ").next().unwrap_or_default().to_string()
            };
            return (Vec::new(), vec![format!("{FILE} 第 {} 行写错了：{msg}", pos.line)]);
        }
    };
    let Value::Array(items) = value else {
        return (Vec::new(), vec![format!("{FILE} 最外层要是 [ … ]（任务的列表）")]);
    };
    let mut defs: Vec<Def> = Vec::new();
    let mut problems = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let Value::Object(o) = item else {
            problems.push(format!("{FILE} 第 {n} 个任务要是 {{ … }}，跳过了"));
            continue;
        };
        let name = o.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
        let command = o.get("command").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if name.is_empty() || command.is_empty() {
            problems.push(format!("{FILE} 第 {n} 个任务缺 name 或 command（都要是非空的字符串），跳过了"));
            continue;
        }
        let who = format!("{FILE} 里的「{name}」");
        for k in o.keys() {
            if !matches!(k.as_str(), "name" | "command" | "cwd" | "env") {
                // 拼错时（`cmd`、`dir`）这是唯一的线索 —— 不说的话它照样跑，只是没在你以为的目录里
                problems.push(format!("{who}：不认识的键 {k}（认 name、command、cwd、env）"));
            }
        }
        let cwd = match o.get("cwd") {
            None => String::new(),
            Some(Value::String(s)) => match inside(s) {
                Some(c) => c,
                None => {
                    problems.push(format!("{who}：cwd 要是项目里的相对路径（不能是绝对路径、不能用 .. 出去），跳过了"));
                    continue;
                }
            },
            Some(_) => {
                problems.push(format!("{who}：cwd 要是字符串，跳过了"));
                continue;
            }
        };
        let mut env = Vec::new();
        match o.get("env") {
            None => {}
            Some(Value::Object(m)) => {
                for (k, v) in m {
                    match v.as_str() {
                        Some(v) => env.push((k.clone(), v.to_string())),
                        None => problems.push(format!("{who}：env 里的 {k} 要是字符串，没带上")),
                    }
                }
            }
            Some(_) => problems.push(format!("{who}：env 要是 {{ \"名字\": \"值\" }}，没带上")),
        }
        if defs.iter().any(|d| d.name == name) {
            problems.push(format!("{who} 重名了，用的是前面那个"));
            continue;
        }
        defs.push(Def { name: name.to_string(), command: command.to_string(), cwd, env, source: Source::File });
    }
    (defs, problems)
}

/// `cwd` 规整成项目里的相对路径；绝对路径、`..` 出去 → None
fn inside(s: &str) -> Option<String> {
    let p = Path::new(s.trim());
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(x) => parts.push(x.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// `package.json` 里的 scripts，**按文件里的顺序**。
///
/// serde_json 的 `Map` 没开 `preserve_order`，读出来按键名排序 —— `dev` 会排到 `build` 后面，而作者是按「最常用的在前」写的。
/// 为这一处去开整个依赖树的 feature 不值，这里自己收：反序列化时键本来就是按文档顺序一个个交过来的
struct Ordered(Vec<(String, String)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("scripts 对象")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<Ordered, M::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = m.next_entry::<String, Value>()? {
                    // 值不是字符串的（坏掉的 package.json）跳过，不让一条坏的拖垮整份
                    if let Value::String(s) = v {
                        out.push((k, s));
                    }
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    #[serde(default)]
    scripts: Option<Ordered>,
    /// corepack 的约定：`"packageManager": "pnpm@9.1.0"`。比看锁文件准（锁文件可能是别人提交错的）
    #[serde(default)]
    package_manager: Option<String>,
}

/// npm 自己在 install / publish / version 时会跑的那些，不是给人手动跑的；`preX` / `postX` 是 `X` 的附属，跑 `X` 时 npm 自己带上
const LIFECYCLE: &[&str] = &[
    "preinstall", "install", "postinstall", "prepublish", "preprepare", "prepare", "postprepare", "prepublishOnly",
    "prepack", "postpack", "publish", "postpublish", "preversion", "version", "postversion", "dependencies",
];

/// 一份 `package.json` 能跑的 scripts（去掉生命周期钩子）和它声明的包管理器
pub fn package_scripts(text: &str) -> Result<(Vec<String>, Option<String>), String> {
    let p: PackageJson = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let all = p.scripts.map(|o| o.0).unwrap_or_default();
    let names: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
    let hook = |k: &str| {
        ["pre", "post"].iter().any(|pre| k.strip_prefix(pre).is_some_and(|rest| names.contains(&rest)))
    };
    let runnable = all.iter().map(|(k, _)| k.clone()).filter(|k| !LIFECYCLE.contains(&k.as_str()) && !hook(k)).collect();
    let pm = p.package_manager.and_then(|s| {
        let n = s.split('@').next().unwrap_or_default().to_string();
        ["pnpm", "yarn", "npm", "bun"].contains(&n.as_str()).then_some(n)
    });
    Ok((runnable, pm))
}

/// 用哪个包管理器：声明了就听声明的，否则从这个目录往上（到项目根为止）找锁文件，都没有就 npm
fn package_manager(dir: &Path, root: &Path, declared: Option<String>) -> String {
    if let Some(d) = declared {
        return d;
    }
    let mut cur = Some(dir);
    while let Some(d) = cur {
        for (lock, pm) in [("pnpm-lock.yaml", "pnpm"), ("yarn.lock", "yarn"), ("bun.lock", "bun"), ("bun.lockb", "bun"), ("package-lock.json", "npm")] {
            if d.join(lock).is_file() {
                return pm.into();
            }
        }
        if d == root {
            break;
        }
        cur = d.parent();
    }
    "npm".into()
}

/// 放进 shell 命令行的一个词。scripts 名字常带 `:`（`build:prod`），那个不用引；带空格、引号之类的才套单引号
fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-:./@+=,%".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

fn read_capped(p: &Path) -> Option<Result<String, String>> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > MAX_BYTES {
        return Some(Err(format!("超过 {} KB，没读", MAX_BYTES / 1024)));
    }
    Some(std::fs::read_to_string(p).map_err(|e| e.to_string()))
}

/// 列出一个项目的全部任务：`tasks.json` 里的在前，然后根目录的 scripts，再然后一层子目录的（按目录名）。
/// 和 `tasks.json` 里同名的自动任务不列（以文件为准，Q2）
pub fn discover(root: &Path) -> Found {
    let mut found = Found::default();
    match read_capped(&root.join(FILE)) {
        None => {}
        Some(Err(e)) => {
            found.file = true;
            found.problems.push(format!("{FILE} 读不了：{e}"));
        }
        Some(Ok(text)) => {
            found.file = true;
            let (defs, problems) = parse_file(&text);
            found.defs = defs;
            found.problems = problems;
        }
    }

    // 根目录 + 一层子目录。不往下翻：monorepo 的包一般就在第一层（apps/、packages/ 底下那一层是例外，第一版不管）；
    // node_modules、隐藏目录不进
    let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.') && n != "node_modules"))
            .collect();
        subs.sort();
        dirs.extend(subs);
    }
    for dir in dirs {
        let rel = dir.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
        let pj = dir.join("package.json");
        let (scripts, declared) = match read_capped(&pj) {
            None => continue,
            Some(Err(e)) => {
                found.problems.push(format!("{} 读不了：{e}", shown(&rel, "package.json")));
                continue;
            }
            Some(Ok(text)) => match package_scripts(&text) {
                Ok(x) => x,
                Err(e) => {
                    found.problems.push(format!("{} 解析不了：{e}", shown(&rel, "package.json")));
                    continue;
                }
            },
        };
        if scripts.is_empty() {
            continue;
        }
        let pm = package_manager(&dir, root, declared);
        for s in scripts {
            let name = if rel.is_empty() { s.clone() } else { format!("{rel}/{s}") };
            if found.defs.iter().any(|d| d.name == name) {
                continue;
            }
            found.defs.push(Def {
                name,
                command: format!("{pm} run {}", shell_word(&s)),
                cwd: rel.clone(),
                env: Vec::new(),
                source: Source::Package { dir: rel.clone() },
            });
        }
    }
    found
}

fn shown(rel: &str, file: &str) -> String {
    if rel.is_empty() {
        file.to_string()
    } else {
        format!("{rel}/{file}")
    }
}

/// 新建 `tasks.json`（带模板）。已经有了就不动它、返回路径 —— 「新建」点两下不能把人写好的冲掉
pub fn create_file(root: &Path) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let p = root.join(FILE);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // `create_new`：rust.md「std API 会吃掉已有文件」—— `File::create` 会把已有的截成 0 字节
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
        Ok(mut f) => f.write_all(template().as_bytes())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lite-ide-taskdefs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 模板解析出来是空的_一句问题都没有() {
        assert_eq!(parse_file(template()), (Vec::new(), Vec::new()));
    }

    #[test]
    fn 四个字段_注释和末尾逗号都认() {
        let (defs, problems) = parse_file(
            r#"[
              // 后端
              { "name": "后端", "command": "mvn spring-boot:run", "cwd": "./admin/", "env": { "A": "1" }, },
              { "name": "脚本", "command": "python3 main.py" },
            ]"#,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(defs[0], Def { name: "后端".into(), command: "mvn spring-boot:run".into(), cwd: "admin".into(), env: vec![("A".into(), "1".into())], source: Source::File });
        assert_eq!(defs[1].cwd, "");
    }

    #[test]
    fn 写坏的那一个跳过并说出来_别的照常() {
        let (defs, problems) = parse_file(
            r#"[
              { "name": "好的", "command": "echo ok" },
              { "name": "缺命令" },
              { "name": "出去了", "command": "ls", "cwd": "../别人的" },
              { "name": "绝对路径", "command": "ls", "cwd": "/etc" },
              { "name": "拼错", "cmd": "x", "command": "echo y" },
              { "name": "好的", "command": "echo dup" },
            ]"#,
        );
        assert_eq!(defs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["好的", "拼错"]);
        assert_eq!(defs[0].command, "echo ok", "重名用前面那个");
        let all = problems.join("\n");
        for want in ["第 2 个任务缺 name 或 command", "「出去了」：cwd", "「绝对路径」：cwd", "不认识的键 cmd", "「好的」 重名了"] {
            assert!(all.contains(want), "少了「{want}」：\n{all}");
        }
    }

    #[test]
    fn 语法错说出第几行() {
        let (defs, problems) = parse_file("[\n  { \"name\": \"a\", \"command\": \"b\" }\n  { \"name\": \"c\" }\n]");
        assert!(defs.is_empty());
        assert!(problems[0].contains("第 3 行"), "{problems:?}");
        let (_, problems) = parse_file("{ \"name\": \"a\" }");
        assert!(problems[0].contains("最外层要是 ["), "{problems:?}");
    }

    /// 这条测试存在的理由：serde_json 的 Map 按键名排序，`dev` 会跑到 `build` 后面
    #[test]
    fn scripts_按文件里的顺序_生命周期钩子不列() {
        let (s, pm) = package_scripts(
            r#"{ "scripts": { "dev": "vite", "build": "vite build", "prebuild": "rm -rf dist", "postinstall": "x",
                 "test:unit": "node t", "preview": "vite preview" }, "packageManager": "pnpm@9.1.0" }"#,
        )
        .unwrap();
        assert_eq!(s, ["dev", "build", "test:unit", "preview"], "preview 不是 view 的钩子（没有 view 这个 script），要留着");
        assert_eq!(pm.as_deref(), Some("pnpm"));
    }

    #[test]
    fn 自动认出_包管理器按声明再按锁文件_同名以文件为准() {
        let root = tmp("discover");
        std::fs::write(root.join("package.json"), r#"{ "scripts": { "dev": "vite", "lint": "eslint ." } }"#).unwrap();
        std::fs::write(root.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::create_dir_all(root.join("api")).unwrap();
        std::fs::write(root.join("api/package.json"), r#"{ "scripts": { "start": "node s" }, "packageManager": "yarn@4" }"#).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::write(root.join("node_modules/package.json"), r#"{ "scripts": { "no": "x" } }"#).unwrap();
        std::fs::create_dir_all(root.join(".lite-ide")).unwrap();
        std::fs::write(root.join(FILE), r#"[{ "name": "lint", "command": "pnpm lint --fix" }]"#).unwrap();

        let f = discover(&root);
        assert!(f.file && f.problems.is_empty(), "{:?}", f.problems);
        let got: Vec<(&str, &str, &str)> = f.defs.iter().map(|d| (d.name.as_str(), d.command.as_str(), d.cwd.as_str())).collect();
        assert_eq!(
            got,
            [("lint", "pnpm lint --fix", ""), ("dev", "pnpm run dev", ""), ("api/start", "yarn run start", "api")],
            "文件里的在前；同名的 lint 以文件为准；子目录声明了 yarn 就用 yarn；node_modules 不进"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 没有锁文件就是_npm_名字怪的套引号() {
        let root = tmp("npm");
        std::fs::write(root.join("package.json"), r#"{ "scripts": { "build:prod": "x", "it's": "y" } }"#).unwrap();
        let f = discover(&root);
        assert_eq!(f.defs[0].command, "npm run build:prod");
        assert_eq!(f.defs[1].command, r"npm run 'it'\''s'");
        assert!(!f.file);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 新建不冲掉已有的() {
        let root = tmp("create");
        let p = create_file(&root).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), template());
        std::fs::write(&p, "[]").unwrap();
        create_file(&root).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[]", "第二次新建把写好的冲掉了");
        let _ = std::fs::remove_dir_all(&root);
    }
}
