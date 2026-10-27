//! fsservice 的测试（2026-10-12 从 lib.rs 拆出来，内容没动）

use super::*;

// ── 剪贴板：上限那几条用普通命令测（CI 上剪贴板不一定有东西） ──

fn sh(script: &str) -> Command {
    let mut c = Command::new("/bin/sh");
    c.args(["-c", script]);
    c
}

/// 编码真正的风险在 pbpaste 的 locale，不在这里：printf 原样吐字节，不看 locale。
/// 这条只管「UTF-8 字节进来，原样解成字符串」，locale 由下一条管
#[test]
fn 剪贴板_utf8字节原样解码() {
    let t = read_text_capped(sh("printf '订单 8842011 超时'"), 1024).unwrap();
    assert_eq!(t, "订单 8842011 超时");
}

#[test]
fn 剪贴板_pbpaste必须带utf8_locale() {
    // 少了这两个，从 Finder 启动时中文剪贴板读出来是 GB18030，粘贴直接报错
    let cmd = pbpaste_cmd();
    let envs: Vec<_> = cmd.get_envs().collect();
    for key in ["LANG", "LC_ALL"] {
        let v = envs.iter().find(|(k, _)| *k == key).and_then(|(_, v)| *v);
        assert_eq!(v.and_then(|v| v.to_str()), Some("en_US.UTF-8"), "{key}");
    }
}

#[test]
fn 剪贴板_超过上限报错不截断() {
    let e = read_text_capped(sh("head -c 5000 /dev/zero | tr '\\0' a"), 1000).unwrap_err();
    assert!(e.to_string().contains("没有粘贴"), "{e}");
}

#[test]
fn 剪贴板_正好等于上限不算超() {
    // 多读一个字节就是为了这条：读满 cap 和「后面还有」是两回事
    let t = read_text_capped(sh("head -c 1000 /dev/zero | tr '\\0' a"), 1000).unwrap();
    assert_eq!(t.len(), 1000);
}

#[test]
fn 剪贴板_命令失败报错() {
    assert!(read_text_capped(sh("exit 3"), 1000).is_err());
}

/// 挪进目录、同名拒绝、目录不能挪进自己、挪回原处是空操作
#[test]
fn move_entry_四条判据() {
    let d = sandbox("move");
    fs::create_dir_all(d.join("a/deep")).unwrap();
    fs::create_dir_all(d.join("b")).unwrap();
    fs::write(d.join("a/f.txt"), "x").unwrap();
    fs::write(d.join("b/f.txt"), "y").unwrap();

    let to = move_entry(d.join("a/f.txt"), d.join("a/deep")).unwrap();
    assert_eq!(to, d.join("a/deep/f.txt"));
    assert!(d.join("a/deep/f.txt").exists() && !d.join("a/f.txt").exists());

    let err = move_entry(d.join("a/deep/f.txt"), d.join("b")).expect_err("同名要拒绝");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(d.join("b/f.txt")).unwrap(), "y", "被拒绝就不能动人家的文件");

    let err = move_entry(d.join("a"), d.join("a/deep")).expect_err("目录不能挪进自己");
    assert!(err.to_string().contains("自己"), "{err}");

    assert_eq!(move_entry(d.join("b/f.txt"), d.join("b")).unwrap(), d.join("b/f.txt"), "挪回原处是空操作");
    let err = move_entry(d.join("b/f.txt"), d.join("b/f.txt")).expect_err("目标不是目录");
    assert!(err.to_string().contains("不是目录"), "{err}");
}

/// 目录整条解析、文件只解析父目录、文件本身是软链时最后一段留着
#[test]
fn canonical_解析目录但留着文件名() {
    let real = sandbox("canon-real");
    fs::write(real.join("a.txt"), "x").unwrap();
    fs::write(real.join("target.txt"), "y").unwrap();
    std::os::unix::fs::symlink(real.join("target.txt"), real.join("link.txt")).unwrap();
    let alias = std::env::temp_dir().join("fsservice-test-canon-alias");
    let _ = fs::remove_file(&alias);
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let real_c = fs::canonicalize(&real).unwrap();

    assert_eq!(canonical(&alias), real_c, "目录要整条解析到真身");
    assert_eq!(canonical(alias.join("a.txt")), real_c.join("a.txt"), "文件的父目录要解析");
    assert_eq!(
        canonical(alias.join("link.txt")),
        real_c.join("link.txt"),
        "文件本身是软链时最后一段要留着 —— 标签得叫 link.txt"
    );
    let gone = alias.join("没有的.txt");
    assert_eq!(canonical(&gone), real_c.join("没有的.txt"), "不存在的文件也按父目录整理（新建那条路）");
    let _ = fs::remove_file(&alias);
}

/// CRLF 文件：读进来是 \n，写回去还是 CRLF；这是 issue #33 ③ 修的那个静默转换
#[test]
fn crlf_文件读写不换行符() {
    let d = sandbox("crlf");
    let f = d.join("w.txt");
    fs::write(&f, b"a\r\nb\r\n").unwrap();
    let got = read_text_detect(&f, "").unwrap();
    assert_eq!(got.eol, eol::Eol::CrLf);
    assert_eq!(got.content, "a\nb\n", "前端只该见到 \\n");
    write_text_as(&f, "a\nb\nc\n", got.encoding, got.bom, got.eol).unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"a\r\nb\r\nc\r\n", "写回去要保持 CRLF，新加的行也是");
}

