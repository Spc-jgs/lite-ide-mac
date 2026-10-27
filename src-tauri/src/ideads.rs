//! 从 IDEA 带数据库连接过来（docs/DATABASE.md 第 4 节，DB-4）：读项目里的 `.idea/dataSources.xml`（连接串、驱动，跟着项目走、会进 git）
//! 和 `.idea/dataSources.local.xml`（用户名，只在本机），按 `uuid` 对上。只读，不往 `.idea` 里写。
//!
//! **密码不带**：IDEA 把密码存在它自己的钥匙串条目里，那是 IDEA 的东西，不去读 —— 导入后第一次连之前在「编辑连接」里填一次。
//! 认得的库（MySQL / MariaDB / PostgreSQL / H2 / SQLite）才能导；别的（Oracle、SQL Server、达梦……）列出来、说清为什么导不了，不悄悄丢掉。
//! `$PROJECT_DIR$` / `$USER_HOME$` 换回真路径（H2 的文件库常写成 `jdbc:h2:$PROJECT_DIR$/data/db`），同 `ideacfg`

use crate::ideacfg::elements;
use crate::junit::{attr, unescape};
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 4 << 20;

/// IDEA 里的一个数据源，转好了
#[derive(Clone, Debug, PartialEq)]
pub struct IdeaDs {
    pub uuid: String,
    pub name: String,
    /// 我们认得的种类（mysql / postgresql / h2）；认不得是 None，`note` 说为什么
    pub kind: Option<&'static str>,
    pub url: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub note: Option<String>,
}

/// 连接串是哪种库。MariaDB 用 MySQL 的驱动连（Connector/J 认 MariaDB 的协议），说一句
fn kind_of(url: &str) -> (Option<&'static str>, Option<String>) {
    let u = url.to_ascii_lowercase();
    if u.starts_with("jdbc:mysql:") {
        (Some("mysql"), None)
    } else if u.starts_with("jdbc:mariadb:") {
        (Some("mysql"), Some("MariaDB 将使用 MySQL 驱动连接".into()))
    } else if u.starts_with("jdbc:postgresql:") {
        (Some("postgresql"), None)
    } else if u.starts_with("jdbc:h2:") {
        (Some("h2"), None)
    } else if u.starts_with("jdbc:sqlite:") {
        (Some("sqlite"), None)
    } else {
        let product = u.strip_prefix("jdbc:").and_then(|r| r.split(':').next()).filter(|p| !p.is_empty()).unwrap_or("未知");
        (None, Some(format!("暂不支持 {product}（没有它的驱动）")))
    }
}

/// `jdbc:mysql://h:3306/db?x=1` → (h, 3306, db)。认不出就是空的（连接用的是完整的 URL，这三样只给树里显示）
fn host_port_db(url: &str) -> (String, u16, String) {
    let Some(rest) = url.split_once("://").map(|x| x.1) else { return (String::new(), 0, String::new()) };
    let rest = rest.split(['?', ';']).next().unwrap_or("");
    let (hp, db) = rest.split_once('/').unwrap_or((rest, ""));
    // 多个主机（`h1:3306,h2:3306`）只取第一个给树里显示
    let hp = hp.split(',').next().unwrap_or("");
    let (h, p) = match hp.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, p.parse().unwrap_or(0)),
        _ => (hp, 0),
    };
    (h.to_string(), p, db.to_string())
}

fn child_text(body: &str, name: &str) -> String {
    elements(body, name).first().map(|(_, b)| unescape(b.trim())).unwrap_or_default()
}

/// 两份文件的内容 → 数据源。`local` 没有就是空串（用户名就是空的）
pub fn parse(shared: &str, local: &str, project_dir: &Path, home: &Path) -> Vec<IdeaDs> {
    let users: Vec<(String, String)> = elements(local, "data-source").into_iter().filter_map(|(tag, body)| Some((attr(tag, "uuid")?, child_text(body, "user-name")))).collect();
    let expand = |s: &str| s.replace("$PROJECT_DIR$", &project_dir.to_string_lossy()).replace("$USER_HOME$", &home.to_string_lossy());
    elements(shared, "data-source")
        .into_iter()
        .filter_map(|(tag, body)| {
            let uuid = attr(tag, "uuid").unwrap_or_default();
            let name = attr(tag, "name").unwrap_or_default();
            let url = expand(&child_text(body, "jdbc-url"));
            if url.is_empty() {
                return None;
            }
            let (kind, note) = kind_of(&url);
            let (host, port, database) = host_port_db(&url);
            let user = users.iter().find(|(u, _)| *u == uuid).map(|(_, n)| n.clone()).unwrap_or_default();
            Some(IdeaDs { uuid, name, kind, url, user, host, port, database, note })
        })
        .collect()
}

