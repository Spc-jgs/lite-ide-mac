//! 搜索：被 git 忽略的目录、项目文件索引、全文搜索。

use super::*;

/// 这个项目里哪些目录被 git 忽略了（相对项目根）。
///
/// **给文件树用，前端按项目问一次。** 名字叫 `build` 不等于它是生成物
/// （issue #13），而 `list_dir` 答不了这个 —— `fsservice` 零 git 依赖。
/// 每展开一层都问一次也不行：同一个项目的答案是同一份，而一次树刷新
/// 会重列每一个展开着的目录，那就是十来次子进程换同一个答案
/// （本仓库实测一次 11.5ms / 19 条）。
///
/// **`null` 和 `[]` 是两件事**：`null` = 问不到 git（不是仓库、git 不在），
/// 那时前端退回按名字判；`[]` = 问到了，一个都没忽略。
/// 合成一个的话，一个干净的非 git 目录会被当成「git 说什么都没忽略」，
/// 于是 `node_modules` 也不压暗了。
/// 走阻塞池而不是主线程：输出规模由**仓库**说了算（本仓库 11.5ms，
/// 而一个忽略了几十万文件的 monorepo 是另一个数量级），
/// 判据同 `list_project_files` —— 时长不可控的活不留在主线程上。
#[tauri::command]
pub async fn ignored_dirs(root: String) -> Result<Option<Vec<String>>, String> {
    blocking(move || {
        Ok(gitsvc::ignored_dirs(Path::new(&root))
            .ok()
            .map(|s| s.into_iter().collect()))
    })
    .await
}

/// 搜索开始之前问一次 git：这个项目里哪些目录是被忽略的。
///
/// # 为什么这一句在命令层，而不在 searchsvc 里面
///
/// `searchsvc` 判「这个目录跳不跳」需要 git 的答案（issue #13：名字叫
/// `build` 不等于它是生成物），但**「搜索依赖 git」是条错箭头** ——
/// 和当初把名单拆成 `excludes` crate 时说的「搜索依赖文件树是错的」
/// 是同一条判据。所以 searchsvc 只收一份算好的 `Skip`，不认识 git。
///
/// 于是拼接这一步只能落在命令层。它是**组装，不是业务**：
/// 判据全在两个 crate 里，这里既不决定哪些目录该跳，也不决定跳了怎么办。
///
/// 问不到就退回按名字判（不是 git 仓库、git 不在、仓库太大读不动）——
/// **绝不能因此把整次搜索判成失败**。
fn skip_for(root: &str) -> searchsvc::Skip {
    match gitsvc::ignored_dirs(std::path::Path::new(root)) {
        Ok(ignored) => searchsvc::Skip::with_git(ignored),
        Err(e) => {
            crate::diag!("ignored_dirs({root}) 失败，退回按名字判：{e}");
            searchsvc::Skip::by_name()
        }
    }
}

/// 列出项目里的文件（相对路径），供前端做模糊匹配。
///
/// 匹配放前端做是有意为之：每敲一个字符都往 Rust 跑一趟，IPC 往返会让输入发木。
/// 几万条路径传过去也就几 MB。
#[tauri::command]
pub async fn list_project_files(root: String) -> Result<ProjectFilesDto, String> {
    blocking(move || {
        let (files, truncated) =
            searchsvc::list_files_capped(&root, &skip_for(&root)).map_err(|e| format!("索引项目失败：{e}"))?;
        Ok(ProjectFilesDto { files, truncated })
    })
    .await
}

/// 全局内容搜索。有 rg 用 rg，没有就用进程内实现，两者结果一致。
#[tauri::command]
pub async fn grep_project(root: String, pattern: String, limit: usize) -> Result<Vec<HitDto>, String> {
    blocking(move || {
        let hits = searchsvc::grep(&root, &pattern, limit, &skip_for(&root))
            .map_err(|e| format!("搜索失败：{e}"))?;
        Ok(hits
            .into_iter()
            .map(|h| HitDto {
                path: h.path,
                line: h.line,
                text: h.text,
            })
            .collect())
    })
    .await
}
