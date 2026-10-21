//! 热替换（docs/IDEA.md 第 26 节）：调试时改了方法体，⌘F9 构建完不重启就生效 —— IDEA 的「Reload Changed Classes」。
//!
//! ```text
//! 调试开始  java … -javaagent:lite-hotswap.jar=<控制目录> …     ← agent（`agent/`，Java 8）在 JVM 里拿到 Instrumentation
//!           JVM 起来那一刻记下盘上每个 class 的指纹（[`Baseline::scan`]）
//! 构建完了  再扫一遍、和指纹比（[`Baseline::changes`]）→ 变了的写成 <控制目录>/<pid>/req
//!           agent 一批换进去（redefineClasses）→ 写回 res → 换成了的记进指纹（[`Baseline::accept`]）
//! ```
//!
//! 几条为什么：
//! - **不用 java-debug 自带的热替换**：它只认 jdtls 自己构建完（`POST_BUILD`）时改过的 class，我们的构建是 Maven 做的（读 0.53.1 的
//!   `JavaHotCodeReplaceProvider` 确认的）。让 jdtls 再编一遍去触发它，就又回到「两个编译器写同一个 target/classes」（第 22 节关掉它的理由）
//! - **不另开 JDWP**：一个 JVM 只能挂一个调试器，java-debug 占着
//! - **比内容不比修改时间**：Maven 一个模块里有改动就整个模块重编，没改的 class 时间也新了。换没改的类不出错，但白做、而且回话里的「换了几个类」就不是人改的那几个
//! - **指纹记在 JVM 起来那一刻**：JVM 里跑的是那时盘上的样子。换成功了才把新指纹记上；没成功（加了方法这类）记着旧的，改回去再构建还能接着换
//! - **按 pid 分子目录**：一个调试任务里可能先后起好几个 JVM（surefire fork），各自一个 agent
//! - **断点**（[`Change::same_lines`]）：JVM 换掉一个类时把这个类里的断点全清了，而调试器那边（java-debug 里的 JDI）不知道 ——
//!   它缓存着这个类的方法和「行号 → 字节码位置」表，那是旧版本的。照旧表往新方法里下断点，位置可能落在一条指令的中间（jdk21u 的
//!   `jvmtiEnv.cpp` 里 `SetBreakpoint` 只查「小于 0」和「越界」，不查是不是指令开头），改坏被调试的程序。所以换之前先让调试器把这些断点删干净（那时类还是旧的，删得对），
//!   换完只有「行号表和 JVM 起来那会儿一字不差」的类才重下：缓存的那张就是对的。变了的，这次调试里它的断点不再停，照实说

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::Hasher;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 一个 class 文件的指纹：长度 + 内容的哈希。只用来比「变没变」，不防人造碰撞
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Print {
    len: u64,
    hash: u64,
}

fn print_of(bytes: &[u8]) -> Print {
    let mut h = DefaultHasher::new();
    h.write(bytes);
    Print { len: bytes.len() as u64, hash: h.finish() }
}

/// 一个变了的 class
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// 类的全名（`com.demo.Money$1`），agent 拿它去找已经加载的类
    pub class: String,
    pub path: PathBuf,
    print: Print,
    /// 每个方法的行号表和 JVM 起来那会儿一字不差（见文件头「断点」）。新出来的类（之前没有）也算：调试器从没见过它，没有旧表
    pub same_lines: bool,
}

/// 项目里所有 Maven 模块的输出目录（`target/classes`、`target/test-classes`）。
/// 不读 pom：模块就是带 pom.xml 的目录，往下找到 `target` 为止 —— 读 `<modules>` 要处理 profile 里的模块、聚合和继承分开的写法，
/// 而「目录里有 pom.xml、旁边有 target」这件事不会错。往下最多 8 层，跳过隐藏目录、`node_modules`、`src`
pub fn class_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_modules(root, 0, &mut out);
    out
}

fn walk_modules(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if dir.join("pom.xml").is_file() {
        for sub in ["target/classes", "target/test-classes"] {
            let d = dir.join(sub);
            if d.is_dir() {
                out.push(d);
            }
        }
    }
    if depth >= 8 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut kids: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.') && !matches!(n, "node_modules" | "src" | "target")))
        .collect();
    kids.sort();
    for k in kids {
        walk_modules(&k, depth + 1, out);
    }
}

