//! MyBatis 的 Mapper 接口 ↔ XML 跳转（docs/IDEA.md 第 24 节，IDEA 的 MyBatisX 插件那套，Lithe-IDEA 也做了）。
//!
//! 规则是 MyBatis 自己的：XML 里 `<mapper namespace="接口全名">`，`<select|insert|update|delete id="方法名">` 对上接口里同名的方法。
//! **不靠语言服务器**（docs/IDEA.md 第 3 节「秒开不等」）：两边都是读文本 —— 打开文件当场就有，不用等 jdtls 导入。
//!
//! 两边的解析都在这里，前端只画图标、点了跳（同一套规则写两份，早晚有一边少一条）：
//! - [`statements`]：XML 里的那几段语句，带行号。注释里的不算
//! - [`methods`]：接口体第一层的方法声明，带行号。用 `javalite` 的分词：跳过字符串、注释、注解的括号，只认「类型 名字(」——
//!   `default` 方法体里的调用、`@Param("id")` 都不会被当成声明（正则做不到这件事：它不知道自己在第几层括号里）
//!
//! 用注解写 SQL 的（`@Select("…")`）SQL 就在方法上，不用跳；MyBatis-Plus 的通用方法（`selectById`）XML 里没有，没配上的不画。

use crate::javalite::{after_type, tokens};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub id: String,
    /// select / insert / update / delete
    pub kind: String,
    /// 从 1 数
    pub line: u32,
}