fn sandbox(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fsservice-test-{name}"));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/*
 * 读文件的硬顶。上限可注入是有意的 —— 真造一个 64MB 的临时文件来测
 * 太贵（跑测试的人的盘和时间都要付账），而这条闸必须被测到：
 * 它挡的是「打开时 1KB、后来涨到 2GB」那类文件从重读路径进来。
 */
#[test]
fn 超过上限的文件不读进内存() {
    let d = std::env::temp_dir().join("fsservice-cap");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let f = d.join("big.txt");
    fs::write(&f, "0123456789ABCDEF").unwrap(); // 16 字节

    // 正好等于上限：要能读，不能误报
    let ok = read_capped(&f, "", 16).expect("正好等于上限应当读得出来");
    assert_eq!(ok.content, "0123456789ABCDEF");

    // 超一个字节：必须报错，而且话要说得让人知道下一步干什么
    let err = read_capped(&f, "", 15).expect_err("超限必须报错");
    let msg = err.to_string();
    assert!(msg.contains("读不下"), "错误里要说明读不下：{msg}");
    assert!(msg.contains("日志模式"), "要指出下一步该怎么办：{msg}");

    fs::remove_dir_all(d).ok();
}

/// 取 `list_dir(d)` 里某一项的链
fn chain_of(d: &Path, name: &str) -> Vec<String> {
    list_dir(d).unwrap().into_iter().find(|e| e.name == name).unwrap().chain
}

#[test]
fn 单子目录链_走到分叉为止_杂物不算数() {
    let d = sandbox("chain");
    let pkg = d.join("src/main/java/com/demo");
    fs::create_dir_all(pkg.join("order")).unwrap();
    fs::create_dir_all(pkg.join("user")).unwrap();
    // Finder 打开过 com/ 就会有这个 —— 它不许把链从中间截断
    fs::write(d.join("src/main/java/com/.DS_Store"), "x").unwrap();
    // 唯一那一项是文件：链到这里停，文件不进链
    fs::create_dir_all(d.join("docs/guide")).unwrap();
    fs::write(d.join("docs/guide/a.md"), "x").unwrap();
    fs::create_dir(d.join("empty")).unwrap();
    fs::write(d.join("README.md"), "x").unwrap();

    assert_eq!(chain_of(&d, "src"), ["main", "java", "com", "demo"], "demo 底下分叉了，停在 demo");
    assert_eq!(chain_of(&d, "docs"), ["guide"]);
    assert!(chain_of(&d, "empty").is_empty());
    assert!(chain_of(&d, "README.md").is_empty(), "文件没有链");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 单子目录链_生成物和有争议的目录要自己占一行() {
    let d = sandbox("chain-gen");
    fs::create_dir_all(d.join("web/node_modules/vue")).unwrap();
    fs::create_dir_all(d.join("native/build/out")).unwrap();
    fs::create_dir_all(d.join("node_modules/a/b")).unwrap();
    assert!(chain_of(&d, "web").is_empty(), "合并成 web/node_modules 的话它就没法单独压暗了");
    assert!(chain_of(&d, "native").is_empty(), "build 要等 git 说了算，不能先吞进链里");
    assert!(chain_of(&d, "node_modules").is_empty(), "生成物目录自己不探：不预取是它的规矩");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 单子目录链_不跟软链() {
    let d = sandbox("chain-link");
    fs::create_dir_all(d.join("real/inner")).unwrap();
    fs::create_dir(d.join("l")).unwrap();
    std::os::unix::fs::symlink(d.join("real"), d.join("l/to-real")).unwrap();
    assert!(chain_of(&d, "l").is_empty(), "软链可能绕回自己，也不是盘上真的一串目录");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 单子目录链_有深度上限和预算() {
    let d = sandbox("chain-cap");
    let mut deep = d.join("deep");
    for i in 0..40 {
        deep.push(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(chain_of(&d, "deep").len(), CHAIN_MAX_DEPTH);
    fs::remove_dir_all(&d).ok();

    // 600 个子目录、每个底下一层：每个要读两次（有一个孩子 → 再读那个孩子发现空了），
    // 预算 512 只够前 256 个。超了的**不探**，而不是把整次列目录拖慢
    let d = sandbox("chain-budget");
    for i in 0..600 {
        fs::create_dir_all(d.join(format!("m{i:03}/x"))).unwrap();
    }
    let got = list_dir(&d).unwrap();
    let probed = got.iter().filter(|e| !e.chain.is_empty()).count();
    assert_eq!(probed, CHAIN_BUDGET / 2);
    assert!(!got[0].chain.is_empty() && got[599].chain.is_empty(), "按显示顺序先到先得");
    fs::remove_dir_all(d).ok();
}

/// 按名单循环造，不照抄名字：往 `excludes` 里加一个名字，这条自动就验到了
#[test]
fn 版本库目录和系统杂物不列出来() {
    let d = sandbox("hidden-names");
    for n in excludes::VCS_DIRS {
        fs::create_dir(d.join(n)).unwrap();
    }
    for n in excludes::JUNK_FILES {
        fs::write(d.join(n), "x").unwrap();
    }
    fs::create_dir(d.join(".github")).unwrap();
    fs::write(d.join(".env"), "x").unwrap();
    let names: Vec<String> = list_dir(&d).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, [".github", ".env"], "别的点文件、点目录照列");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 目录在前_同类按名称排序() {
    let d = sandbox("sort");
    fs::write(d.join("beta.txt"), "x").unwrap();
    fs::write(d.join("Alpha.txt"), "x").unwrap();
    fs::create_dir(d.join("zeta")).unwrap();
    fs::create_dir(d.join("Mid")).unwrap();

    let got: Vec<String> = list_dir(&d)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(got, vec!["Mid", "zeta", "Alpha.txt", "beta.txt"]);
    fs::remove_dir_all(d).ok();
}

/// 点文件要列出来，生成物目录不列。
///
/// 这两件事以前共用一个 `show_hidden` 开关，于是 `.gitignore` 这类
/// 天天要改的文件跟着 `node_modules` 一起消失了。
///
/// 造目录是**按 [`excludes::GENERATED_DIRS`] 循环**，不是照抄四个名字：
/// 往那份名单里加一个名字，这条测试自动就覆盖到了 —— 而写死名字的话，
/// 名单长了测试却还只验老那几个，正是当初两份名单分岔的形状。
#[test]
fn 点文件和生成物目录都要列出来_生成物带标记() {
    let d = sandbox("hidden");
    fs::write(d.join("visible.rs"), "x").unwrap();
    fs::write(d.join(".env"), "x").unwrap();
    fs::create_dir(d.join(".github")).unwrap();
    fs::create_dir(d.join("build")).unwrap();
    fs::create_dir(d.join("node_modules")).unwrap();
    // **一个叫 build 的文件不是生成物目录。** shell 脚本里这个名字很常见，
    // 而 `generated` 只该跟着「里面全是工具生成的东西」这件事走
    fs::write(d.join("target"), "#!/bin/sh\n").unwrap();

    let got: Vec<(String, bool, bool)> = list_dir(&d)
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.generated, e.contested))
        .collect();
    /*
     * 目录在前、同类不区分大小写排序 —— 点目录也照这条规矩排。
     *
     * **生成物不单独排到末尾**（issue #13）：挪位置比压暗更让人意外，
     * 而且 `target/` 一旦换了位置，「它刚才还在这儿」这种困惑比看见它更贵。
     */
    assert_eq!(
        got,
        vec![
            (".github".into(), false, false),
            // build 是有争议的：名字像，但要问过 git 才算数
            ("build".into(), true, true),
            // node_modules 没有第二种可能，不用问
            ("node_modules".into(), true, false),
            (".env".into(), false, false),
            ("target".into(), false, false), // 它是文件
            ("visible.rs".into(), false, false),
        ]
    );
    fs::remove_dir_all(d).ok();
}

/// **保存不许改掉文件的权限位。**
///
/// 「临时文件 + rename」这条路上，临时文件是新建的，带的是 umask 的默认
/// 权限；不把原文件的权限抄过去，一个 `0755` 的脚本保存完就变成 `0644`，
/// **当场不能执行了**，而界面还报「已保存」。
#[test]
fn 保存不改变文件权限() {
    use std::os::unix::fs::PermissionsExt;
    let d = sandbox("perm");
    let f = d.join("run.sh");
    write_text(&f, "#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();

    write_text(&f, "#!/bin/sh\necho hi2\n").unwrap();

    let mode = fs::metadata(&f).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "保存之后权限变成了 {mode:o}，这个脚本已经不能执行了");
    fs::remove_dir_all(d).ok();
}

/// **保存一条软链，要写进它指向的那个文件。**
///
/// 不解引用的话，rename 会把软链本身换成一个普通文件：改动写在了
/// 一个新文件里，真身一字未动，而链接没了。`~/.zshrc` 指向 dotfiles
/// 仓库这种最常见的用法正好踩中。
#[test]
fn 保存写进软链指向的文件_而不是把软链换掉() {
    let d = sandbox("symlink");
    let real = d.join("real.txt");
    let link = d.join("link.txt");
    write_text(&real, "原文\n").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    write_text(&link, "改过了\n").unwrap();

    assert!(
        fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
        "软链被换成普通文件了"
    );
    assert_eq!(read_text(&real).unwrap(), "改过了\n", "改动没写进真身");
    fs::remove_dir_all(d).ok();
}

/// **保存不许把文件从硬链接组里摘出去。**
///
/// rename 换的是目录项，原来那个 inode 还被另一个名字拿着 ——
/// 于是「同一个文件」悄悄变成了两个，另一头再也收不到改动。
/// 这条走的是原地覆写那条分支（判据同 vim 的 `backupcopy=auto`）。
/// **准备不碰原文件，提交才换上**（#42 两段提交的前提）：准备完原文件一个字节没变、
/// 临时文件在；提交后内容换了、临时文件没了、权限照旧
#[test]
fn 准备和提交分两步() {
    use std::os::unix::fs::PermissionsExt;
    let d = sandbox("prepare-commit");
    let f = d.join("run.sh");
    fs::write(&f, "old\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    let p = prepare_bytes(&f, b"new\n").unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "old\n", "准备阶段不能动原文件");
    assert!(tmp_path_for(&f).exists(), "新内容先落在临时文件里");
    assert!(!p.in_place());
    p.commit().unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "new\n");
    assert!(!tmp_path_for(&f).exists(), "提交后不留临时文件");
    assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o755, "权限照旧");
}

/// **准备了不提交（出错返回、或者别的文件准备失败放弃整次），临时文件要自己收掉。**
/// 替换准备几百个文件、第 200 个失败时，前 199 个的临时文件不能留在用户的目录里
#[test]
fn 准备了不提交_临时文件自己收掉() {
    let d = sandbox("prepare-drop");
    let f = d.join("a.txt");
    fs::write(&f, "old").unwrap();
    let p = prepare_bytes(&f, b"new").unwrap();
    assert!(tmp_path_for(&f).exists());
    drop(p);
    assert!(!tmp_path_for(&f).exists(), "没提交的临时文件被收掉了");
    assert_eq!(fs::read_to_string(&f).unwrap(), "old", "原文件没动");
}

/// 硬链接只能原地写，准备阶段什么都不写，提交时才写 —— 替换要知道这一类，把它排到最后
#[test]
fn 硬链接的准备不落盘() {
    let d = sandbox("prepare-hardlink");
    let a = d.join("a.txt");
    let b = d.join("b.txt");
    fs::write(&a, "old").unwrap();
    fs::hard_link(&a, &b).unwrap();
    let p = prepare_bytes(&a, b"new").unwrap();
    assert!(p.in_place());
    assert!(!tmp_path_for(&a).exists(), "原地写不经过临时文件");
    assert_eq!(fs::read_to_string(&b).unwrap(), "old", "准备阶段两份都没变");
    p.commit().unwrap();
    assert_eq!(fs::read_to_string(&b).unwrap(), "new", "提交后另一份也变了（还在一个链接组里）");
}

#[test]
fn 保存不拆掉硬链接() {
    use std::os::unix::fs::MetadataExt;
    let d = sandbox("hardlink");
    let a = d.join("a.txt");
    let b = d.join("b.txt");
    write_text(&a, "原文\n").unwrap();
    fs::hard_link(&a, &b).unwrap();

    write_text(&a, "改过了\n").unwrap();

    assert_eq!(read_text(&b).unwrap(), "改过了\n", "另一个名字还看着旧内容，链接被拆了");
    assert_eq!(fs::metadata(&a).unwrap().nlink(), 2, "链接数掉了");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 读写往返() {
    let d = sandbox("rw");
    let f = d.join("a.txt");
    write_text(&f, "第一行\n第二行\n").unwrap();
    assert_eq!(read_text(&f).unwrap(), "第一行\n第二行\n");
    fs::remove_dir_all(d).ok();
}

/// 用什么编码读进来，就该用什么编码写回去 —— 保存不该偷偷改变文件的编码
#[test]
fn 非utf8文件的读写往返不改变编码() {
    let d = sandbox("enc-roundtrip");
    let f = d.join("gbk.txt");
    let text = "订单处理失败\n重试中\n";

    // 造一个 GBK 文件
    fs::write(&f, encoding::encode(text, "GBK", false)).unwrap();

    let got = read_text_detect(&f, "").unwrap();
    assert_eq!(got.content, text);
    assert!(got.encoding == "GBK" || got.encoding == "gb18030", "探测成了 {}", got.encoding);
    assert!(!got.lossy);

    // 原样写回去，磁盘字节应当和原来一致
    let before = fs::read(&f).unwrap();
    write_text_as(&f, &got.content, got.encoding, got.bom, got.eol).unwrap();
    assert_eq!(fs::read(&f).unwrap(), before, "保存改变了文件编码");

    fs::remove_dir_all(d).ok();
}

#[test]
fn 保存不留临时文件() {
    let d = sandbox("atomic");
    let f = d.join("a.txt");
    write_text(&f, "content").unwrap();
    let leftovers: Vec<String> = list_dir(&d)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .filter(|n| n.contains("lite-ide-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "残留临时文件: {leftovers:?}");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 指纹能认出内容变化() {
    let d = sandbox("stamp");
    let f = d.join("a.txt");
    write_text(&f, "one").unwrap();
    let s1 = stamp(&f).unwrap();

    // 只改内容不改长度，靠 mtime 认出来；睡一下确保时间戳有差异
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_text(&f, "two").unwrap();
    let s2 = stamp(&f).unwrap();
    assert_ne!(s1, s2, "内容变了指纹却没变");

    // 长度变化也要认出来
    write_text(&f, "three-longer").unwrap();
    let s3 = stamp(&f).unwrap();
    assert_ne!(s2.size, s3.size);
    fs::remove_dir_all(d).ok();
}

#[test]
fn 文件不存在时取指纹报错() {
    let d = sandbox("stamp-missing");
    assert!(stamp(d.join("nope.txt")).is_err());
    fs::remove_dir_all(d).ok();
}

#[test]
/// M14 之前这里断言的是「非 UTF-8 一律报 InvalidData」。
/// 那条策略被换掉了：现在探测编码并如实解码，`lossy` 才是「有损坏」的信号。
/// 保留这条测试的位置，是为了守住换掉它之后的新契约。
fn 非utf8不再被拒绝而是探测出编码() {
    let d = sandbox("gbk");
    let f = d.join("gbk.txt");
    // GBK 编码的「中文」
    fs::write(&f, [0xd6, 0xd0, 0xce, 0xc4]).unwrap();

    let got = read_text_detect(&f, "").unwrap();
    assert_ne!(got.encoding, "UTF-8", "不该判成 UTF-8");
    assert!(!got.lossy, "这四个字节是合法 GBK，不该报有损");
    // 短样本上 chardetng 未必能分辨 GBK / Big5 / EUC-KR，
    // 所以只断言「按 GBK 明确读能读对」，不去要求自动探测在 4 字节上也猜准
    assert_eq!(read_text_detect(&f, "GBK").unwrap().content, "中文");

    fs::remove_dir_all(d).ok();
}

// ── reveal_in_finder ──
//
// 这里**不测真的弹 Finder**：那既会打断跑测试的人，也依赖 GUI 会话
// （CI 上是没有的）。测的是两件不用起子进程就能定死的事。

#[test]
fn reveal的路径前面一定紧挨着双横线() {
    // 一个名叫 -Q 的文件。少了 `--`，open 会把它当开关：
    // 实测 `open -R "-Q"` → `open: invalid option -- Q`
    let a = reveal_args(Path::new("-Q"));
    assert_eq!(
        a,
        vec![
            OsString::from("-R"),
            OsString::from("--"),
            OsString::from("-Q")
        ]
    );
    // 真正要守的性质：路径是最后一个参数，而它前面紧挨着 `--`
    assert_eq!(a.last().unwrap(), &OsString::from("-Q"));
    assert_eq!(a[a.len() - 2], OsString::from("--"), "路径前必须有 --");
}

#[test]
fn reveal的路径整个只占一个参数() {
    // 带空格和引号的路径不能被拆开 —— Command::arg 本来就不过 shell，
    // 这条钉的是「别哪天改成拼字符串」
    let weird = "/tmp/a b/c\"d\"/e f.txt";
    let a = reveal_args(Path::new(weird));
    assert_eq!(a.len(), 3, "参数个数必须恒为 3，路径不该被拆");
    assert_eq!(a[2], OsString::from(weird));
}

#[test]
fn reveal不存在的路径不去起子进程() {
    let e = reveal_in_finder("/这个路径/根本/不存在.txt").unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
    assert!(
        e.to_string().contains("已不存在"),
        "要给一句中文，而不是把 open 的英文原样透出来，实得：{e}"
    );
}

#[test]
fn reveal坏掉的符号链接仍然算存在() {
    // 链接本身在盘上，Finder 显示得出来 —— 不该被"目标不存在"拦掉。
    // 这条是 try_exists 和 symlink_metadata 的分水岭
    let d = sandbox("reveal-symlink");
    let link = d.join("断链");
    std::os::unix::fs::symlink(d.join("目标早没了"), &link).unwrap();
    assert!(!link.try_exists().unwrap(), "前提：try_exists 判它不存在");
    assert!(
        exists_for_reveal(&link),
        "但 reveal 的判定必须放它过去 —— 链接本身在盘上，Finder 显示得出来"
    );
    fs::remove_dir_all(d).ok();
}

// ── 新建 / 重命名 / 废纸篓 ──
//
// 这一批里有三条**先证明前提**再断言：std 的默认行为（截断、覆盖、复用）
// 正是这些保护存在的理由，不把它演示一遍，读代码的人会以为
// `create_new` 和那道存在性检查是可有可无的防御性代码。

#[test]
fn 名字校验挡住五类坏名字() {
    assert!(validate_name("正常.txt").is_ok());
    assert!(validate_name(" 前导空格也放行").is_ok(), "首尾空格是合法文件名，不该越权拦");

    let bad = |n: &str| validate_name(n).unwrap_err();
    assert!(bad("").contains("不能为空"));
    assert!(bad("   ").contains("不能为空"), "全空白的名字在树里就是一行空的");
    assert!(bad("a/b").contains("不能有 /"), "放它过去就是往别的目录写东西");
    assert!(bad("a\0b").contains("空字符"));
    assert!(bad(".").contains(". 或 .."));
    assert!(bad("..").contains(". 或 .."));

    // 255 是**字节**不是字符：85 个中文正好 255 字节，86 个就超
    assert!(validate_name(&"中".repeat(85)).is_ok());
    let e = bad(&"中".repeat(86));
    assert!(e.contains("258 字节"), "错误里要说清按字节算，实得：{e}");
}

#[test]
fn 新建文件绝不截断已有文件() {
    let d = sandbox("create-file");

    // 前提：std 最顺手的那个写法会把已有文件截成 0 字节
    let victim = d.join("victim.txt");
    write_text(&victim, "本来有内容").unwrap();
    fs::File::create(&victim).unwrap();
    assert_eq!(
        fs::metadata(&victim).unwrap().len(),
        0,
        "前提不成立：File::create 不再截断了？那这条保护的理由要重写"
    );

    // 我们的：撞名报错，内容一个字节都不动
    let keep = d.join("keep.txt");
    write_text(&keep, "别动我").unwrap();
    let e = create_entry(&d, "keep.txt", false).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
    assert!(e.to_string().contains("已经有一个叫"), "实得：{e}");
    assert_eq!(read_text(&keep).unwrap(), "别动我");

    // 正常路径：新文件是空的，路径带回来
    let p = create_entry(&d, "新建.java", false).unwrap();
    assert_eq!(p, d.join("新建.java"));
    assert_eq!(read_text(&p).unwrap(), "");

    fs::remove_dir_all(d).ok();
}

#[test]
fn 同一分钟建第二份草稿要自己让开() {
    let d = sandbox("scratch").join("scratches");

    // 目录还不存在 —— create_scratch 得自己把它建出来，
    // 「第一次记东西」正是这条路唯一走过的一次
    assert!(!d.exists());
    let a = create_scratch(&d, "2026-09-09 1030").unwrap();
    assert_eq!(a.file_name().unwrap(), "2026-09-09 1030.md");
    assert_eq!(read_text(&a).unwrap(), "");

    // 同一分钟再来一份：不能失败，也不能覆盖掉上一份
    write_text(&a, "第一份的内容").unwrap();
    let b = create_scratch(&d, "2026-09-09 1030").unwrap();
    assert_eq!(b.file_name().unwrap(), "2026-09-09 1030-2.md");
    assert_eq!(read_text(&a).unwrap(), "第一份的内容", "第一份被盖了");

    let c = create_scratch(&d, "2026-09-09 1030").unwrap();
    assert_eq!(c.file_name().unwrap(), "2026-09-09 1030-3.md");

    fs::remove_dir_all(d.parent().unwrap()).ok();
}

#[test]
fn 草稿列表按名字倒序且摘要取第一行有字的() {
    let d = sandbox("scratch-list").join("scratches");
    assert!(!d.exists());
    // 目录还没有 = 一条都没记过，空列表不是错误
    assert_eq!(list_scratches(&d).unwrap(), Vec::<Scratch>::new());

    let a = create_scratch(&d, "2026-09-09 1030").unwrap();
    let b = create_scratch(&d, "2026-09-10 0900").unwrap();
    let c = create_scratch(&d, "2026-09-08 2359").unwrap();
    // 同一分钟的第二份：名字是 `… 0900-2.md`，得排在 `… 0900.md` 上面
    let b2 = create_scratch(&d, "2026-09-10 0900").unwrap();
    assert_eq!(b2.file_name().unwrap(), "2026-09-10 0900-2.md");
    write_text(&a, "\n\n# 标题在第三行\n正文").unwrap();
    write_text(&b, "").unwrap();
    write_text(&c, &"很长".repeat(200)).unwrap();
    // 目录和点文件不该出现在列表里
    fs::create_dir(d.join("子目录")).unwrap();
    fs::write(d.join(".DS_Store"), b"x").unwrap();

    let list = list_scratches(&d).unwrap();
    let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["2026-09-10 0900-2.md", "2026-09-10 0900.md", "2026-09-09 1030.md", "2026-09-08 2359.md"],
        "按名字倒序，最近的在上；同一分钟的序号大的在上"
    );
    assert_eq!(list[2].first_line, "标题在第三行", "跳过空行和 # 前缀");
    assert_eq!(list[1].first_line, "", "空文件摘要是空串");
    assert_eq!(
        list[3].first_line.chars().count(),
        SCRATCH_PREVIEW_CHARS,
        "摘要截到上限"
    );
    assert!(list.iter().all(|s| s.mtime_ms > 0));

    fs::remove_dir_all(d.parent().unwrap()).ok();
}

#[test]
fn 从可执行文件推_app_包路径() {
    assert_eq!(
        bundle_from_exe(Path::new("/Applications/lite-ide.app/Contents/MacOS/lite-ide")),
        Some(PathBuf::from("/Applications/lite-ide.app"))
    );
    // cargo run 出来的二进制不在 .app 里
    assert_eq!(bundle_from_exe(Path::new("/proj/target/debug/lite-ide")), None);
    assert_eq!(bundle_from_exe(Path::new("/x/Contents/MacOS/lite-ide")), None);
}

#[test]
fn 脚本走_open_a_且路径要引起来() {
    let s = cli_script(Path::new("/Users/me/my apps/lite-ide.app"));
    assert!(s.starts_with("#!/bin/sh\n"));
    assert!(
        s.contains("exec /usr/bin/open -a \"/Users/me/my apps/lite-ide.app\" \"$@\""),
        "{s}"
    );
    // 路径里的引号和 $ 要转义，否则 shell 会把它拆开
    let odd = cli_script(Path::new("/tmp/a\"b$c.app"));
    assert!(odd.contains("\"/tmp/a\\\"b\\$c.app\""), "{odd}");
}

/// 软链：是我们的才换（指向现在的脚本 / 旧 USAGE 教的 .app 里的二进制 / 头上有标记的旧脚本），别人的一概不碰；
/// 没装上时给的那句命令，那儿原来没东西就不带 `-f`，被别人占着就不给 —— 原来一律 `ln -sf`，抄进去就把别人的盖了
#[test]
fn 安装命令行工具_只换我们的软链_别人的不碰_补的命令不盖别人() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let d = sandbox("cli");
    let script = d.join("bin").join("lite");
    let dir = d.join("usr-local-bin");
    let link = dir.join("lite");
    fs::create_dir_all(&dir).unwrap();

    // 头一次：脚本可执行、软链指过去
    let r = install_cli(&script, "#!/bin/sh\necho 1\n", &link).unwrap();
    assert_eq!((r.link.clone(), r.replaced), (CliLink::Linked, false));
    assert_eq!(fs::metadata(&script).unwrap().permissions().mode() & 0o777, 0o755);
    assert_eq!(fs::read_link(&link).unwrap(), script);

    // 再装一次：覆盖脚本、换掉我们自己的软链
    let r = install_cli(&script, "#!/bin/sh\necho 2\n", &link).unwrap();
    assert_eq!((r.link, r.replaced), (CliLink::Linked, true));
    assert_eq!(read_text(&script).unwrap(), "#!/bin/sh\necho 2\n");

    // 别人的软链（指向别的工具）：不碰，说清是什么，不给命令
    let other = d.join("别的工具").join("lite");
    fs::remove_file(&link).unwrap();
    symlink(&other, &link).unwrap();
    let r = install_cli(&script, "#!/bin/sh\n", &link).unwrap();
    assert!(matches!(&r.link, CliLink::Taken(w) if w.contains("别的工具")), "{:?}", r.link);
    assert_eq!(fs::read_link(&link).unwrap(), other, "别人的软链被换了");
    assert_eq!(r.link.fix_cmd(&script, &link), None, "被别人占着时不该给命令（-f 会把它盖掉）");

    // 旧 USAGE 教的：软链到 .app 里的二进制（指过去的东西不在也认）
    fs::remove_file(&link).unwrap();
    symlink(d.join("Old.app/Contents/MacOS/lite-ide"), &link).unwrap();
    assert_eq!(install_cli(&script, "#!/bin/sh\n", &link).unwrap().link, CliLink::Linked);
    assert_eq!(fs::read_link(&link).unwrap(), script);

    // 以前装在别处、头上有标记的脚本
    let old = d.join("旧位置").join("lite");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    fs::write(&old, cli_script(Path::new("/Applications/lite-ide.app"))).unwrap();
    fs::remove_file(&link).unwrap();
    symlink(&old, &link).unwrap();
    assert_eq!(install_cli(&script, "#!/bin/sh\n", &link).unwrap().link, CliLink::Linked);

    // 真文件（用户自己放的）：不碰
    fs::remove_file(&link).unwrap();
    fs::write(&link, "别的 lite").unwrap();
    let r = install_cli(&script, "#!/bin/sh\n", &link).unwrap();
    assert_eq!(r.link, CliLink::Taken("一个文件".into()));
    assert_eq!(read_text(&link).unwrap(), "别的 lite", "用户的文件被盖了");

    // 目录写不进去、那儿原来没东西：脚本照写，补的命令不带 -f
    let r = install_cli(&script, "#!/bin/sh\n", &d.join("nope").join("lite")).unwrap();
    assert_eq!(r.link, CliLink::NoPermission { ours: false });
    let cmd = r.link.fix_cmd(&script, &d.join("nope").join("lite")).unwrap();
    assert!(cmd.starts_with("sudo ln -s \""), "{cmd}");

    // 目录写不进去、那儿是我们的旧软链：补的时候要 -f 换掉它
    fs::remove_file(&link).unwrap();
    symlink(&script, &link).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let r = install_cli(&script, "#!/bin/sh\n", &link).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(r.link, CliLink::NoPermission { ours: true });
    assert!(r.link.fix_cmd(&script, &link).unwrap().starts_with("sudo ln -sf "));

    fs::remove_dir_all(&d).ok();
}

/// frontmatter：写进去能读回来；没有头 / 头没收尾 / 正文里的 `---` 三种不误认；
/// 摘要跳过头；带头但没正文的算「空」，能被 discard 收掉
#[test]
fn 草稿锚点写读一致() {
    let a = Anchor {
        project: "/Users/x/proj".into(),
        branch: "main".into(),
        head: "b6176f3".into(),
        at: "src/lib/git/GitPane.svelte:347".into(),
    };
    let head = a.frontmatter();
    assert!(head.starts_with("---\nproject: /Users/x/proj\n"), "{head}");
    assert!(head.ends_with("---\n\n"), "头后面要留一个空行给光标：{head:?}");
    let text = format!("{head}# 标题\n正文");
    let (got, off) = frontmatter(&text);
    assert_eq!(got.as_ref(), Some(&a));
    assert_eq!(&text[off..], "# 标题\n正文", "正文起点要跳过头和那个空行");

    // 空字段不写
    let half = Anchor { branch: "dev".into(), ..Default::default() };
    assert_eq!(half.frontmatter(), "---\nbranch: dev\n---\n\n");
    assert_eq!(Anchor::default().frontmatter(), "", "全空不写头");

    // 不误认的三种
    assert_eq!(frontmatter("正文\n---\nbranch: x\n---\n"), (None, 0), "`---` 不在第一行");
    assert_eq!(frontmatter("---\nbranch: x\n没有收尾"), (None, 0), "头没收尾");
    assert_eq!(frontmatter(""), (None, 0));
    // 认不出的 key 跳过，不报错
    let (g, _) = frontmatter("---\nfoo: bar\nbranch: z\n---\n");
    assert_eq!(g.unwrap().branch, "z");

    // 真文件：摘要跳过头；带头没正文的算空
    let base = sandbox("anchor");
    let d = base.join("scratches");
    let p = create_scratch_with(&d, "2026-09-16 1200", Some(&a)).unwrap();
    let list = list_scratches(&d).unwrap();
    assert_eq!(list[0].anchor.as_ref(), Some(&a));
    assert_eq!(list[0].first_line, "", "只有头，摘要该是空");
    discard_empty_scratch(&d, &p).expect("带头没正文的草稿算空，要能收掉");
    assert!(!p.exists());

    let p = create_scratch_with(&d, "2026-09-16 1201", Some(&a)).unwrap();
    fs::write(&p, format!("{}记了一笔", a.frontmatter())).unwrap();
    assert_eq!(list_scratches(&d).unwrap()[0].first_line, "记了一笔");
    assert!(discard_empty_scratch(&d, &p).is_err(), "有正文的不能丢");
    // 头 6 行（--- 四个字段 ---），搜内容时第 7 行起才算
    assert_eq!(frontmatter_lines(&p), 6, "头占 6 行");
    let q = create_scratch_with(&d, "2026-09-16 1202", None).unwrap();
    fs::write(&q, "没有头\n").unwrap();
    assert_eq!(frontmatter_lines(&q), 0);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn 只丢得掉空的且在草稿目录里的东西() {
    let base = sandbox("discard");
    let d = base.join("scratches");
    let 空的 = create_scratch(&d, "2026-09-09 1030").unwrap();

    // 有内容的：一个字节都不许碰
    let 有内容 = create_scratch(&d, "2026-09-09 1031").unwrap();
    write_text(&有内容, "记了一半").unwrap();
    let e = discard_empty_scratch(&d, &有内容).unwrap_err();
    assert!(e.to_string().contains("有东西"), "实得：{e}");
    assert!(有内容.exists());

    // 草稿目录外面的：哪怕是空的也不行 ——
    // 判据是「在不在草稿目录里」，不是「是不是空的」，两条都要
    let 外面 = base.join("别人的空文件.md");
    fs::write(&外面, "").unwrap();
    let e = discard_empty_scratch(&d, &外面).unwrap_err();
    assert!(e.to_string().contains("不在草稿目录"), "实得：{e}");
    assert!(外面.exists());

    // 用 .. 拼出来的也一样：canonicalize 之后就露馅了
    let 绕路 = d.join("../别人的空文件.md");
    assert!(discard_empty_scratch(&d, &绕路).is_err());
    assert!(外面.exists());

    // 目录不行
    assert!(discard_empty_scratch(&d, &d).is_err());

    /*
     * **符号链接：绝不能顺着它去删别人。**
     *
     * 这条是 `symlink_metadata`（而不是 `metadata`）唯一的存在理由。
     * 改成跟随链接的 `metadata` 之后：链接的 `is_file()` 变成 true、
     * 跟随后的大小是 0、`canonicalize` 又把它解析回草稿目录里 ——
     * 三条判据全过，`remove_file` 删掉的是**链接指向的那个文件**，
     * 而链接本身还在。测这一条才拦得住那次改动。
     */
    let 目标 = create_scratch(&d, "2026-09-09 1032").unwrap();
    let 链接 = d.join("链接.md");
    std::os::unix::fs::symlink(&目标, &链接).unwrap();
    assert!(discard_empty_scratch(&d, &链接).is_err());
    assert!(目标.exists(), "顺着符号链接把目标文件删了");

    // 正常路径：真的没了
    discard_empty_scratch(&d, &空的).unwrap();
    assert!(!空的.exists());

    fs::remove_dir_all(base).ok();
}

#[test]
fn 新建文件夹撞名要报错而不是复用() {
    let d = sandbox("create-dir");
    let sub = d.join("已有目录");
    fs::create_dir(&sub).unwrap();

    // 前提：create_dir_all 对已存在的目录返回 Ok —— 用它就会「新建成功」
    // 一个早就存在的目录，界面报了「已新建」而盘上什么都没发生
    assert!(fs::create_dir_all(&sub).is_ok(), "前提不成立");

    let e = create_entry(&d, "已有目录", true).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);

    let p = create_entry(&d, "新目录", true).unwrap();
    assert!(p.is_dir());
    fs::remove_dir_all(d).ok();
}

#[test]
fn 新建撞的是一个文件还是目录都要拦() {
    // 撞名检查看的是「这个名字被占了没有」，不该只在同类之间比
    let d = sandbox("create-cross");
    fs::create_dir(d.join("x")).unwrap();
    // 断言的是**我们自己那句中文**，不只是 is_err()：std 对跨类型撞名
    // 本来就会 EEXIST，只断言"报错了"的话，把预检查改成只在同类之间比
    // 也照样绿（试过）
    let e = create_entry(&d, "x", false).unwrap_err();
    assert!(e.to_string().contains("已经有一个叫"), "同名目录占着，实得：{e}");

    write_text(d.join("y"), "内容").unwrap();
    let e = create_entry(&d, "y", true).unwrap_err();
    assert!(e.to_string().contains("已经有一个叫"), "同名文件占着，实得：{e}");
    assert_eq!(read_text(d.join("y")).unwrap(), "内容");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 改名不覆盖已存在的目标() {
    let d = sandbox("rename-clobber");

    // 前提：fs::rename 静默覆盖 —— 这是 rename(2) 的语义，不是 Rust 的选择
    let a = d.join("a0.txt");
    let b = d.join("b0.txt");
    write_text(&a, "源").unwrap();
    write_text(&b, "本来的 b").unwrap();
    fs::rename(&a, &b).unwrap();
    assert_eq!(read_text(&b).unwrap(), "源", "前提不成立：rename 不再覆盖了？");
    assert!(!a.exists());

    // 我们的：拦住，两边内容都不变
    let x = d.join("x.txt");
    let y = d.join("y.txt");
    write_text(&x, "我是 x").unwrap();
    write_text(&y, "我是 y").unwrap();
    let e = rename_entry(&x, "y.txt").unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(read_text(&x).unwrap(), "我是 x");
    assert_eq!(read_text(&y).unwrap(), "我是 y", "被覆盖的话这里就是「我是 x」");

    fs::remove_dir_all(d).ok();
}

#[test]
fn 改名之后旧路径没了新路径在() {
    let d = sandbox("rename-ok");
    let a = d.join("旧名.java");
    write_text(&a, "内容").unwrap();

    let to = rename_entry(&a, "新名.java").unwrap();
    assert_eq!(to, d.join("新名.java"));
    assert!(!a.exists(), "旧路径还在");
    assert_eq!(read_text(&to).unwrap(), "内容", "改名不该动内容");

    // 名字没变：直接返回，不去惊动文件系统
    let same = rename_entry(&to, "新名.java").unwrap();
    assert_eq!(same, to);
    assert_eq!(read_text(&to).unwrap(), "内容");

    fs::remove_dir_all(d).ok();
}

#[test]
fn 只改大小写要能成功() {
    // APFS 默认大小写不敏感：a.txt → A.txt 时目标"已存在"，
    // 而存在的正是源文件自己。靠 dev+ino 认出这一点，不然这条永远失败 ——
    // 而「把 readme.md 改成 README.md」是真实需求
    let d = sandbox("rename-case");
    let a = d.join("readme.md");
    write_text(&a, "内容").unwrap();

    let to = rename_entry(&a, "README.md").unwrap();
    assert_eq!(to, d.join("README.md"));

    let names: Vec<String> = list_dir(&d).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["README.md"], "盘上的名字没跟着换大小写");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 改名的目标是坏软链也不许覆盖() {
    // to.exists() 会跟随软链，于是一个指向已删除目标的坏链接被判成
    // "不存在"，然后 rename 把链接本身覆盖掉。这条是 exists 和
    // symlink_metadata 的分水岭，和 reveal 那边同一个判据
    let d = sandbox("rename-symlink");
    let src = d.join("src.txt");
    write_text(&src, "源").unwrap();
    let link = d.join("断链");
    std::os::unix::fs::symlink(d.join("目标早没了"), &link).unwrap();
    assert!(!link.try_exists().unwrap(), "前提：try_exists 判它不存在");

    let e = rename_entry(&src, "断链").unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
    assert!(fs::symlink_metadata(&link).unwrap().is_symlink(), "链接被覆盖掉了");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 改名先校验名字() {
    let d = sandbox("rename-name");
    let a = d.join("a.txt");
    write_text(&a, "x").unwrap();
    // 带斜杠的名字要在碰盘之前就被挡下，否则就是「改名」变成「移动到别处」
    let e = rename_entry(&a, "../跑出去.txt").unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(a.exists());
    fs::remove_dir_all(d).ok();
}

#[test]
fn 改不存在的东西要报中文() {
    let d = sandbox("rename-missing");
    let e = rename_entry(d.join("没有这个.txt"), "新名").unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
    assert!(e.to_string().contains("已不存在"), "实得：{e}");
    fs::remove_dir_all(d).ok();
}

#[test]
fn 废纸篓拒绝不存在的路径() {
    // 和 reveal 一样：自己判一次给一句中文，而不是把 trash 的英文透出来
    let e = move_to_trash("/这个路径/根本/不存在.txt").unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotFound);
    assert!(e.to_string().contains("已不存在"), "实得：{e}");
}

/// 都已经不在盘上：要的就是它们不在了，不报错、也不去叫 Finder
#[test]
fn 批量扔_都不在了就算成功() {
    assert_eq!(move_all_to_trash(&[PathBuf::from("/这个路径/根本/不存在.txt")]), Ok(()));
    assert_eq!(move_all_to_trash(&[]), Ok(()));
}

/// 同下面那条，**默认不跑**：批量那条路（`trash::delete_all`）也要有人真的跑过一次
///
/// ```bash
/// cargo test -p fsservice -- --ignored 真的把一批文件移进废纸篓
/// ```
#[test]
#[ignore = "会往用户的废纸篓里扔文件"]
fn 真的把一批文件移进废纸篓() {
    let d = sandbox("trash-batch");
    let a = d.join("lite-ide-废纸篓测试-批量-a.txt");
    let b = d.join("lite-ide-废纸篓测试-批量-b.txt");
    write_text(&a, "a").unwrap();
    write_text(&b, "b").unwrap();
    move_all_to_trash(&[a.clone(), b.clone(), d.join("本来就没有.txt")]).unwrap();
    assert!(fs::symlink_metadata(&a).is_err() && fs::symlink_metadata(&b).is_err(), "还在原处 —— delete_all 什么都没做");
    fs::remove_dir_all(d).ok();
}

/// **默认不跑**：它会往跑测试的人的废纸篓里真的扔一个文件进去。
///
/// 但这条路必须有人验过 —— 上面那条只测了守卫，一行 `trash::delete`
/// 换成 `Ok(())` 它照样绿。手动跑：
///
/// ```bash
/// cargo test -p fsservice -- --ignored 真的把文件移进废纸篓
/// ```
#[test]
#[ignore = "会往用户的废纸篓里扔文件"]
fn 真的把文件移进废纸篓() {
    let d = sandbox("trash-real");
    let f = d.join("lite-ide-废纸篓测试.txt");
    write_text(&f, "这个文件应该出现在废纸篓里").unwrap();

    move_to_trash(&f).unwrap();
    assert!(
        fs::symlink_metadata(&f).is_err(),
        "文件还在原处 —— trash::delete 什么都没做"
    );
    fs::remove_dir_all(d).ok();
}

// ── 图片预览（G26）：扩展名和文件头都对上才算图 ──

#[test]
fn 图片_扩展名和文件头都对上才算() {
    assert_eq!(image::sniff("png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Some("image/png"));
    assert_eq!(image::sniff("jpeg", b"\xff\xd8\xff\xe0"), Some("image/jpeg"));
    assert_eq!(image::sniff("gif", b"GIF89a.."), Some("image/gif"));
    assert_eq!(image::sniff("webp", b"RIFF\x10\0\0\0WEBPVP8 "), Some("image/webp"));
    assert_eq!(image::sniff("ico", b"\0\0\x01\0\x01\0"), Some("image/x-icon"));
    // 改了名的文本：扩展名说是图，文件头说不是 —— 交回编辑器
    assert_eq!(image::sniff("png", b"just some notes\n"), None);
    // 光标文件（类型 2）不是图标
    assert_eq!(image::sniff("ico", b"\0\0\x02\0"), None);
    assert_eq!(image::sniff("pdf", b"%PDF-1.7\n"), Some("application/pdf"));
    assert_eq!(image::sniff("pdf", b"not a pdf"), None);
    // 不认识的扩展名，文件头再像也不管
    assert_eq!(image::sniff("dat", b"\x89PNG\r\n\x1a\n"), None);
}

#[test]
fn 图片_svg认标签不认开头() {
    let head = b"<?xml version=\"1.0\"?>\n<!-- by hand -->\n<SVG xmlns=\"http://www.w3.org/2000/svg\"/>";
    assert_eq!(image::sniff("svg", head), Some("image/svg+xml"));
    assert_eq!(image::sniff("svg", b"<html><body>not an svg</body></html>"), None);
}

#[test]
fn 图片_大小写扩展名_读盘() {
    let d = sandbox("image-ext");
    let f = d.join("Logo.PNG");
    std::fs::write(&f, b"\x89PNG\r\n\x1a\nrest").unwrap();
    assert_eq!(image_mime(&f), Some("image/png"));
    assert_eq!(read_image(&f).unwrap().len(), 12);
    fs::remove_dir_all(d).ok();
}

#[test]
fn 图片_超过上限报错不截断() {
    let d = sandbox("image-cap");
    let f = d.join("big.png");
    std::fs::write(&f, vec![7u8; 101]).unwrap();
    assert_eq!(image::read_image_capped(&f, 101).unwrap().len(), 101, "正好等于上限要读得出来");
    let e = image::read_image_capped(&f, 100).unwrap_err();
    assert!(e.to_string().contains("不预览"), "{e}");
    fs::remove_dir_all(d).ok();
}