/// `classes` 目录下的 class 文件 → 类全名（`com/demo/Money$1.class` → `com.demo.Money$1`）。`module-info` / `package-info` 不是能换的类
pub fn class_name(classes: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(classes).ok()?.to_str()?.strip_suffix(".class")?;
    let name = rel.replace('/', ".");
    (!name.ends_with("module-info") && !name.ends_with("package-info")).then_some(name)
}

fn class_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => class_files(&p, out),
            Ok(t) if t.is_file() && p.extension().is_some_and(|x| x == "class") => out.push(p),
            _ => {}
        }
    }
}

/// 盘上每个 class 的指纹，按文件路径记
#[derive(Default, Debug)]
pub struct Baseline {
    prints: HashMap<PathBuf, Print>,
    /// JVM 起来那会儿每个 class 的行号表摘要（[`line_digest`]）。**换成功也不更新**：调试器缓存的可能是任何一版，
    /// 只有每一次换都和最初那版一样，才能说缓存的那张是对的
    lines: HashMap<PathBuf, Option<u64>>,
    /// 换进去过一版行号表不一样的：调试器可能已经缓存了那一版（显示调用栈也要用行号表，不只是下断点），这次调试里再也不能信它
    tainted: std::collections::HashSet<PathBuf>,
}

impl Baseline {
    /// 现在盘上的样子。几千个 class 也就几十毫秒（读文件 + 哈希 + 扫一遍行号表）
    pub fn scan(root: &Path) -> Baseline {
        let mut b = Baseline::default();
        for (c, lines) in current(root) {
            b.lines.insert(c.path.clone(), lines);
            b.prints.insert(c.path, c.print);
        }
        b
    }

    /// 和记着的比，内容变了的、新出来的（新加的内部类、匿名类）。删掉的不管：JVM 里那个类留着，没人再用它就没事
    pub fn changes(&self, root: &Path) -> Vec<Change> {
        current(root)
            .into_iter()
            .filter(|(c, _)| self.prints.get(&c.path) != Some(&c.print))
            .map(|(mut c, lines)| {
                c.same_lines = !self.tainted.contains(&c.path)
                    && match self.lines.get(&c.path) {
                        None => true,
                        Some(old) => old.is_some() && *old == lines,
                    };
                c
            })
            .collect()
    }

    /// 这几个换成功了：之后拿它们的新样子当基准。行号表变了的记下来，这次调试一直算变了
    pub fn accept(&mut self, done: &[Change]) {
        for c in done {
            self.prints.insert(c.path.clone(), c.print);
            if !c.same_lines {
                self.tainted.insert(c.path.clone());
            }
        }
    }

    pub fn len(&self) -> usize {
        self.prints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prints.is_empty()
    }
}

fn current(root: &Path) -> Vec<(Change, Option<u64>)> {
    let mut out = Vec::new();
    for dir in class_dirs(root) {
        let mut files = Vec::new();
        class_files(&dir, &mut files);
        for f in files {
            let (Some(class), Ok(bytes)) = (class_name(&dir, &f), fs::read(&f)) else { continue };
            out.push((Change { class, print: print_of(&bytes), path: f, same_lines: true }, line_digest(&bytes)));
        }
    }
    out
}

