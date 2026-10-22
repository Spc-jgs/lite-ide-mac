//! 这台机器上的 JDK：自动找到的（`lspsvc::jdtls::default_jdk_dirs` 那几处）+ 设置里手动加的（`java.jdks`，docs/SETTINGS.md 13.4）。
//!
//! 跑程序、构建、跑单测挑 JDK（`taskdefs::jdk_prefix`）、设置页的 JDK 列表、jdtls 认的 runtimes 都从这一份来 ——
//! 原来跑程序扫三处、jdtls 自己跑 `java_home -X`，两边看到的不一样：sdkman 装的 JDK 运行用得上，jdtls 却说找不到（13.1）

use std::path::{Path, PathBuf};

/// 手动加的那几个目录（`java.jdks`），设置读到 / 变了时由 `settingsctl` 设进来
static USER: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

pub fn set_user(dirs: Vec<String>) {
    *USER.write().unwrap_or_else(|e| e.into_inner()) = dirs;
}

pub fn user() -> Vec<String> {
    USER.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 全部 JDK：(主版本, JAVA_HOME)，按版本排，同一个 JDK（按真实路径）只列一次。手动加的认不出来的（目录没了、不是 JDK）不列 ——
/// 设置页上那一行另外标出来（[`at`] 给 None）
pub fn all() -> Vec<(u32, PathBuf)> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut out = lspsvc::jdtls::list_jdks(&lspsvc::jdtls::default_jdk_dirs(&home));
    let mut seen: std::collections::HashSet<PathBuf> = out.iter().map(|(_, h)| real(h)).collect();
    for d in user() {
        if let Some((v, h)) = at(&d) {
            if seen.insert(real(&h)) {
                out.push((v, h));
            }
        }
    }
    out.sort();
    out
}

/// 一个目录是不是 JDK（手动加的、项目或运行配置里选的）：给 JAVA_HOME 或者 macOS 的包目录都认
pub fn at(dir: &str) -> Option<(u32, PathBuf)> {
    let d = dir.trim();
    if d.is_empty() {
        return None;
    }
    lspsvc::jdtls::jdk_at(Path::new(d))
}

fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}
