//! 跨文件替换的验收（docs/REPLACE.md 第 11 节）里 Rust 侧能验的那几条，加上两段提交和崩溃恢复。
//! 每条测试一个独立的临时目录（名字带进程号），失败时 panic 在清理之前也不会污染下一次。

use replacesvc::*;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn sandbox(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("replacesvc-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("proj")).unwrap();
    d
}

/// 列项目里的文件（测试里不走 searchsvc 的跳过规则，那条在 searchsvc 自己测）
fn files(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let ft = fs::symlink_metadata(&p).unwrap().file_type();
            if ft.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn lit(p: &str) -> Query {
    Query::literal(p)
}

fn scan_all(root: &Path, q: &Query) -> Scan {
    scan(root, &files(root), false, q, &HashMap::new()).unwrap()
}

/// 全选：每个文件的每一处
fn all(s: &Scan) -> Vec<Pick> {
    s.files.iter().map(|f| Pick { rel: f.rel.clone(), hits: (0..f.hits.len()).collect() }).collect()
}

fn opts<'a>(j: &'a Path, repl: &'a str, picks: &'a [Pick], dirty: &'a HashMap<PathBuf, String>) -> ApplyOpts<'a> {
    ApplyOpts { journal: j, replacement: repl, picks, dirty_now: dirty, allow_no_undo: false, crash_after: None, journal_cap: None }
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

/// 目录底下有没有我们的临时文件残留
fn leftovers(dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    for f in files(dir) {
        if f.contains(".lite-ide-tmp") {
            v.push(f);
        }
    }
    v
}

#[test]
fn 预览六处_取消勾一处_改五处() {
    // 验收 1：三个文件里各有两处 OrderClient
    let d = sandbox("pick");
    let root = d.join("proj");
    for n in ["A.java", "B.java", "C.java"] {
        fs::write(root.join(n), "OrderClient a;\nOrderClient b;\n").unwrap();
    }
    let s = scan_all(&root, &lit("OrderClient"));
    assert_eq!(s.total, 6);
    let after = s.after("OrderGateway");
    assert_eq!(after[0][0].text, "OrderGateway a;", "预览里的改后那一行");
    assert_eq!(after[0][0].spans, vec![[0, 12]]);
    let mut picks = all(&s);
    picks[1].hits = vec![1]; // B.java 只改第二处
    let j = d.join("journal");
    let none = HashMap::new();
    let out = apply(&s, &opts(&j, "OrderGateway", &picks, &none)).unwrap();
    assert_eq!(out.changed.iter().map(|c| c.count).sum::<usize>(), 5);
    assert_eq!(read(&root.join("A.java")), "OrderGateway a;\nOrderGateway b;\n");
    assert_eq!(read(&root.join("B.java")), "OrderClient a;\nOrderGateway b;\n", "取消勾的那处没动");
    assert!(out.undo);
    assert!(leftovers(&root).is_empty());
    fs::remove_dir_all(d).ok();
}

#[test]
fn 权限_软链_硬链接_编码_换行符_bom_都原样() {
    // 验收 3 + 三条保存语义 + 编码 / 换行 / BOM：替换走的是 fsservice 的同一条写盘路
    let d = sandbox("keep");
    let root = d.join("proj");
    fs::write(root.join("run.sh"), "echo OLD\n").unwrap();
    fs::set_permissions(root.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    // 软链指向项目外的真身：写进真身，链接还是链接
    fs::write(d.join("real.txt"), "OLD real\n").unwrap();
    std::os::unix::fs::symlink(d.join("real.txt"), root.join("link.txt")).unwrap();
    // 硬链接：项目里一份、项目外一份，改完两份一起变
    fs::write(root.join("hard.txt"), "OLD hard\n").unwrap();
    fs::hard_link(root.join("hard.txt"), d.join("hard-outside.txt")).unwrap();
    // GBK：「旧」= c9 ca
    fs::write(root.join("gbk.txt"), [0xc9, 0xca, b' ', b'O', b'L', b'D', b'\n', 0xd6, 0xd0, 0xce, 0xc4, b'\n']).unwrap();
    fs::write(root.join("crlf.txt"), "a OLD\r\nb\r\n").unwrap();
    fs::write(root.join("bom.txt"), b"\xef\xbb\xbfOLD\n").unwrap();

    let s = scan_all(&root, &lit("OLD"));
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    apply(&s, &opts(&j, "NEW", &picks, &none)).unwrap();

    assert_eq!(read(&root.join("run.sh")), "echo NEW\n");
    assert_eq!(fs::metadata(root.join("run.sh")).unwrap().permissions().mode() & 0o777, 0o755, "执行权限还在");
    assert!(fs::symlink_metadata(root.join("link.txt")).unwrap().file_type().is_symlink(), "软链还是软链");
    assert_eq!(read(&d.join("real.txt")), "NEW real\n", "改动写进了真身");
    assert_eq!(read(&d.join("hard-outside.txt")), "NEW hard\n", "硬链接组没被拆开");
    let gbk = fs::read(root.join("gbk.txt")).unwrap();
    assert_eq!(&gbk[..3], &[0xc9, 0xca, b' '], "GBK 写回去还是 GBK");
    assert!(gbk.windows(3).any(|w| w == b"NEW"));
    assert_eq!(read(&root.join("crlf.txt")), "a NEW\r\nb\r\n", "CRLF 没被改成 LF");
    assert_eq!(fs::read(root.join("bom.txt")).unwrap(), b"\xef\xbb\xbfNEW\n", "BOM 还在");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 软链和真身都在结果里时只替换一次() {
    // 验收 5：不去重的话第二次核对指纹时那份已经变了，被报成「预览之后被改过」
    let d = sandbox("dedupe");
    let root = d.join("proj");
    fs::create_dir_all(root.join("real")).unwrap();
    fs::write(root.join("real/config.txt"), "port=OLD\n").unwrap();
    std::os::unix::fs::symlink("real/config.txt", root.join("link.txt")).unwrap();
    let s = scan_all(&root, &lit("OLD"));
    assert_eq!(s.files.len(), 1);
    assert_eq!(s.files[0].rel, "real/config.txt", "留的是真身那条路径");
    assert!(s.skipped.iter().any(|k| k.rel == "link.txt" && k.why == Why::Same { as_rel: "real/config.txt".into() }));
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    let out = apply(&s, &opts(&j, "NEW", &picks, &none)).unwrap();
    assert!(out.skipped.is_empty(), "{:?}", out.skipped);
    assert_eq!(read(&root.join("real/config.txt")), "port=NEW\n");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 预览之后改过的跳过_其余照常() {
    // 验收 6
    let d = sandbox("changed");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "OLD\n").unwrap();
    fs::write(root.join("b.txt"), "OLD\n").unwrap();
    let s = scan_all(&root, &lit("OLD"));
    fs::write(root.join("b.txt"), "OLD 外面改了一笔\n").unwrap();
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    let out = apply(&s, &opts(&j, "NEW", &picks, &none)).unwrap();
    assert_eq!(read(&root.join("a.txt")), "NEW\n");
    assert_eq!(read(&root.join("b.txt")), "OLD 外面改了一笔\n", "没在一份没给人看过的内容上动手");
    assert!(out.skipped.iter().any(|k| k.rel == "b.txt" && k.why == Why::Changed));
    fs::remove_dir_all(d).ok();
}

#[test]
fn 有未保存改动的标签只改编辑器_盘上不动() {
    // 验收 2：预览和执行都用编辑器里那份；给回编辑器要做的改动（UTF-16），盘上那份不写
    let d = sandbox("dirty");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "盘上的 OLD\n").unwrap();
    let mut dirty = HashMap::new();
    dirty.insert(root.join("a.txt"), "😀编辑器里的 OLD 还没存\n".to_string());
    let s = scan(&root, &files(&root), false, &lit("OLD"), &dirty).unwrap();
    assert!(matches!(s.files[0].source, Source::Editor));
    let j = d.join("journal");
    let picks = all(&s);
    let out = apply(&s, &opts(&j, "NEW", &picks, &dirty)).unwrap();
    assert_eq!(read(&root.join("a.txt")), "盘上的 OLD\n", "盘上那份没被覆盖");
    let c = &out.changed[0];
    assert!(!c.on_disk);
    // 😀 是两个 UTF-16 单位：「😀编辑器里的 」= 2 + 6 = 8
    assert_eq!(c.edits, vec![Edit { from: 8, to: 11, insert: "NEW".into() }]);
    fs::remove_dir_all(d).ok();
}

#[test]
fn 撤销_回到原样_替换之后又改过的跳过() {
    // 验收 4
    let d = sandbox("undo");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "OLD a\n").unwrap();
    fs::write(root.join("b.txt"), "OLD b\n").unwrap();
    fs::write(root.join("c.txt"), "OLD c\n").unwrap();
    let s = scan_all(&root, &lit("OLD"));
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    apply(&s, &opts(&j, "NEW", &picks, &none)).unwrap();
    fs::write(root.join("c.txt"), "NEW c 替换之后我又改了\n").unwrap();
    let out = undo(&j, &HashMap::new()).unwrap();
    assert_eq!(read(&root.join("a.txt")), "OLD a\n");
    assert_eq!(read(&root.join("b.txt")), "OLD b\n");
    assert_eq!(read(&root.join("c.txt")), "NEW c 替换之后我又改了\n", "不盲写");
    assert!(out.skipped.iter().any(|k| k.rel == "c.txt" && k.why == Why::EditedSince));
    assert!(journal::load(&j).is_none(), "撤销完日志就没了");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 重启之后还能撤销() {
    // 验收 7 的 Rust 那半：日志在盘上，扫描结果、内存里的东西全丢了也撤得了
    let d = sandbox("restart");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "OLD\n").unwrap();
    let j = d.join("journal");
    {
        let s = scan_all(&root, &lit("OLD"));
        let none = HashMap::new();
        let picks = all(&s);
        apply(&s, &opts(&j, "NEW", &picks, &none)).unwrap();
    }
    let p = pending(&j).expect("日志在");
    assert_eq!(p.state, State::Done);
    undo(&j, &HashMap::new()).unwrap();
    assert_eq!(read(&root.join("a.txt")), "OLD\n");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 开着的干净标签撤销时编辑器和盘一起退回() {
    let d = sandbox("undo-open");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "OLD\n").unwrap();
    let s = scan_all(&root, &lit("OLD"));
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    apply(&s, &opts(&j, "NEW", &picks, &none)).unwrap();
    let mut open = HashMap::new();
    open.insert(root.join("a.txt"), OpenNow { text: "NEW\n".into(), dirty: false });
    let out = undo(&j, &open).unwrap();
    assert_eq!(read(&root.join("a.txt")), "OLD\n");
    assert!(out.changed[0].on_disk);
    assert_eq!(out.changed[0].edits, vec![Edit { from: 0, to: 4, insert: "OLD\n".into() }]);
    fs::remove_dir_all(d).ok();
}

#[test]
fn 准备段失败_一个原文件都没动() {
    // 12.2：b 所在的目录不可写，建不了临时文件。a 先准备好了也不能换上去
    let d = sandbox("prep-fail");
    let root = d.join("proj");
    fs::create_dir_all(root.join("ro")).unwrap();
    fs::write(root.join("a.txt"), "OLD\n").unwrap();
    fs::write(root.join("ro/b.txt"), "OLD\n").unwrap();
    let s = scan_all(&root, &lit("OLD"));
    fs::set_permissions(root.join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    let r = apply(&s, &opts(&j, "NEW", &picks, &none));
    fs::set_permissions(root.join("ro"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(r, Err(ApplyError::Failed { ref rel, .. }) if rel == "ro/b.txt"), "{r:?}");
    assert_eq!(read(&root.join("a.txt")), "OLD\n", "准备好的那个也没换上去");
    assert!(leftovers(&root).is_empty(), "临时文件收干净了：{:?}", leftovers(&root));
    assert!(journal::load(&j).is_none(), "日志丢掉了");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 提交段中途崩溃_重启后退回成全部改前() {
    // 12.2：提交了 1 个之后进程没了。剩下的临时文件还在、日志停在「提交中」
    let d = sandbox("crash");
    let root = d.join("proj");
    for n in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.join(n), format!("OLD {n}\n")).unwrap();
    }
    let s = scan_all(&root, &lit("OLD"));
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    let mut o = opts(&j, "NEW", &picks, &none);
    o.crash_after = Some(1);
    assert!(apply(&s, &o).is_err());
    let changed_now = ["a.txt", "b.txt", "c.txt"].iter().filter(|n| read(&root.join(n)).starts_with("NEW")).count();
    assert_eq!(changed_now, 1, "崩溃时改了一个");
    assert_eq!(leftovers(&root).len(), 2, "另外两个的临时文件还在");

    // 「下次启动」
    let p = pending(&j).expect("日志在");
    assert_eq!((p.state, p.files, p.changed), (State::Committing, 3, 1), "卡片上说：已改 1 个、共 3 个");
    // 中断没收拾之前，不许开始新的替换（会把这份日志盖掉）
    let s2 = scan_all(&root, &lit("OLD"));
    let picks2 = all(&s2);
    assert_eq!(apply(&s2, &opts(&j, "X", &picks2, &none)).err(), Some(ApplyError::Pending));

    recover(&j, true).unwrap();
    for n in ["a.txt", "b.txt", "c.txt"] {
        assert_eq!(read(&root.join(n)), format!("OLD {n}\n"), "{n} 退回到改前");
    }
    assert!(leftovers(&root).is_empty(), "残留的临时文件收掉了");
    assert!(pending(&j).is_none());
    fs::remove_dir_all(d).ok();
}

#[test]
fn 提交段中途崩溃_选保留现状之后还能撤销() {
    let d = sandbox("crash-keep");
    let root = d.join("proj");
    for n in ["a.txt", "b.txt"] {
        fs::write(root.join(n), "OLD\n").unwrap();
    }
    let s = scan_all(&root, &lit("OLD"));
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    let mut o = opts(&j, "NEW", &picks, &none);
    o.crash_after = Some(1);
    let _ = apply(&s, &o);
    recover(&j, false).unwrap();
    assert!(leftovers(&root).is_empty());
    assert_eq!(pending(&j).unwrap().state, State::Done, "保留现状 = 当作做完了");
    // 改了的那个能撤回去；没改的那个此刻不是「改后」，跳过
    let out = undo(&j, &HashMap::new()).unwrap();
    assert_eq!(read(&root.join("a.txt")), "OLD\n");
    assert_eq!(read(&root.join("b.txt")), "OLD\n");
    assert_eq!(out.skipped.len(), 1);
    fs::remove_dir_all(d).ok();
}

#[test]
fn 有损_混用换行符_只在有命中时才说_二进制只给个数() {
    let d = sandbox("skip");
    let root = d.join("proj");
    // 现实里的「有损」：大半是 GBK 中文（探测会选 GBK），中间夹一个 GBK 里也不合法的 0xFF。
    // 只放几个坏字节的话，探测会选一个单字节编码，每个字节都有字可对，根本不算有损（第一版测试就栽在这儿）
    let gbk_body: Vec<u8> = [0xd6u8, 0xd0, 0xce, 0xc4, 0xb2, 0xe2, 0xca, 0xd4].repeat(40); // 「中文测试」× 40
    let lossy = [&gbk_body[..], b"\nOLD \xff\n"].concat();
    let lossy_nohit = [&gbk_body[..], b"\nnothing \xff\n"].concat();
    assert!(fsservice::decode_bytes(&lossy, "").lossy, "语料本身要真的是有损的，不然这条测试测的不是它说的东西");
    fs::write(root.join("lossy.txt"), &lossy).unwrap();
    fs::write(root.join("lossy-nohit.txt"), &lossy_nohit).unwrap();
    fs::write(root.join("mixed.txt"), "OLD\r\nb\n").unwrap();
    fs::write(root.join("bin.dat"), b"OLD\0\0\0").unwrap();
    let s = scan_all(&root, &lit("OLD"));
    let whys: Vec<(String, Why)> = s.skipped.iter().map(|k| (k.rel.clone(), k.why.clone())).collect();
    assert!(whys.contains(&("mixed.txt".into(), Why::MixedEol)), "{whys:?}");
    assert!(whys.contains(&("lossy.txt".into(), Why::Lossy)), "{whys:?}");
    assert!(!whys.iter().any(|(r, _)| r == "lossy-nohit.txt"), "没命中的不说");
    assert_eq!(s.binary, 1);
    assert!(s.files.is_empty(), "一个都不替换：{:?}", s.files.iter().map(|f| &f.rel).collect::<Vec<_>>());
    fs::remove_dir_all(d).ok();
}

#[test]
fn 命中超过上限不许执行() {
    let d = sandbox("cap");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "x\n".repeat(MAX_HITS + 1)).unwrap();
    let s = scan_all(&root, &lit("x"));
    assert!(s.truncated);
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    assert_eq!(apply(&s, &opts(&j, "y", &picks, &none)).err(), Some(ApplyError::Truncated));
    fs::remove_dir_all(d).ok();
}

#[test]
fn 改动太大撤销不了_要点头才做() {
    let d = sandbox("toobig");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "OLD 0123456789\n").unwrap();
    let s = scan_all(&root, &lit("OLD"));
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    let mut o = opts(&j, "NEW", &picks, &none);
    o.journal_cap = Some(4);
    assert_eq!(apply(&s, &o).err(), Some(ApplyError::TooBigForUndo { bytes: 15 }));
    assert_eq!(read(&root.join("a.txt")), "OLD 0123456789\n");
    o.allow_no_undo = true;
    let out = apply(&s, &o).unwrap();
    assert!(!out.undo, "没有日志，撤销不了");
    assert_eq!(read(&root.join("a.txt")), "NEW 0123456789\n");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 正则替换展开分组_utf16位置对() {
    let d = sandbox("regex");
    let root = d.join("proj");
    fs::write(root.join("a.ts"), "订单 getName(); getAge();\n").unwrap();
    let q = Query { pattern: r"get(\w+)\(".into(), regex: true, ..Default::default() };
    let s = scan_all(&root, &q);
    assert_eq!(s.files[0].hits[0].col, 4, "列是 UTF-16、从 1 起：「订单 」是 3 个单位");
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    let out = apply(&s, &opts(&j, "fetch$1(", &picks, &none)).unwrap();
    assert_eq!(read(&root.join("a.ts")), "订单 fetchName(); fetchAge();\n");
    assert_eq!(out.changed[0].edits[0], Edit { from: 3, to: 11, insert: "fetchName(".into() });
    fs::remove_dir_all(d).ok();
}

#[test]
fn 跨行替换_预览带片段_crlf文件换行符不变() {
    // docs/REPLACE.md 12.3 + 验收 8：`@Autowired\n    private` 这种跨行的词。CRLF 文件里写 \n 一样配得上，写回去还是 CRLF
    let d = sandbox("multiline");
    let root = d.join("proj");
    fs::write(root.join("A.java"), "class A {\r\n    @Autowired\r\n    private Repo repo;\r\n}\r\n").unwrap();
    let q = Query { pattern: r"@Autowired\n\s*private".into(), regex: true, ..Default::default() };
    let s = scan_all(&root, &q);
    assert_eq!(s.total, 1);
    let h = &s.files[0].hits[0];
    assert_eq!((h.line, h.lines), (2, 2), "记在起头那一行，跨 2 行");
    assert_eq!(h.text, "    @Autowired", "列表里显示起头那一行");
    assert_eq!(h.block.as_deref(), Some("    @Autowired\n    private Repo repo;"), "点开看的是整的那两行");
    let after = s.after("private final");
    assert_eq!(after[0][0].block.as_deref(), Some("    private final Repo repo;"), "改后那一块：两行并成了一行");
    let j = d.join("journal");
    let none = HashMap::new();
    let picks = all(&s);
    apply(&s, &opts(&j, "private final", &picks, &none)).unwrap();
    assert_eq!(read(&root.join("A.java")), "class A {\r\n    private final Repo repo;\r\n}\r\n", "换行符还是 CRLF");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 不写换行的词不跨行_和查找一致() {
    // `\s` 配得上换行：不按行找的话 foo 和下一行的 bar 会被连起来替换，而 ⇧⌘F 搜不到它 —— 同一个词两个答案
    let d = sandbox("noml");
    let root = d.join("proj");
    fs::write(root.join("a.txt"), "foo\nbar\nfoo bar\n").unwrap();
    let s = scan_all(&root, &Query { pattern: r"foo\s+bar".into(), regex: true, ..Default::default() });
    assert_eq!(s.total, 1);
    assert_eq!(s.files[0].hits[0].line, 3);
    fs::remove_dir_all(d).ok();
}

#[test]
fn 超长的跨行命中_预览中间折叠() {
    let d = sandbox("fold");
    let root = d.join("proj");
    let body: String = (0..300).map(|i| format!("l{i}\n")).collect();
    fs::write(root.join("a.txt"), format!("BEGIN\n{body}END\n")).unwrap();
    let s = scan_all(&root, &Query { pattern: r"BEGIN\n[\s\S]*?\nEND".into(), regex: true, ..Default::default() });
    let h = &s.files[0].hits[0];
    assert_eq!(h.lines, 302);
    let b = h.block.as_deref().unwrap();
    assert_eq!(b.lines().count(), 41, "头 20 + 一行说明 + 尾 20");
    assert!(b.contains("中间 262 行没显示"), "{b}");
    fs::remove_dir_all(d).ok();
}
