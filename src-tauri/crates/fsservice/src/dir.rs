//! 列目录：一层的条目、只有一个子目录的链折成一行（文件树用）

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    /// 名字命中了 [`excludes::GENERATED_DIRS`] —— **这是「怀疑」，不是「判决」**。
    ///
    /// 界面据此把它压暗、不自动展开、也不预取子目录，但它**在树里**、
    /// 点得开、里面的文件打得开。判据见 `list_dir` 的说明。
    pub generated: bool,
    /// 这个名字**有第二种可能**（`dist` / `build` / `vendor`），
    /// 要拿 git 的答案对一遍才算数。见 [`excludes::CONTESTED_DIRS`]。
    ///
    /// 为什么这一位要传给前端：这个 crate 答不了「git 忽不忽略它」
    /// （零 git 依赖，也不该有），而**每展开一层都去问一次 git 太浪费** ——
    /// 同一个项目的答案是同一份。所以前端按项目问一次，拿这一位决定
    /// 「这一条要不要用那份答案覆盖掉名字的判断」。
    ///
    /// `generated` 为假时这一位没有意义（恒为假）。
    pub contested: bool,
    /// 从这个目录往下「里面只有一个子目录、别的什么都没有」的那一串名字，**不含它自己**。
    /// `java/` 底下只有 `com/demo/order` 这一路的话，`java` 这一项带 `["com", "demo", "order"]`
    /// 的是 `com` —— 见 [`single_child_chain`]。文件和不探的目录恒为空。
    ///
    /// 这是**结构上的事实**，不是显示规则：合不合并、用 `/` 还是 `.` 连、在源码根那儿断开，
    /// 全是前端 `tree-rows.ts` 的事。这里只负责「一次 IPC 就把这一串读回来」——
    /// 文件树是懒展开的，不提前往下读，就不知道 `src` 底下是不是只有 `main`。
    pub chain: Vec<String>,
}

/// 单子目录链最多往下探几层。正常项目五六层（`com/company/project/module`），
/// 再深就是造出来的病态目录，不值得一路读下去。
pub(crate) const CHAIN_MAX_DEPTH: usize = 32;

/// 一次 [`list_dir`] 最多为探链读多少个目录。
///
/// 每个子目录要多读一次（`read_dir` 只取前两项就停，一次几十微秒），一层几千个子目录时
/// 列一层目录就会变成读几千个目录。超了预算的就不探 —— 界面上只是那几行不合并，
/// 不会错。
pub(crate) const CHAIN_BUDGET: usize = 512;

/// 从 `dir` 往下走，每一层**只有一个条目、而且它是真目录**就继续，返回沿途的名字。
///
/// 停下的几种情况：分叉了（两个以上条目）、空了、唯一那一项是文件或软链、
/// 读不了、到了深度上限或预算用完，以及唯一那一项是生成物 / 有争议的目录 ——
/// 那种目录要自己占一行，合并进去它就没法被单独压暗了。
///
/// **软链不跟**：跟了可能绕回自己；而且合并出来的那一行说的是「盘上就是这一串目录」，
/// 软链不是。（`list_dir` 本来也不把指向目录的软链当目录。）
fn single_child_chain(dir: &Path, budget: &mut usize) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cur = dir.to_path_buf();
    while chain.len() < CHAIN_MAX_DEPTH && *budget > 0 {
        *budget -= 1;
        let Ok(rd) = fs::read_dir(&cur) else { break };
        let mut only = None;
        let mut forked = false;
        for ent in rd {
            let Ok(ent) = ent else {
                forked = true; // 读到一半出错：当它分叉了，不冒险合并
                break;
            };
            // 树里不列的东西探链时也不算数（`.DS_Store` 不许把链从中间截断）
            if excludes::is_hidden(&ent.file_name().to_string_lossy()) {
                continue;
            }
            if only.is_some() {
                forked = true;
                break;
            }
            only = Some(ent);
        }
        let Some(ent) = only.filter(|_| !forked) else { break };
        if !ent.file_type().is_ok_and(|t| t.is_dir()) {
            break;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        if excludes::is_generated_dir(&name) || excludes::is_contested_dir(&name) {
            break;
        }
        cur.push(&name);
        chain.push(name);
    }
    chain
}