/// 注释换成空格（换行留着，行号不变）
fn blank_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find("-->").map_or(rest.len(), |j| i + j + 3);
        out.extend(rest[i..end].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// 标签文本（`<select id="x" resultType="…"`）里一个属性的值。属性名前面要是空白（`resultId` 里的 `Id` 不算）
fn attr(tag: &str, name: &str) -> Option<String> {
    let b = tag.as_bytes();
    let mut from = 0;
    while let Some(i) = tag[from..].find(name).map(|i| from + i) {
        from = i + name.len();
        if i == 0 || !b[i - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let q = rest.chars().next()?;
        if q != '"' && q != '\'' {
            continue;
        }
        let end = rest[1..].find(q)?;
        return Some(rest[1..1 + end].trim().to_string());
    }
    None
}

/// XML 的 `<mapper namespace="…">`。不是 Mapper XML 给 None
pub fn namespace(xml: &str) -> Option<String> {
    let s = blank_comments(xml);
    let i = s.find("<mapper")?;
    let tag = &s[i..i + s[i..].find('>')?];
    attr(tag, "namespace").filter(|n| !n.is_empty())
}

/// XML 里的语句：`<select|insert|update|delete … id="…">`，带行号
pub fn statements(xml: &str) -> Vec<Statement> {
    let s = blank_comments(xml);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = s[from..].find('<').map(|i| from + i) {
        from = i + 1;
        let rest = &s[i + 1..];
        let Some(kind) = ["select", "insert", "update", "delete"].into_iter().find(|k| rest.starts_with(k)) else { continue };
        // `<selectKey>` 不是 `<select>`：名字后面得是空白、`>` 或 `/`。防着的 —— MyBatis 的 selectKey 不带 id，实际撞不上，测试证明不了它
        if !rest[kind.len()..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        if let Some(id) = attr(&rest[..end], "id").filter(|x| !x.is_empty()) {
            out.push(Statement { id, kind: kind.to_string(), line: s[..i].matches('\n').count() as u32 + 1 });
        }
    }
    out
}

/// Java 源码里的接口全名（`package` + 第一个顶层 `interface`）。不是接口给 None
pub fn interface_name(java: &str) -> Option<String> {
    let toks = tokens(java);
    let pkg = toks.iter().position(|t| t.text == "package" && t.depth == 0).map(|i| {
        toks[i + 1..].iter().take_while(|t| t.text != ";").map(|t| t.text.as_str()).collect::<String>()
    });
    // `@interface`（注解的定义）分词时是一个注解记号、不是标识符，这里自然认不上 —— 它不是 Mapper
    let at = toks.iter().position(|t| t.ident && t.text == "interface" && t.depth == 0 && t.paren == 0)?;
    let name = &toks.get(at + 1)?.text;
    Some(match pkg {
        Some(p) if !p.is_empty() => format!("{p}.{name}"),
        _ => name.clone(),
    })
}

/// 接口体第一层的方法声明：(方法名, 行号)
pub fn methods(java: &str) -> Vec<(String, u32)> {
    let toks = tokens(java);
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        // 第一层花括号（接口体）、不在括号里、后面紧跟 `(`、前面是一个类型（标识符、`>` 泛型、`]` 数组）
        if t.depth != 1 || t.paren != 0 || !t.ident || toks.get(i + 1).is_none_or(|n| n.text != "(") {
            continue;
        }
        let Some(prev) = i.checked_sub(1).map(|p| &toks[p]) else { continue };
        if after_type(prev) {
            out.push((t.text.clone(), t.line));
        }
    }
    out
}

/// 项目里 namespace 是 `fqn` 的那几份 XML（相对路径）。走项目搜索那条路（有 rg 用 rg），跳过构建产物
pub fn find_xml(root: &Path, fqn: &str, skip: &searchsvc::Skip) -> Vec<String> {
    let pattern = format!(r#"namespace\s*=\s*["']{}["']"#, fqn.replace('.', r"\.").replace('$', r"\$"));
    let q = searchsvc::Query { pattern, case: true, word: false, regex: true };
    let mut out: Vec<String> = searchsvc::grep(root, &q, 50, skip).unwrap_or_default().into_iter().map(|h| h.path).filter(|p| p.ends_with(".xml")).collect();
    out.sort();
    out.dedup();
    out
}

/// 接口全名 → 项目里那个文件（相对路径）：`…/包路径/名字.java`，`src/main/java` 底下的优先（测试里同名的排后面）
pub fn find_java(files: &[String], fqn: &str) -> Option<String> {
    let tail = format!("/{}.java", fqn.replace('.', "/"));
    let mut hits: Vec<&String> = files.iter().filter(|f| f.ends_with(&tail) || format!("/{f}").ends_with(&tail)).collect();
    hits.sort_by_key(|f| !f.contains("src/main/java"));
    hits.first().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE mapper PUBLIC "-//mybatis.org//DTD Mapper 3.0//EN" "http://mybatis.org/dtd/mybatis-3-mapper.dtd">
<mapper namespace="com.demo.order.mapper.OrderMapper">
    <resultMap id="base" type="Order"><id column="id" property="id"/></resultMap>
    <!-- <select id="old">SELECT 1</select> 注释掉的不算 -->
    <select id="findById" resultMap="base">
        SELECT * FROM t_order WHERE id = #{id}
    </select>
    <insert id='insert' useGeneratedKeys="true">
        <selectKey keyProperty="id" resultType="long">SELECT LAST_INSERT_ID()</selectKey>
        INSERT INTO t_order(sku) VALUES (#{sku})
    </insert>
    <update
        id="updateStatus">UPDATE t_order SET status = #{s}</update>
    <delete id="deleteById">DELETE FROM t_order WHERE id = #{id} AND a < 3</delete>
</mapper>
"#;

    #[test]
    fn xml_的语句_带行号_注释里的不算_属性跨行也认() {
        assert_eq!(namespace(XML).as_deref(), Some("com.demo.order.mapper.OrderMapper"));
        let s = statements(XML);
        assert_eq!(
            s.iter().map(|x| (x.id.as_str(), x.kind.as_str(), x.line)).collect::<Vec<_>>(),
            vec![("findById", "select", 6), ("insert", "insert", 9), ("updateStatus", "update", 13), ("deleteById", "delete", 15)]
        );
        assert_eq!(namespace("<beans><bean id=\"x\"/></beans>"), None, "不是 Mapper XML");
    }

    const JAVA: &str = r#"package com.demo.order.mapper;

import org.apache.ibatis.annotations.*;

/** 订单 Mapper：findById(Long) 在注释里不算 */
@Mapper
public interface OrderMapper {
    String SQL = "select count(*) from t_order where id in (select 1)";

    Order findById(@Param("id") Long id);

    int insert(Order o);

    @Update("UPDATE t_order SET status = #{s} WHERE id = (#{id})")
    int updateStatus(@Param("id") Long id,
                     @Param("s") String s);

    List<Order> listByIds(@Param("ids") List<Long> ids);

    Order[] all();

    default int deleteById(Long id) {
        Runnable r = new Runnable() {
            public void run() { System.out.println(id); }
        };
        return helper(id);
    }
}
"#;

    #[test]
    fn 接口的方法_第一层才算_注解括号和字符串和_default_里的匿名类不算() {
        assert_eq!(interface_name(JAVA).as_deref(), Some("com.demo.order.mapper.OrderMapper"));
        assert_eq!(
            methods(JAVA),
            vec![
                ("findById".into(), 10),
                ("insert".into(), 12),
                ("updateStatus".into(), 15),
                ("listByIds".into(), 18),
                ("all".into(), 20),
                ("deleteById".into(), 22)
            ]
        );
        assert_eq!(interface_name("package a; public @interface Mark {}"), None, "注解的定义不是接口");
        assert_eq!(interface_name("package a; public class A {}"), None);
    }

    #[test]
    fn 按全名找接口文件_main_的优先() {
        let files = vec!["svc/src/test/java/com/demo/OrderMapper.java".to_string(), "svc/src/main/java/com/demo/OrderMapper.java".into(), "svc/src/main/java/com/demo/XOrderMapper.java".into()];
        assert_eq!(find_java(&files, "com.demo.OrderMapper").as_deref(), Some("svc/src/main/java/com/demo/OrderMapper.java"));
        assert_eq!(find_java(&files, "com.other.OrderMapper"), None);
    }
}
