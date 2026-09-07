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
//! **这里只回答「哪些名字是生成物」，不回答「该藏还是该折叠」。**
//! 后者是 #13 还没定的事（倾向照 IDEA 折叠而不是隐藏），定下来之后
//! 要改的是调用方，不是这份名单。

/// 生成物目录：**里面没有一个文件是手写的**。
///
/// 判据是「谁写的」，不是「名字带不带点」—— 点文件和点目录在文件树里一律列出来
/// （`.gitignore` `.github/` `.claude/` 是天天要改的项目文件），
/// 而这些不列，因为里面几万个文件全是工具生成的，`target/` 在本仓库就有 1GB 多。
///
/// `dist` 和 `build` 这两个名字是有争议的 —— 有的项目 `build/` 里放的是构建
/// 脚本（CMake 尤其常见）。争议本身是 #13 的正题，这份名单不解决它，
/// 只保证「争议的后果两边一致」。
pub const GENERATED_DIRS: &[&str] = &[
    "node_modules", // npm / pnpm / yarn
    "target",       // cargo
    "dist",         // 打包产物的通用名
    "build",        // 同上
    "venv",         // python virtualenv（点号那版在 GENERATED_DOT_DIRS）
    "__pycache__",  // python 字节码
    "vendor",       // go mod vendor / composer
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

/// 这个目录名是不是生成物。文件树用它。
pub fn is_generated_dir(name: &str) -> bool {
    GENERATED_DIRS.contains(&name)
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