/// 这个项目里 IDEA 存的数据源：项目根和每个 reactor 目录下的 `.idea/`（同 `ideacfg::read`）。第二个是读不了的文件，一条一句
pub fn read(root: &Path) -> (Vec<IdeaDs>, Vec<String>) {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs = vec![root.to_path_buf()];
    for rel in crate::maven::reactors(root) {
        let d = root.join(rel);
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    let (mut out, mut problems) = (Vec::new(), Vec::new());
    let load = |f: &Path, problems: &mut Vec<String>| -> Option<String> {
        let meta = std::fs::metadata(f).ok()?;
        if meta.len() > MAX_BYTES {
            problems.push(format!("{} 大小为 {} MB，超过 {} MB 上限，未读取", f.display(), meta.len() >> 20, MAX_BYTES >> 20));
            return None;
        }
        match std::fs::read(f) {
            Ok(b) => Some(String::from_utf8_lossy(&b).into_owned()),
            Err(e) => {
                problems.push(format!("无法读取 {}：{e}", f.display()));
                None
            }
        }
    };
    for dir in dirs {
        let idea = dir.join(".idea");
        let Some(shared) = load(&idea.join("dataSources.xml"), &mut problems) else { continue };
        let local = load(&idea.join("dataSources.local.xml"), &mut problems).unwrap_or_default();
        for d in parse(&shared, &local, &dir, &home) {
            if !out.iter().any(|x: &IdeaDs| x.uuid == d.uuid) {
                out.push(d);
            }
        }
    }
    (out, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="DataSourceManagerImpl" format="xml" multifile-model="true">
    <data-source source="LOCAL" name="shop@localhost" uuid="u-1">
      <driver-ref>mysql.8</driver-ref>
      <synchronize>true</synchronize>
      <jdbc-driver>com.mysql.cj.jdbc.Driver</jdbc-driver>
      <jdbc-url>jdbc:mysql://localhost:3306/shop?useSSL=false&amp;serverTimezone=Asia/Shanghai</jdbc-url>
      <working-dir>$ProjectFileDir$</working-dir>
    </data-source>
    <data-source source="LOCAL" name="本地 H2" uuid="u-2">
      <driver-ref>h2.unified</driver-ref>
      <jdbc-url>jdbc:h2:$PROJECT_DIR$/data/app</jdbc-url>
    </data-source>
    <data-source source="LOCAL" name="报表库" uuid="u-3">
      <jdbc-url>jdbc:oracle:thin:@db.example:1521/ORCL</jdbc-url>
    </data-source>
    <data-source source="LOCAL" name="pg" uuid="u-4">
      <jdbc-url>jdbc:postgresql://pg1:5432,pg2:5432/orders</jdbc-url>
    </data-source>
  </component>
</project>"#;
    const LOCAL: &str = r#"<project version="4"><component name="dataSourceStorageLocal">
    <data-source name="shop@localhost" uuid="u-1"><database-info product="MySQL" version="8.4.3"/><user-name>root</user-name></data-source>
    <data-source name="pg" uuid="u-4"><user-name>app</user-name></data-source>
  </component></project>"#;

    #[test]
    fn 连接串_用户名按_uuid_对上_实体解码_路径变量换回来() {
        let got = parse(SHARED, LOCAL, Path::new("/work/shop"), Path::new("/Users/me"));
        assert_eq!(got.len(), 4);
        let m = &got[0];
        assert_eq!((m.name.as_str(), m.kind, m.user.as_str()), ("shop@localhost", Some("mysql"), "root"));
        assert_eq!(m.url, "jdbc:mysql://localhost:3306/shop?useSSL=false&serverTimezone=Asia/Shanghai", "&amp; 要解回 &，连接串的参数一个不丢");
        assert_eq!((m.host.as_str(), m.port, m.database.as_str()), ("localhost", 3306, "shop"));
        assert_eq!(got[1].url, "jdbc:h2:/work/shop/data/app", "$PROJECT_DIR$ 换回项目目录");
        assert_eq!(got[1].user, "", "local 里没有它：用户名是空的");
        assert_eq!(got[2].kind, None);
        assert!(got[2].note.as_deref().unwrap_or("").contains("oracle"), "认不得的说清是什么库：{:?}", got[2].note);
        assert_eq!((got[3].host.as_str(), got[3].port, got[3].database.as_str(), got[3].user.as_str()), ("pg1", 5432, "orders", "app"), "多主机取第一个给树里显示");
    }

    #[test]
    fn 没有本机那份_照样读_mariadb_说一句() {
        let x = r#"<data-source name="m" uuid="a"><jdbc-url>jdbc:mariadb://db:3307/x</jdbc-url></data-source><data-source name="空" uuid="b"><jdbc-url></jdbc-url></data-source>"#;
        let got = parse(x, "", Path::new("/p"), Path::new("/h"));
        assert_eq!(got.len(), 1, "没有连接串的跳过");
        assert_eq!((got[0].kind, got[0].port), (Some("mysql"), 3307));
        assert!(got[0].note.as_deref().unwrap_or("").contains("MariaDB"));
    }
}
