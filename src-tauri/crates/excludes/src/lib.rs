//! 哪些目录不进视野 —— 文件树与搜索**共用的唯一一份名单**。
//!
//! 拆出来之前是两份，而且不一样：`fsservice::BUILD_DIRS` 四个
//! （`node_modules` `target` `dist` `build`），`searchsvc::SKIP_DIRS` 十四个。
//! 两份名单各自都说得通，合在一起就成了一个说不清的行为：
//! 一个真叫 `build/` 的源码目录在这个应用里**完全够不着** ——
//! 树里没有、⌘P 搜不到、⇧⌘F 也搜不到（rg 那条路也照名单传了 `--glob !`）。
//! 而两份名单还会各自演化，改一处漏一处只是时间问题。见 issue #13。
//!
//! 单独一个 crate 而不是塞进 fsservice：searchsvc 也要用，
//! 而「搜索依赖文件树」这条箭头是错的 —— 还会顺手把 encoding_rs / chardetng /
//! trash 拽进搜索的依赖图，只为了一个字符串数组。这个 crate 零依赖。
//!
//! # 这里只回答「名字」，不回答「该怎么办」（2026-09-10 定下来了）
//!
//! 名字判得了 `node_modules`，判不了 `dist` 和 `build` —— 那两个是常见的
//! 源码目录名。**名字只是怀疑，git 忽不忽略它才是证据。** 所以名单分成两档
//! （[`CERTAIN_GENERATED_DIRS`] / [`CONTESTED_DIRS`]），而「怀疑之后做什么」
//! 由两边各自回答，判据不一样：
//!
//! | | 怎么办 | 谁去问 git |
//! |---|---|---|
//! | 文件树 | **压暗，不隐藏** | 前端按项目问一次（`ignored_dirs` 命令），`list_dir` 只传「怀疑」 |
//! | 搜索 | **不进** | 命令层在每次搜索前问一次，结果做成 `searchsvc::Skip` |
//!
//! 两边问的是同一个 `gitsvc::ignored_dirs`，所以答案一致 ——
//! **这一条是硬要求**：树里压暗的东西搜得到、树里正常的东西搜不到，
//! 比两边都错更难理解。
//!
//! 这个 crate 仍然零依赖，也不知道 git 的存在。

/// **只可能是生成物**的名字。见到就跳，不用问任何人。
///
/// 判据是「这个名字有没有**第二种**可能」。这四个没有：
/// 没有人手写一个叫 `node_modules` 的源码目录，`target/` 里那 1GB 也不是
/// 谁敲出来的。跳掉它们不会误伤，所以不必为它们多起一个 git 子进程。
///
/// （和「名字带不带点」无关 —— 点文件和点目录在文件树里一律列出来，
/// `.gitignore` `.github/` `.claude/` 是天天要改的项目文件。）
pub const CERTAIN_GENERATED_DIRS: &[&str] = &[
    "node_modules", // npm / pnpm / yarn
    "target",       // cargo / maven
    "venv",         // python virtualenv（点号那版在 GENERATED_DOT_DIRS）
    "__pycache__",  // python 字节码
];

/// **经常是生成物，但也经常是源码**的名字。见到要先问 git。
///
/// 这三个名字有第二种可能，而且都不罕见：
///
/// | 名字 | 另一种身份 |
/// |---|---|
/// | `build` | CMake 项目把构建脚本放这儿；Gradle 项目里它又确实是产物 |
/// | `dist` | 有的项目把要提交的产物放这儿（浏览器扩展、单文件库） |
/// | `vendor` | Go 的 `vendor/` 是**提交进仓库**的依赖，composer 的不是 |
///
/// 只按名字跳掉它们的后果是 issue #13 记的那个：一个真叫 `build/` 的源码
/// 目录在这个应用里凭空消失，**而且没有任何提示**。
///
/// **名字只是怀疑，git 忽不忽略它才是证据**（`gitsvc::ignored_dirs`）。
/// 问不到 git（不是仓库、git 不在）时退回按名字跳 —— 那时我们没有别的
/// 证据，而「多列出一个 node_modules」和「少列一个源码目录」里，
/// 后者才是没有提示的那个。
pub const CONTESTED_DIRS: &[&str] = &["dist", "build", "vendor"];