/// 一个 class 里每个方法（名字 + 描述符）的行号表（`LineNumberTable`：字节码位置 → 行）合起来的摘要。读不懂的 class 回 `None`（当成「变了」）。
///
/// 只读 class 文件格式里够用的那几块（JVMS 第 4 章）：常量池跳过去（记下 UTF8 的位置拿名字）、字段跳过去、方法里找 Code 属性、Code 里找 LineNumberTable。
/// 不引一个 class 文件的库：要的就这一张表，几十行
pub fn line_digest(b: &[u8]) -> Option<u64> {
    let mut r = Reader { b, at: 0 };
    if r.u4()? != 0xCAFE_BABE {
        return None;
    }
    r.skip(4)?; // 版本号
    let n = r.u2()? as usize;
    let mut utf8: Vec<Option<(usize, usize)>> = vec![None; n];
    let mut i = 1;
    while i < n {
        let tag = r.u1()?;
        match tag {
            1 => {
                let len = r.u2()? as usize;
                utf8[i] = Some((r.at, len));
                r.skip(len)?;
            }
            3 | 4 => r.skip(4)?,
            // long / double 占两格（JVMS 4.4.5）
            5 | 6 => {
                r.skip(8)?;
                i += 1;
            }
            7 | 8 | 16 | 19 | 20 => r.skip(2)?,
            9 | 10 | 11 | 12 | 17 | 18 => r.skip(4)?,
            15 => r.skip(3)?,
            _ => return None,
        }
        i += 1;
    }
    let name = |idx: u16| utf8.get(idx as usize).copied().flatten().map(|(at, len)| &b[at..at + len]);
    r.skip(6)?; // 访问标志、本类、父类
    let ifaces = r.u2()? as usize;
    r.skip(ifaces * 2)?;
    for _ in 0..r.u2()? {
        r.skip(6)?;
        r.skip_attrs()?;
    }
    let mut h = DefaultHasher::new();
    for _ in 0..r.u2()? {
        r.skip(2)?;
        let (mname, desc) = (name(r.u2()?)?, name(r.u2()?)?);
        h.write(mname);
        h.write(b"(");
        h.write(desc);
        for _ in 0..r.u2()? {
            let attr = name(r.u2()?)?;
            let len = r.u4()? as usize;
            let end = r.at.checked_add(len)?;
            if attr == b"Code" {
                r.skip(4)?; // max_stack, max_locals
                let code = r.u4()? as usize;
                r.skip(code)?;
                let ex = r.u2()? as usize;
                r.skip(ex * 8)?;
                for _ in 0..r.u2()? {
                    let a = name(r.u2()?)?;
                    let alen = r.u4()? as usize;
                    if a == b"LineNumberTable" {
                        h.write(r.take(alen)?);
                    } else {
                        r.skip(alen)?;
                    }
                }
            }
            r.at = end;
        }
    }
    Some(h.finish())
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }
    fn u1(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u2(&mut self) -> Option<u16> {
        let s = self.take(2)?;
        Some(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u4(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn skip_attrs(&mut self) -> Option<()> {
        for _ in 0..self.u2()? {
            self.skip(2)?;
            let len = self.u4()? as usize;
            self.skip(len)?;
        }
        Some(())
    }
}

/// 给 JVM 的那个参数。`jar` / `dir` 由调用方负责引号（shell 里、surefire 的参数里各有各的规矩）
pub fn agent_arg(jar: &Path, dir: &Path) -> String {
    format!("-javaagent:{}={}", jar.display(), dir.display())
}

/// 新建一个控制目录：用户自己的临时目录下、只有自己能进（0700）。放临时目录不放应用数据目录：后者的路径里有空格
/// （`Application Support`），单测调试时这个参数要塞进 surefire 的 `maven.surefire.debug`，surefire 按空格拆
pub fn control_dir(tag: &str) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let d = std::env::temp_dir().join(format!("lite-hotswap-{tag}-{}-{nanos}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&d)?;
    Ok(d)
}

/// 调试结束：控制目录整个删掉。只删自己建的那种名字（`lite-hotswap-` 开头、在临时目录里），别的路径一概不碰
pub fn remove_control_dir(d: &Path) {
    let ours = d.parent() == Some(std::env::temp_dir().as_path()) && d.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("lite-hotswap-"));
    if ours {
        #[allow(clippy::disallowed_methods)] // 应用自己生成的临时目录（上面卡死了名字和位置），不是用户的文件，不进废纸篓
        let _ = fs::remove_dir_all(d);
    }
}

/// 一个 JVM 的回话
#[derive(Debug, PartialEq)]
pub enum Reply {
    /// 换成了。`redefined`：真换了的（已经加载的）；`unloaded`：还没加载、不用换的（第一次用到时从盘上读的就是新的）
    Done { redefined: Vec<String>, unloaded: Vec<String> },
    /// 一个都没换（JVM 保证一批要么全换要么全不换）。JVM 的原话
    Failed(String),
}

/// 控制目录下还活着的 JVM（子目录名是 pid）
pub fn live_jvms(dir: &Path) -> Vec<u32> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<u32> = rd
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| pid > 0 && unsafe { libc::kill(pid as i32, 0) } == 0)
        .collect();
    v.sort();
    v
}

/// 请求的正文：第一行 id，之后每行「类全名 \t 文件」（协议写在 Agent.java 的 `handle` 上）
pub fn request_text(id: u64, changes: &[Change]) -> String {
    let mut s = format!("id\t{id}\n");
    for c in changes {
        s.push_str(&format!("{}\t{}\n", c.class, c.path.display()));
    }
    s
}

/// 解析回话。id 对不上（上一次超时没取走的那份）当没收到
pub fn parse_reply(id: u64, text: &str) -> Option<Reply> {
    let mut lines = text.lines();
    if lines.next()? != format!("id\t{id}") {
        return None;
    }
    let head = lines.next()?;
    if let Some(msg) = head.strip_prefix("fail\t") {
        return Some(Reply::Failed(msg.to_string()));
    }
    if head != "ok" {
        return None;
    }
    let (mut redefined, mut unloaded) = (Vec::new(), Vec::new());
    for l in lines {
        match l.split_once('\t') {
            Some(("redefined", n)) => redefined.push(n.to_string()),
            Some(("unloaded", n)) => unloaded.push(n.to_string()),
            _ => {}
        }
    }
    Some(Reply::Done { redefined, unloaded })
}

/// 发给控制目录下每个活着的 JVM、等回话。先全写出去再一起等：几个 JVM 各等一遍就是几倍的时间。
/// 等不到的回 `None`（agent 那条线程停着：调试器「暂停全部」时它也停）—— 没取走的请求删掉，免得它继续之后悄悄换了、而界面说的是没换
pub fn send(dir: &Path, id: u64, changes: &[Change], wait: Duration) -> Vec<(u32, Option<Reply>)> {
    let body = request_text(id, changes);
    let pids = live_jvms(dir);
    let mut pending = Vec::new();
    for pid in pids {
        let d = dir.join(pid.to_string());
        if write_atomic(&d.join("req"), &body).is_ok() {
            pending.push(pid);
        }
    }
    let deadline = Instant::now() + wait;
    let mut out = Vec::new();
    while !pending.is_empty() {
        pending.retain(|&pid| {
            let res = dir.join(pid.to_string()).join("res");
            let Ok(text) = fs::read_to_string(&res) else { return true };
            #[allow(clippy::disallowed_methods)] // agent 写给我们的回话文件，读完就删（应用自己的临时目录）
            let _ = fs::remove_file(&res);
            match parse_reply(id, &text) {
                Some(r) => {
                    out.push((pid, Some(r)));
                    false
                }
                None => true,
            }
        });
        if pending.is_empty() || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in pending {
        #[allow(clippy::disallowed_methods)] // 没被取走的请求（应用自己的临时目录里）
        let _ = fs::remove_file(dir.join(pid.to_string()).join("req"));
        out.push((pid, None));
    }
    out
}

/// 先写 `.tmp` 再改名：agent 每 200ms 看一眼，不能让它读到写了一半的请求
fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&tmp)?;
        f.write_all(body.as_bytes())?;
    }
    #[allow(clippy::disallowed_methods)] // 改名的两头都是应用自己在自己的临时目录里写的请求文件，覆盖上一份没取走的正是想要的
    fs::rename(&tmp, path)
}

