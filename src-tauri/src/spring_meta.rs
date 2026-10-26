//! Spring Boot 配置文件的补全和文档（docs/TAKEOVER.md G30 的轻量做法）：`application.yml` / `.properties` 里敲键名给候选、悬停说这个键是什么。
//!
//! 不起 Spring Tools 的语言服务器（又一个 JVM、约 300 MB）：Spring Boot 的 jar 自己带着一份 `META-INF/spring-configuration-metadata.json`
//! （每个配置键的名字、类型、说明、默认值；`hints` 里是可选的值）—— IDEA 读的也是它。项目自己的 `@ConfigurationProperties`
//! 开了 `spring-boot-configuration-processor` 的话编出来在输出目录里（`target/classes`、`build/classes/java/main`）同一个名字。
//! classpath 问 jdtls（`java.project.getClasspaths`，Maven / Gradle 一样）；jar 里的文件用系统的 `unzip -p` 读（同 Maven 插件的 plugin.xml，不加 zip 的依赖）。
//! 每个 jar 按（路径, 改动时间）记住解析结果：几十个 jar、一份 autoconfigure 的元数据上 MB，每次敲键都读就太慢了

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// 一个配置键
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: String,
    /// Java 类型（`java.lang.Integer`、`java.util.List<java.lang.String>`），没写是空的
    pub ty: String,
    pub desc: String,
    /// 默认值（原样转成字符串；列表照 JSON 写）
    pub default: String,
    /// 过时了：换成什么（空 = 没说）。没过时是 None
    pub deprecated: Option<String>,
    /// `hints` 里给的可选值（值, 说明）；布尔和枚举类型的由前端按类型补
    pub values: Vec<(String, String)>,
}

/// 元数据文件的名字：Spring Boot 生成的、作者手写补充的
const FILES: [&str; 2] = ["META-INF/spring-configuration-metadata.json", "META-INF/additional-spring-configuration-metadata.json"];
/// 一份元数据读多少（spring-boot-autoconfigure 的约 1–2 MB；超过这个数的不是我们要的那种文件）
const CAP: usize = 16 << 20;

/// 一份元数据 JSON → 键。`hints` 按名字挂到键上；同名的键（Spring 偶尔一个键在两个组里出现）只留第一个
pub fn parse(text: &str) -> Vec<Prop> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let s = |x: &Value| match x {
        Value::Null => String::new(),
        Value::String(t) => t.clone(),
        other => other.to_string(),
    };
    let mut hints: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for h in v["hints"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let vals: Vec<(String, String)> = h["values"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter().map(|x| (s(&x["value"]), s(&x["description"]))).filter(|(v, _)| !v.is_empty()).collect();
        if let Some(n) = h["name"].as_str() {
            if !vals.is_empty() {
                hints.insert(n.to_string(), vals);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    v["properties"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| {
            let name = p["name"].as_str()?.to_string();
            if !seen.insert(name.clone()) {
                return None;
            }
            let deprecated = p.get("deprecation").filter(|d| d.is_object()).map(|d| s(&d["replacement"])).or_else(|| p["deprecated"].as_bool().filter(|b| *b).map(|_| String::new()));
            Some(Prop { values: hints.remove(&name).unwrap_or_default(), ty: s(&p["type"]), desc: s(&p["description"]), default: s(&p["defaultValue"]), deprecated, name })
        })
        .collect()
}

static CACHE: Mutex<Option<HashMap<PathBuf, (Option<SystemTime>, Vec<Prop>)>>> = Mutex::new(None);

/// 一个 classpath 条目（jar 或输出目录）里的元数据。jar 按（路径, 改动时间）缓存；目录每次现读（那是你正在编的项目，随时会变）
fn of_entry(p: &Path) -> Vec<Prop> {
    if p.is_dir() {
        return FILES.iter().filter_map(|f| std::fs::read_to_string(p.join(f)).ok()).flat_map(|t| parse(&t)).collect();
    }
    if p.extension().is_none_or(|e| e != "jar") {
        return Vec::new();
    }
    let mtime = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let Some((t, props)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(p)) {
        if *t == mtime {
            return props.clone();
        }
    }
    let mut props = Vec::new();
    for f in FILES {
        let mut c = std::process::Command::new("/usr/bin/unzip");
        c.arg("-p").arg(p).arg(f);
        if let Ok(o) = procutil::run_capped(&mut c, CAP, procutil::ERR_CAP, procutil::Overflow::Kill) {
            // 没有这个文件 unzip 退 11：那不是错，是这个 jar 里没元数据（多数 jar 都没有）
            if o.status.success() && !o.truncated {
                props.extend(parse(&String::from_utf8_lossy(&o.stdout)));
            }
        }
    }
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(p.to_path_buf(), (mtime, props.clone()));
    props
}

/// 一个模块 classpath 上全部的键（按名字排、去重）
pub fn collect(entries: &[PathBuf]) -> Vec<Prop> {
    let mut all: Vec<Prop> = entries.iter().flat_map(|e| of_entry(e)).collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all.dedup_by(|a, b| a.name == b.name);
    all
}

/// 是不是 Spring Boot 的配置文件（补全只挂在这几种上）：`application*` / `bootstrap*` 的 yml、yaml、properties
pub fn is_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    (name.starts_with("application") || name.starts_with("bootstrap")) && (name.ends_with(".yml") || name.ends_with(".yaml") || name.ends_with(".properties"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// spring-boot-autoconfigure 4.0 那份的样子（节选，字段原样）
    const META: &str = r#"{
      "groups": [{ "name": "server", "type": "org.springframework.boot.autoconfigure.web.ServerProperties" }],
      "properties": [
        { "name": "server.port", "type": "java.lang.Integer", "description": "Server HTTP port.", "sourceType": "org.springframework.boot.autoconfigure.web.ServerProperties", "defaultValue": 8080 },
        { "name": "spring.main.banner-mode", "type": "org.springframework.boot.Banner$Mode", "description": "Mode used to display the banner when the application runs.", "defaultValue": "console" },
        { "name": "server.servlet.path", "type": "java.lang.String", "deprecation": { "level": "error", "replacement": "spring.mvc.servlet.path" } },
        { "name": "server.port", "type": "java.lang.Integer", "description": "dup" },
        { "name": "spring.profiles.active", "type": "java.util.List<java.lang.String>" }
      ],
      "hints": [{ "name": "spring.main.banner-mode", "values": [{ "value": "off" }, { "value": "console", "description": "Print to System.out." }] }]
    }"#;

    #[test]
    fn 元数据_键带类型说明默认值_过时的说换成什么_可选值挂上去_同名只留第一个() {
        let p = parse(META);
        assert_eq!(p.len(), 4);
        let port = &p[0];
        assert_eq!((port.name.as_str(), port.ty.as_str(), port.desc.as_str(), port.default.as_str()), ("server.port", "java.lang.Integer", "Server HTTP port.", "8080"));
        assert_eq!(p[1].values, [("off".to_string(), String::new()), ("console".to_string(), "Print to System.out.".to_string())]);
        assert_eq!(p[2].deprecated.as_deref(), Some("spring.mvc.servlet.path"));
        assert_eq!(p[0].deprecated, None);
        assert!(parse("不是 JSON").is_empty());
    }

    #[test]
    fn 只认_application_和_bootstrap_的配置文件() {
        assert!(is_config("/p/src/main/resources/application.yml"));
        assert!(is_config("/p/src/main/resources/application-dev.properties"));
        assert!(is_config("/p/bootstrap.yaml"));
        assert!(!is_config("/p/docker-compose.yml"));
        assert!(!is_config("/p/application.json"));
    }
}
