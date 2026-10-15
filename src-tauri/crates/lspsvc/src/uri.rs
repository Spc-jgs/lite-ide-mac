//! 路径 ↔ `file://` URI。LSP 里文件一律用 URI 说，我们这边（编辑器的标签、文件树）一律用路径。
//!
//! 两个容易错的地方：
//!
//! - **要百分号编码**：项目路径里有中文、空格是常事（`~/工作/订单 服务`）。URI 里这些字符要按 UTF-8 字节写成 `%E5%B7%A5`，
//!   不编码的话服务器那头解析出来的路径对不上，报错回来也对不上我们开着的那个标签。
//! - **jdtls 发回来的不一定是 `file:///`**：它是 Java 写的，`java.io.File.toURI()` 给的是单斜杠的 `file:/Users/...`；
//!   有的地方中文原样、有的地方编码。所以解码两种都认，比较一律先解成路径再比，不比 URI 字符串。

/// 路径 → `file:///…`。只留 RFC 3986 的「不用编码」那几类和 `/`，别的按 UTF-8 字节编码
pub fn from_path(path: &str) -> String {
    let mut out = String::from("file://");
    if !path.starts_with('/') {
        out.push('/');
    }
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `file:` URI → 路径。不是 `file:` 的（`jdt://contents/…` 这种 jar 里的类）给 `None`。
/// 编码坏了（半截的 `%E5`、解出来不是 UTF-8）也给 `None`：猜一个路径出来比说「不认得」糟
pub fn to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file:")?;
    // file:///a、file:/a、file://localhost/a 都是 /a
    let rest = rest.strip_prefix("//localhost").or_else(|| rest.strip_prefix("//")).unwrap_or(rest);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let p = String::from_utf8(out).ok()?;
    p.starts_with('/').then_some(p)
}

/// jar 里的类（`jdt://contents/<jar>/<包>/<类>.class?=<项目>/…`）→ `(全名, jar 名)`，给标签和跳转列表看。
/// `?` 后面那段是 jdtls 自己用的（哪个项目、classpath 里哪一项），带着斜杠，不能参与切分。
/// **依赖带源码 jar 时末尾是 `.java` 不是 `.class`**（真 jdtls 回的，2026-10-15 验收撞上：只认 `.class` 时标签成了 `class.class`）
pub fn jdt_label(uri: &str) -> Option<(String, String)> {
    let path = uri.strip_prefix("jdt://contents/")?.split('?').next()?;
    let mut seg = path.split('/');
    let (jar, pkg, class) = (seg.next()?, seg.next()?, seg.next()?);
    let class = class.strip_suffix(".class").or_else(|| class.strip_suffix(".java"))?;
    let dec = |s: &str| to_path(&format!("file:///{s}")).map(|p| p[1..].to_string());
    let (pkg, class, jar) = (dec(pkg)?, dec(class)?, dec(jar)?);
    Some((if pkg.is_empty() { class } else { format!("{pkg}.{class}") }, jar))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 中文和空格要编码_来回不变() {
        let p = "/Users/u/工作/订单 服务/src/Main.java";
        let u = from_path(p);
        assert_eq!(u, "file:///Users/u/%E5%B7%A5%E4%BD%9C/%E8%AE%A2%E5%8D%95%20%E6%9C%8D%E5%8A%A1/src/Main.java");
        assert_eq!(to_path(&u).as_deref(), Some(p));
        assert_eq!(from_path("/a/b#c%d.java"), "file:///a/b%23c%25d.java", "# 和 % 本身也要编码，否则解回来就变了");
        assert_eq!(to_path(&from_path("/a/b#c%d.java")).as_deref(), Some("/a/b#c%d.java"));
    }

    #[test]
    fn jdtls_回来的几种写法都认() {
        assert_eq!(to_path("file:/Users/u/a.java").as_deref(), Some("/Users/u/a.java"), "Java 的 File.toURI() 是单斜杠");
        assert_eq!(to_path("file:///Users/u/工作/a.java").as_deref(), Some("/Users/u/工作/a.java"), "中文原样没编码");
        assert_eq!(to_path("file://localhost/a.java").as_deref(), Some("/a.java"));
    }

    #[test]
    fn jar_里的类_取全名和_jar_名_问号后面的斜杠不参与() {
        // 第 0 步在 petclinic 上 jdtls 实际回来的样子（项目路径换成了假的）
        let u = "jdt://contents/spring-data-commons-4.0.1.jar/org.springframework.data.repository/CrudRepository.class?=customers-service/%2FUsers%2Fu%2Frepository%2Forg%2Fspringframework%2Fdata%2Fspring-data-commons%2F4.0.1%2Fspring-data-commons-4.0.1.jar=/maven.pomderived=/true=/=/test=/false=/%3Corg.springframework.data.repository%28CrudRepository.class";
        assert_eq!(jdt_label(u), Some(("org.springframework.data.repository.CrudRepository".into(), "spring-data-commons-4.0.1.jar".into())));
        assert_eq!(jdt_label("jdt://contents/rt.jar/java.util/Map$Entry.class"), Some(("java.util.Map$Entry".into(), "rt.jar".into())));
        // 有源码 jar 时 jdtls 回的样子（2026-10-15 真 jdtls，路径换成假的）：末尾是 .java
        let src = "jdt://contents/spring-boot-4.0.1.jar/org.springframework.boot/SpringApplication.java?=order-service/%5C/Users%5C/u%5C/repo%5C/spring-boot-4.0.1.jar=/maven.pomderived=/true=/%3Corg.springframework.boot%28SpringApplication.class";
        assert_eq!(jdt_label(src), Some(("org.springframework.boot.SpringApplication".into(), "spring-boot-4.0.1.jar".into())));
        assert_eq!(jdt_label("file:///a.java"), None);
    }

    #[test]
    fn 不是文件的和编码坏的_不猜() {
        assert_eq!(to_path("jdt://contents/x.jar/org.y/Z.class?=p"), None);
        assert_eq!(to_path("file:///a/%E5"), None, "半截的 UTF-8");
        assert_eq!(to_path("file:///a/%zz"), None);
    }
}
