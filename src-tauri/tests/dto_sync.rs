//! IPC 两侧的 DTO 必须逐字段对得上。
//!
//! ARCHITECTURE.md §4 原本写的是「用 `ts-rs` 导出类型到 `src/lib/ipc/types.ts`」——
//! 那套东西从来没落地过，两侧一直是手写两遍。今天量下来它们碰巧还是一致的，
//! 但「碰巧一致」不是一种能维持下去的状态：漏改一侧不会有任何报错，
//! 只会在运行时变成一个 `undefined`，而 `undefined` 在界面上通常表现为
//! **一片空白**，没人会往类型上想。
//!
//! 这里不上 ts-rs（要给 crate 加依赖、加生成步骤、还要把生成物提交进仓库），
//! 改成一条**只读源码的测试**：解析 `commands.rs` 里带 `#[derive(serde::Serialize)]`
//! 的结构体和 `commands.ts` 里的 `export interface`，比字段名集合。
//! 造价是这一个文件，拦住的是同一类 bug。

use std::collections::BTreeSet;

const RUST: &str = include_str!("../src/commands.rs");
const TS: &str = include_str!("../../src/lib/ipc/commands.ts");

/// Rust 结构体 ↔ TS 接口的对应关系。
///
/// 刻意手写而不是靠命名规则猜：两侧的名字本来就不一样（`StatDto` ↔ `LogStat`），
/// 而且新加一个 DTO 时**必须**在这里登记一行 —— 忘了登记测试就红，
/// 这正是我们要的那道提醒。
const PAIRS: &[(&str, &str)] = &[
    ("OpenResult", "OpenResult"),
    ("StatDto", "LogStat"),
    ("FilterStatDto", "FilterStat"),
    ("RefreshDto", "RefreshResult"),
    ("PathInfo", "PathInfo"),
    ("DirEntryDto", "DirEntry"),
    ("TextDto", "TextFile"),
    ("StampDto", "Stamp"),
    ("HitDto", "Hit"),
    ("GitEntryDto", "GitEntry"),
    ("GitStatusDto", "GitStatus"),
    ("DiffDto", "DiffText"),
    ("LogEntryDto", "GitLogEntry"),
    ("BranchDto", "GitBranch"),
    ("WorktreeDto", "GitWorktree"),
    ("ProgressDto", "RemoteProgress"),
    ("RemoteErrDto", "RemoteErr"),
];

struct RustStruct {
    name: String,
    fields: BTreeSet<String>,
    camel: bool,
}

/// 扫出所有 `#[derive(serde::Serialize)] … pub struct X { … }`。
fn rust_structs() -> Vec<RustStruct> {
    let lines: Vec<&str> = RUST.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("#[derive(serde::Serialize") {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut camel = false;
        while j < lines.len() && lines[j].starts_with("#[") {
            if lines[j].contains("rename_all = \"camelCase\"") {
                camel = true;
            }
            j += 1;
        }
        let Some(name) = lines
            .get(j)
            .and_then(|l| l.strip_prefix("pub struct "))
            .and_then(|l| l.strip_suffix(" {"))
        else {
            i = j + 1;
            continue;
        };
        let mut fields = BTreeSet::new();
        let mut k = j + 1;
        while k < lines.len() && lines[k] != "}" {
            let t = lines[k].trim();
            if let Some(rest) = t.strip_prefix("pub ") {
                if let Some((f, _)) = rest.split_once(':') {
                    fields.insert(to_camel(f.trim()));
                }
            }
            k += 1;
        }
        out.push(RustStruct {
            name: name.to_string(),
            fields,
            camel,
        });
        i = k + 1;
    }
    out
}

/// 扫出 `export interface X { … }` 的字段名。注释与嵌套的块注释都要跳过。
fn ts_interface(name: &str) -> Option<BTreeSet<String>> {
    let head = format!("export interface {name} {{");
    let start = TS.find(&head)? + head.len();
    let body = &TS[start..];
    let end = body.find("\n}")?;
    let mut fields = BTreeSet::new();
    let mut in_block = false;
    for line in body[..end].lines() {
        let t = line.trim();
        if in_block {
            if t.contains("*/") {
                in_block = false;
            }
            continue;
        }
        if t.starts_with("/*") {
            in_block = !t.contains("*/");
            continue;
        }
        if t.starts_with("//") || t.starts_with('*') || t.is_empty() {
            continue;
        }
        if let Some((f, _)) = t.split_once(':') {
            fields.insert(f.trim().trim_end_matches('?').to_string());
        }
    }
    Some(fields)
}

fn to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn ipc两侧的dto字段必须一致() {
    let structs = rust_structs();
    assert!(
        structs.len() >= PAIRS.len(),
        "只扫到 {} 个 Serialize 结构体，比登记的 {} 还少 —— 解析逻辑坏了",
        structs.len(),
        PAIRS.len()
    );

    for s in &structs {
        let Some((_, ts_name)) = PAIRS.iter().find(|(r, _)| *r == s.name) else {
            panic!(
                "`{}` 没有登记到 PAIRS 里。新加一个过 IPC 的 DTO，\
                 必须同时写好 TS 侧的 interface 并在这里登记，否则两边迟早漂移",
                s.name
            );
        };

        // 少了 rename_all 的话，`line_count` 会原样序列化成 snake_case，
        // 而 TS 侧写的是 lineCount —— 单词字段看不出来，两词字段直接 undefined
        assert!(
            s.camel,
            "`{}` 缺 #[serde(rename_all = \"camelCase\")]",
            s.name
        );

        let ts = ts_interface(ts_name)
            .unwrap_or_else(|| panic!("TS 侧找不到 `export interface {ts_name}`"));

        let only_rust: Vec<_> = s.fields.difference(&ts).collect();
        let only_ts: Vec<_> = ts.difference(&s.fields).collect();
        assert!(
            only_rust.is_empty() && only_ts.is_empty(),
            "{} ↔ {ts_name} 字段对不上：\n  只在 Rust：{only_rust:?}\n  只在 TS：{only_ts:?}",
            s.name
        );
    }
}