/// 列出一层目录。不递归 —— 文件树按需展开，避免大仓库一次性遍历。
///
/// **点文件和点目录列出来**（例外见下）。原来它们跟着 `show_hidden` 一起被藏了，
/// 于是 `.gitignore` `.github/` `.env` `.claude/` 这些**天天要改的项目文件**
/// 在文件树里根本不存在，只能靠 ⌘P 摸黑打开。
///
/// 藏它们的那个理由（「否则文件树被淹没」）说的其实是 `node_modules` 那一类，
/// 而那一类现在由 [`excludes::GENERATED_DIRS`] 单独挡着 —— 两件事本来就不该
/// 共用一个开关。
///
/// **例外是 [`excludes::is_hidden`]**：版本库目录（`.git` `.svn` `.hg` `CVS`）和系统杂物
/// （`.DS_Store` `Thumbs.db`）不列 —— 它们不是项目的内容。`.git/` 原来照列（「懒展开，不点开
/// 就只是一行」），2026-09-24 起不列：那条理由只算了成本，没算树上能对它右键删除、改名、
/// 往里拖文件。搜索那边用的是同一个函数。
///
/// 每个子目录顺手探一条单子目录链（[`Entry::chain`]），有深度上限和预算。
///
/// 那份名单是**和搜索共用的同一份**。原来这里自己有四个、searchsvc 自己有十四个，
/// 于是「树里看不见」和「⌘P 搜不到」是两套判据，各自演化。
///
/// # 生成物目录**列出来，只是压暗**（issue #13，2026-09-10 改）
///
/// 原来是 `continue` 掉的 —— 名字命中就当它不存在。那条判据有个说不清的后果：
///
/// > `dist` 和 `build` 是**常见的源码目录名**。CMake 项目的 `build/` 里放的
/// > 是构建脚本，有的项目 `dist/` 里放的是要提交的产物。这种项目在 lite-ide
/// > 里看到的是一个**凭空少了一个目录**的文件树，而且没有任何提示。
///
/// 「名字叫 build」只是**怀疑**，不是证据。真正的证据是 git 忽不忽略它，
/// 而这个 crate 零 git 依赖（也不该有 —— 起 git 必须走 `gitsvc::git_cmd` 那套
/// 加固，否则 `.git/config` 就能让一次列目录去执行任意命令）。
///
/// 所以这里不再替界面做决定：**照列，打上 `generated` 标记**，
/// 「压暗还是隐藏」由界面回答。信息不丢，噪声也不进来 —— IDEA 对
/// excluded 目录就是这么做的。
///
/// **搜索那半也让 git 说话了**（`searchsvc::Skip`，同一天做的）：
/// 确定是生成物的按名字跳，有争议的按 `gitsvc::ignored_dirs` 的答案跳。
/// 两边问的是同一个函数，所以答案一致 —— 树里压暗的东西搜得到、
/// 树里正常的东西搜不到，比两边都错更难理解。
///
/// 排序：目录在前，同类按名称不区分大小写排列，与 Finder / IDEA 一致。
/// **生成物不单独排到末尾** —— 挪位置比压暗更让人意外，而且 `target/` 一旦
/// 换了位置，「它刚才还在这儿」这种困惑比看见它更贵。
pub fn list_dir(dir: impl AsRef<Path>) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for ent in fs::read_dir(dir.as_ref())? {
        let ent = ent?;
        let name = ent.file_name().to_string_lossy().into_owned();
        if excludes::is_hidden(&name) {
            continue;
        }
        let meta = match ent.metadata() {
            Ok(m) => m,
            // 断掉的软链等：跳过而不是整个目录失败
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        out.push(Entry {
            // 只有目录才谈得上「生成物目录」。一个叫 `build` 的**文件**
            // （shell 脚本很常见）压暗它没有任何道理
            generated: is_dir && excludes::is_generated_dir(&name),
            contested: is_dir && excludes::is_contested_dir(&name),
            name,
            path: ent.path(),
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
            chain: Vec::new(),
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    // 排完序再探：预算不够时，先吃到预算的是界面上排在前面的那些
    let mut budget = CHAIN_BUDGET;
    for e in out.iter_mut().filter(|e| e.is_dir && !e.generated && !e.contested) {
        e.chain = single_child_chain(&e.path, &mut budget);
    }
    Ok(out)
}