/// 两档合起来。**只在「没有 git 信息」那条退路上用**。
pub const GENERATED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "venv",
    "__pycache__",
    "vendor",
];

/// 点号开头的生成物目录。
///
/// **只有搜索侧用得上。** 文件树对点目录一律列出来（理由见
/// `fsservice::list_dir`），所以这份名单在树那边没有意义；
/// 搜索则一律不进点目录，这里逐个列出来是为了把同一条规则也交给 `rg`
/// —— 装了 rg 和没装 rg 搜出来的结果不能不一样。
pub const GENERATED_DOT_DIRS: &[&str] = &[
    ".git", ".next", ".nuxt", ".venv", ".gradle", ".idea", ".vscode",
];

/// 这个目录名是不是生成物 —— **只是「名字像」，不是判决**。
///
/// `fsservice::list_dir` 用它给 `Entry.generated` 赋值，那一位传到前端，
/// 前端再拿 git 的答案（`ignored_dirs` 命令）把有争议的那几个对一遍。
/// 判「要不要对」的是 [`is_contested_dir`]。
pub fn is_generated_dir(name: &str) -> bool {
    GENERATED_DIRS.contains(&name)
}

/// 这个名字**不用问 git 就能跳**。
pub fn is_certain_generated_dir(name: &str) -> bool {
    CERTAIN_GENERATED_DIRS.contains(&name)
}

/// 这个名字**要问过 git 才敢跳**。
pub fn is_contested_dir(name: &str) -> bool {
    CONTESTED_DIRS.contains(&name)
}

/// 搜索要跳过的全部目录名 = 生成物 + 点号版生成物。
///
/// 给成迭代器而不是再拼一个常量数组：searchsvc 有两处要用（进程内遍历、
/// 传给 rg 的 `--glob`），两处必须是同一批名字 —— 这正是当初分岔的形状。
pub fn search_skip_dirs() -> impl Iterator<Item = &'static str> {
    GENERATED_DIRS
        .iter()
        .chain(GENERATED_DOT_DIRS.iter())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 生成物名单不带点_点名单全带点() {
        // 分组判据要能一眼看出来：搜索侧靠「点开头一律跳过」兜底，
        // 一个不带点的名字混进 GENERATED_DOT_DIRS 就等于在树那边悄悄失效了
        assert!(
            GENERATED_DIRS.iter().all(|d| !d.starts_with('.')),
            "带点的生成物目录属于 GENERATED_DOT_DIRS"
        );
        assert!(
            GENERATED_DOT_DIRS.iter().all(|d| d.starts_with('.')),
            "不带点的名字放这里，文件树就看不到它了"
        );
    }

    /*
     * **两档拼起来必须正好是 `GENERATED_DIRS`。**
     *
     * 三份名单摆在一起，加一个名字时漏改一份是迟早的事 —— 而漏改的后果
     * 是静默的：漏在 `GENERATED_DIRS` 里，树不压暗它；漏在两档里，
     * 搜索照旧按老规矩跳。都不报错，只是「某个地方不太对」。
     */
    #[test]
    fn 两档拼起来正好是全集() {
        let mut 两档: Vec<&str> = CERTAIN_GENERATED_DIRS
            .iter()
            .chain(CONTESTED_DIRS.iter())
            .copied()
            .collect();
        let mut 全集: Vec<&str> = GENERATED_DIRS.to_vec();
        两档.sort_unstable();
        全集.sort_unstable();
        assert_eq!(两档, 全集, "改了一份名单，另外两份没跟着改");
    }

    #[test]
    fn 两档不许交叉() {
        for d in CONTESTED_DIRS {
            assert!(
                !CERTAIN_GENERATED_DIRS.contains(d),
                "{d} 同时在两档里 —— 「要不要问 git」就成了两个答案"
            );
        }
    }

    #[test]
    fn 名单里没有重复项() {
        let mut all: Vec<&str> = search_skip_dirs().collect();
        let n = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), n, "重复项说明两份名单又开始各写各的了");
    }

    #[test]
    fn 判断函数只认整个名字() {
        assert!(is_generated_dir("node_modules"));
        assert!(!is_generated_dir("node_modules_old"), "不是前缀匹配");
        assert!(!is_generated_dir("src"));
        assert!(!is_generated_dir(".git"), "点目录不归文件树管");
    }
}
