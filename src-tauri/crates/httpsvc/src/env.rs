//! `.http` 文件的环境（IDEA 的 HTTP Client 那套）：`http-client.env.json` 放公开的，`http-client.private.env.json` 放密码、token。
//!
//! ```json
//! { "$shared": { "ver": "v1" }, "dev": { "host": "localhost:8080" }, "test": { "host": "test.example.com" } }
//! ```
//!
//! 找法照 IDEA：从 `.http` 文件所在的目录往上，一直到项目根；**近的说了算**（子目录里的同名变量盖掉上层的），
//! 同一层里私有的盖掉公开的，`$shared` 是每个环境都有的底子（环境里写了同名的就用环境里的）。
//! 值可以是对象（2024.2 起 `{{client.host}}` 按路径取），这里原样交给前端，取值在那边做。
//!
//! 文件读多大有上限（[`FILE_MAX`]）：环境文件是几十行的东西，一个几百 MB 的同名文件不该把应用拖住。

use serde_json::{Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const FILE_MAX: u64 = 1 << 20;
pub const PUBLIC: &str = "http-client.env.json";
pub const PRIVATE: &str = "http-client.private.env.json";
const SHARED: &str = "$shared";

/// 找到的环境：名字（不含 `$shared`，按名字排）→ 合并好的变量；以及读了哪些文件、哪些读坏了（坏的照实说，不悄悄当没有）
#[derive(Debug, Default)]
pub struct Envs {
    pub envs: Map<String, Value>,
    pub files: Vec<PathBuf>,
    pub problems: Vec<String>,
}

fn read(p: &Path) -> Result<Option<Map<String, Value>>, String> {
    let f = match std::fs::File::open(p) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}：{e}", p.display())),
    };
    let mut buf = Vec::new();
    // 读 cap+1：超了要报出来，不能悄悄拿一份截断的去解析（rust.md「读 cap+1 判截断」）
    f.take(FILE_MAX + 1).read_to_end(&mut buf).map_err(|e| format!("{}：{e}", p.display()))?;
    if buf.len() as u64 > FILE_MAX {
        return Err(format!("{} 超过 {} MB，未读取", p.display(), FILE_MAX >> 20));
    }
    match serde_json::from_slice::<Value>(&buf) {
        Ok(Value::Object(m)) => Ok(Some(m)),
        Ok(_) => Err(format!("{}：顶层必须是对象（环境名 → 变量）", p.display())),
        Err(e) => Err(format!("{}：JSON 格式无效（{e}）", p.display())),
    }
}

/// `over` 盖到 `base` 上（只合并一层：变量名 → 值；值是对象的整个替换，同 IDEA）
fn layer(base: &mut Map<String, Value>, over: &Map<String, Value>) {
    for (k, v) in over {
        base.insert(k.clone(), v.clone());
    }
}

/// 从 `dir` 往上到 `root`（含）找环境文件。`dir` 不在 `root` 底下就只看 `dir` 自己
pub fn load(root: &Path, dir: &Path) -> Envs {
    let mut chain: Vec<&Path> = Vec::new();
    let mut d = Some(dir);
    while let Some(x) = d {
        chain.push(x);
        if x == root || !x.starts_with(root) {
            break;
        }
        d = x.parent();
    }
    let mut out = Envs::default();
    // 远的先铺、近的后盖；每一层先公开、后私有
    let mut raw: Map<String, Value> = Map::new();
    for d in chain.iter().rev() {
        for name in [PUBLIC, PRIVATE] {
            let p = d.join(name);
            match read(&p) {
                Ok(Some(m)) => {
                    out.files.push(p);
                    for (env, vars) in m {
                        let Value::Object(vars) = vars else {
                            out.problems.push(format!("{}：「{env}」的值必须是对象（变量名 → 值）", d.join(name).display()));
                            continue;
                        };
                        let slot = raw.entry(env).or_insert_with(|| Value::Object(Map::new()));
                        if let Value::Object(m) = slot {
                            layer(m, &vars);
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => out.problems.push(e),
            }
        }
    }
    let shared = raw.remove(SHARED).and_then(|v| if let Value::Object(m) = v { Some(m) } else { None }).unwrap_or_default();
    for (env, vars) in raw {
        let mut m = shared.clone();
        if let Value::Object(v) = &vars {
            layer(&mut m, v);
        }
        out.envs.insert(env, Value::Object(m));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("httpsvc-env-{name}-{}-{:?}", std::process::id(), std::time::Instant::now()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
    fn put(p: &Path, s: &str) {
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn 近的盖远的_私有盖公开_shared_是底子() {
        let root = tmp("layers");
        let sub = root.join("api/orders");
        std::fs::create_dir_all(&sub).unwrap();
        put(&root.join(PUBLIC), r#"{ "$shared": { "ver": "v1", "host": "shared" }, "dev": { "host": "root-dev", "token": "公开的" }, "prod": { "host": "prod.example.com" } }"#);
        put(&root.join(PRIVATE), r#"{ "dev": { "token": "私有的" } }"#);
        put(&sub.join(PUBLIC), r#"{ "dev": { "host": "sub-dev", "client": { "id": 7 } } }"#);
        let e = load(&root, &sub);
        assert!(e.problems.is_empty(), "{:?}", e.problems);
        assert_eq!(e.envs.keys().collect::<Vec<_>>(), vec!["dev", "prod"], "$shared 不算一个环境");
        let dev = &e.envs["dev"];
        assert_eq!(dev["host"], "sub-dev", "近的盖远的");
        assert_eq!(dev["token"], "私有的", "同一层私有的盖公开的");
        assert_eq!(dev["ver"], "v1", "$shared 是底子");
        assert_eq!(dev["client"]["id"], 7, "值是对象的原样交出去（{{{{client.id}}}} 在前端按路径取）");
        assert_eq!(e.envs["prod"]["host"], "prod.example.com", "环境里写了的盖掉 $shared");
        assert_eq!(e.files.len(), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 坏的文件照实说_不悄悄当没有_也不往项目根外面找() {
        let above = tmp("bad");
        let root = above.join("proj");
        std::fs::create_dir_all(&root).unwrap();
        // 项目根上面一层放一个：不该被读到（往上找到项目根为止）
        put(&above.join(PUBLIC), r#"{ "outside": { "x": 1 } }"#);
        put(&root.join(PUBLIC), "{ dev: 1 ");
        put(&root.join(PRIVATE), r#"{ "dev": "不是对象" }"#);
        let e = load(&root, &root);
        assert_eq!(e.problems.len(), 2, "{:?}", e.problems);
        assert!(e.problems[0].contains("JSON 格式无效"), "{:?}", e.problems);
        assert!(!e.envs.contains_key("outside"), "项目根外面的环境文件被读进来了：{:?}", e.envs);
        let _ = std::fs::remove_dir_all(&above);
    }
}