/// JVM 的原话翻成人话。HotSpot 换不了时的原话是 `class redefinition failed: attempted to …`（JVMTI 的几种错误各一句），
/// 认得出的换成「改了什么、为什么不行」，认不出的原样给（翻错了比不翻糟）
pub fn humanize(msg: &str) -> String {
    let m = msg.to_ascii_lowercase();
    let what = if m.contains("add a method") {
        "加了方法"
    } else if m.contains("delete a method") {
        "删了方法"
    } else if m.contains("schema") {
        "加减了字段"
    } else if m.contains("superclass or interfaces") || m.contains("hierarchy") {
        "改了父类或接口"
    } else if m.contains("method modifiers") {
        "改了方法的修饰符"
    } else if m.contains("class modifiers") {
        "改了类的修饰符"
    } else if m.contains("rename") {
        "改了类名"
    } else {
        return msg.to_string();
    };
    format!("{what} —— 运行中的 JVM 只能换方法体")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hotswap-test-{tag}-{}", std::process::id()));
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(p: &Path, body: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        #[allow(clippy::disallowed_methods)] // 测试造的样例
        fs::write(p, body).unwrap();
    }

    #[test]
    fn 模块的输出目录_嵌套的模块也找到_src和node_modules不进() {
        let d = tmp("dirs");
        put(&d.join("pom.xml"), b"");
        put(&d.join("shop-common/pom.xml"), b"");
        put(&d.join("shop-common/target/classes/com/demo/Money.class"), b"a");
        put(&d.join("order/pom.xml"), b"");
        put(&d.join("order/target/test-classes/com/demo/OrderTest.class"), b"a");
        // src 底下、node_modules 底下就算有 pom 也不是模块的输出
        put(&d.join("order/src/it/pom.xml"), b"");
        put(&d.join("order/src/it/target/classes/X.class"), b"a");
        put(&d.join("web/node_modules/x/pom.xml"), b"");
        put(&d.join("web/node_modules/x/target/classes/Y.class"), b"a");
        let got: Vec<String> = class_dirs(&d).iter().map(|p| p.strip_prefix(&d).unwrap().display().to_string()).collect();
        assert_eq!(got, vec!["order/target/test-classes", "shop-common/target/classes"]);
    }

    #[test]
    fn 类名_内部类带美元符_module_info不算() {
        let c = Path::new("/p/target/classes");
        assert_eq!(class_name(c, Path::new("/p/target/classes/com/demo/Money$1.class")).as_deref(), Some("com.demo.Money$1"));
        assert_eq!(class_name(c, Path::new("/p/target/classes/module-info.class")), None);
        assert_eq!(class_name(c, Path::new("/p/target/classes/com/demo/package-info.class")), None);
        assert_eq!(class_name(c, Path::new("/p/target/classes/a.txt")), None);
    }

    #[test]
    fn 只报内容变了的_重编出来一模一样的不算_换成功的记进基准() {
        let d = tmp("diff");
        put(&d.join("pom.xml"), b"");
        let money = d.join("target/classes/com/demo/Money.class");
        let order = d.join("target/classes/com/demo/Order.class");
        put(&money, b"old money");
        put(&order, b"order");
        let mut base = Baseline::scan(&d);
        assert_eq!(base.len(), 2);
        // Maven 整个模块重编：两个文件都重写了，只有 Money 的内容变了
        put(&money, b"new money");
        put(&order, b"order");
        put(&d.join("target/classes/com/demo/Money$1.class"), b"anon");
        let ch = base.changes(&d);
        let mut names: Vec<&str> = ch.iter().map(|c| c.class.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["com.demo.Money", "com.demo.Money$1"]);
        base.accept(&ch);
        assert!(base.changes(&d).is_empty(), "换成功之后再比：没有变化");
    }

    /// 三份真 class（javac --release 8 编的，tests/fixtures/）：a 原版；b 只改了字符串（字节码长度不变）；c 前面多一行语句（后面的行挪了位置）
    #[test]
    fn 行号表_只改字符串的一样_多一行的不一样_读不懂的是none() {
        let a = include_bytes!("../tests/fixtures/Money-a.class");
        let b = include_bytes!("../tests/fixtures/Money-b.class");
        let c = include_bytes!("../tests/fixtures/Money-c.class");
        assert_ne!(a.as_slice(), b.as_slice(), "b 确实和 a 不一样（否则这条什么也没测）");
        assert!(line_digest(a).is_some());
        assert_eq!(line_digest(a), line_digest(b), "只改字符串：断点还能照旧表下");
        assert_ne!(line_digest(a), line_digest(c), "多一行：旧表的位置在新方法里对不上");
        assert_eq!(line_digest(b"not a class"), None);
        assert_eq!(line_digest(&a[..a.len() - 20]), None, "截断的不能当成读懂了");
    }

    #[test]
    fn 换过一次行号表变了的类_之后都不算same_lines() {
        let d = tmp("lines");
        put(&d.join("pom.xml"), b"");
        let p = d.join("target/classes/Money.class");
        put(&p, include_bytes!("../tests/fixtures/Money-a.class"));
        let mut base = Baseline::scan(&d);
        put(&p, include_bytes!("../tests/fixtures/Money-b.class"));
        let ch = base.changes(&d);
        assert!(ch[0].same_lines, "只改了字符串");
        base.accept(&ch);
        put(&p, include_bytes!("../tests/fixtures/Money-c.class"));
        let ch = base.changes(&d);
        assert!(!ch[0].same_lines, "多了一行");
        base.accept(&ch);
        // 改回和最初一样的行号表：可 c 那一版换进去过，调试器缓存的可能是 c 的表 —— 照样不算
        put(&p, include_bytes!("../tests/fixtures/Money-b.class"));
        let ch = base.changes(&d);
        assert!(!ch[0].same_lines, "换进去过行号表不一样的一版，这次调试里一直不算");
    }

    #[test]
    fn 回话_id对不上的当没收到_失败带原话() {
        assert_eq!(parse_reply(3, "id\t2\nok\n"), None, "上一次超时没取走的那份");
        assert_eq!(
            parse_reply(3, "id\t3\nok\nredefined\tcom.demo.Money\nunloaded\tcom.demo.Money$1\n"),
            Some(Reply::Done { redefined: vec!["com.demo.Money".into()], unloaded: vec!["com.demo.Money$1".into()] })
        );
        assert_eq!(
            parse_reply(3, "id\t3\nfail\tclass redefinition failed: attempted to add a method\n"),
            Some(Reply::Failed("class redefinition failed: attempted to add a method".into()))
        );
        assert_eq!(parse_reply(3, "id\t3\n"), None, "写了一半（不该发生：agent 先写 .tmp 再改名）");
    }

    #[test]
    fn 原话翻成人话_认不出的原样给() {
        assert_eq!(humanize("class redefinition failed: attempted to add a method"), "加了方法 —— 运行中的 JVM 只能换方法体");
        assert_eq!(humanize("class redefinition failed: attempted to change the schema (add/remove fields)"), "加减了字段 —— 运行中的 JVM 只能换方法体");
        assert_eq!(humanize("Incompatible magic value"), "Incompatible magic value");
    }

    #[test]
    fn 发请求_死了的pid不等_活着的等到回话() {
        let d = tmp("send");
        // 自己这个进程当「活着的 JVM」，回话由测试线程扮演 agent 写
        let me = std::process::id();
        fs::create_dir_all(d.join(me.to_string())).unwrap();
        // 一个肯定不在的 pid：不能把它算成活的、白等它
        fs::create_dir_all(d.join("999999")).unwrap();
        assert_eq!(live_jvms(&d), vec![me]);
        let ch = vec![Change { class: "com.demo.Money".into(), path: "/x/Money.class".into(), print: print_of(b"x"), same_lines: true }];
        let dir = d.join(me.to_string());
        let agent = std::thread::spawn(move || {
            let req = dir.join("req");
            let t = Instant::now();
            while !req.exists() && t.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let body = fs::read_to_string(&req).unwrap();
            assert_eq!(body, "id\t7\ncom.demo.Money\t/x/Money.class\n");
            write_atomic(&dir.join("res"), "id\t7\nok\nredefined\tcom.demo.Money\n").unwrap();
        });
        let t = Instant::now();
        let got = send(&d, 7, &ch, Duration::from_secs(5));
        agent.join().unwrap();
        assert_eq!(got, vec![(me, Some(Reply::Done { redefined: vec!["com.demo.Money".into()], unloaded: vec![] }))]);
        assert!(t.elapsed() < Duration::from_secs(2), "不该等满：{:?}", t.elapsed());
    }

    #[test]
    fn 等不到回话_说没回_请求删掉不留着以后悄悄换() {
        let d = tmp("timeout");
        let me = std::process::id();
        fs::create_dir_all(d.join(me.to_string())).unwrap();
        let got = send(&d, 1, &[], Duration::from_millis(300));
        assert_eq!(got, vec![(me, None)]);
        assert!(!d.join(me.to_string()).join("req").exists(), "没取走的请求要删掉");
    }
}
