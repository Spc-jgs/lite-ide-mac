//! 文件系统：探路径、列目录、读写文本、编码、剪贴板、新建 / 移动 / 改名 / 废纸篓、文件监听。

use super::*;

/// 探测一个路径：是目录还是文件，文件该用哪种模式打开。
///
/// 判据是复合的（ARCHITECTURE.md §1 修正 01）：大小 / 行数 / 最长行任一超标都走
/// 日志模式。只读文件头部采样，不加载全文。
#[tauri::command]
pub async fn probe_path(path: String) -> Result<PathInfo, String> {
    blocking(move || {
        // 前端拿返回的 path 当 key（标签、项目根、和仓库根的前缀匹配），
        // 所以在这一个入口把符号链接整理掉，见 `fsservice::canonical`
        let path = fsservice::canonical(&path).to_string_lossy().into_owned();
        let p = Path::new(&path);
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let meta = std::fs::metadata(p).map_err(|e| format!("读不到 {path}：{e}"))?;

        if meta.is_dir() {
            return Ok(PathInfo {
                kind: "dir",
                mode: "edit",
                path,
                name,
                size: 0,
                reason: String::new(),
            });
        }

        let pr = logengine::probe(p).map_err(|e| format!("探测 {path} 失败：{e}"))?;
        crate::diag!("probe {path} -> {:?} ({})", pr.mode, pr.reason);
        Ok(PathInfo {
            kind: "file",
            mode: pr.mode.as_str(),
            path,
            name,
            size: pr.size,
            reason: pr.reason.to_string(),
        })
    })
    .await
}

/// 列一层目录。不递归 —— 文件树按需展开，大仓库才不会卡。
///
/// 没有 `show_hidden` 这个开关了：点文件一律列，构建产物一律不列。
/// 留一个前端永远传同一个值的参数，就是下一个没人敢动的死开关。
#[tauri::command]
pub async fn list_dir(path: String) -> Result<Vec<DirEntryDto>, String> {
    blocking(move || {
        let entries = fsservice::list_dir(&path).map_err(|e| format!("列目录失败：{e}"))?;
        Ok(entries
            .into_iter()
            .map(|e| DirEntryDto {
                name: e.name,
                path: e.path.to_string_lossy().into_owned(),
                is_dir: e.is_dir,
                size: e.size,
                generated: e.generated,
                contested: e.contested,
                chain: e.chain,
            })
            .collect())
    })
    .await
}

/// 编辑模式读取全文，自动探测编码。
///
/// `label` 非空时按指定编码读（用户点了「以其他编码重新打开」）。
#[tauri::command]
pub async fn read_text(path: String, label: Option<String>) -> Result<TextDto, String> {
    blocking(move || {
        let d = fsservice::read_text_detect(&path, label.as_deref().unwrap_or(""))
            .map_err(|e| format!("{e}"))?;
        Ok(TextDto {
            content: d.content,
            encoding: d.encoding.to_string(),
            bom: d.bom,
            lossy: d.lossy,
            eol: d.eol.as_str().to_string(),
        })
    })
    .await
}

/// 探测一个文件的编码，只读头部采样。日志模式用它决定 TextDecoder 的标签。
///
/// 采样 256KB 而不是读全文：日志可能有 1GB，而编码特征在头部就足够明显。
#[tauri::command]
pub fn detect_encoding(path: String) -> Result<String, String> {
    use std::io::Read;
    const SAMPLE: usize = 256 << 10;
    let mut f = std::fs::File::open(&path).map_err(|e| format!("读不到 {path}：{e}"))?;
    let mut buf = vec![0u8; SAMPLE];
    let n = f.read(&mut buf).map_err(|e| format!("{e}"))?;
    buf.truncate(n);
    // 用 detect_label 而不是 decode：样本是按字节截的，末尾多半切在一个
    // 多字节字符中间，而 decode 会把那当成「这不是 UTF-8」去猜别的编码。
    // 一个 26MB 的中文 UTF-8 日志就是这么整份渲染成乱码的
    Ok(fsservice::encoding::detect_label(&buf).to_string())
}

/// 界面上给用户挑的编码清单
#[tauri::command]
pub fn list_encodings() -> Vec<(String, String)> {
    fsservice::encoding::COMMON
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

/// 取文件指纹，用于判断是否被外部改动过。
#[tauri::command]
pub async fn file_stamp(path: String) -> Result<StampDto, String> {
    blocking(move || {
        let s = fsservice::stamp(&path).map_err(|e| format!("{e}"))?;
        Ok(StampDto {
            mtime_ms: s.mtime_ms,
            size: s.size,
        })
    })
    .await
}

/// 在 Finder 里显示。业务在 fsservice —— 这里只转错误。
#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    fsservice::reveal_in_finder(&path).map_err(|e| format!("{e}"))
}

/// 剪贴板里的纯文本（编辑器右键菜单的「粘贴」，理由见 `fsservice::clipboard_text`）。
/// 起子进程 → 阻塞池。
#[tauri::command]
pub async fn read_clipboard() -> Result<String, String> {
    blocking(|| fsservice::clipboard_text().map_err(|e| e.to_string())).await
}

/// 新建文件或目录，返回新路径。业务在 fsservice —— 这里只转错误。
///
/// 参数是「哪个目录、叫什么」而不是一条拼好的路径：**join 和名字校验都在
/// Rust 侧**，前端少一个把文件写到别处去的机会。
#[tauri::command]
pub fn create_entry(dir: String, name: String, is_dir: bool) -> Result<String, String> {
    let p = fsservice::create_entry(&dir, &name, is_dir).map_err(|e| format!("{e}"))?;
    Ok(p.to_string_lossy().into_owned())
}

/// 挪进另一个目录（文件树拖拽），返回新路径。
#[tauri::command]
pub fn move_entry(path: String, dest: String) -> Result<String, String> {
    let p = fsservice::move_entry(&path, &dest).map_err(|e| format!("{e}"))?;
    Ok(p.to_string_lossy().into_owned())
}

/// 原地改名，返回新路径。
#[tauri::command]
pub fn rename_entry(path: String, name: String) -> Result<String, String> {
    let p = fsservice::rename_entry(&path, &name).map_err(|e| format!("{e}"))?;
    Ok(p.to_string_lossy().into_owned())
}

/// 移到废纸篓。**整个应用里没有第二条删除路径** —— 没有 remove_file。
///
/// 走阻塞池：外部卷/网络卷上的 `trashItemAtURL:` 是秒级的。
#[tauri::command]
pub async fn trash_entry(path: String) -> Result<(), String> {
    blocking(move || fsservice::move_to_trash(&path).map_err(|e| format!("{e}"))).await
}

/// 保存。写临时文件 → fsync → rename，进程崩在中间不会留下半个文件。
///
/// **「崩溃」指的是进程崩溃，不含掉电** —— rename 只保证「要么旧的要么新的」，
/// 数据先于目录项落盘是 `fsync` 给的，判据写在 `fsservice::write_bytes` 上。
///
/// 软链会写进它指向的那个文件，硬链接走原地覆写，权限位跟着原文件走 ——
/// 这三条都在 fsservice 里，各有一条会红的测试卡着。
///
/// 按 `label` 指定的编码写回 —— 用什么编码读进来的就用什么存回去，
/// 不做「顺手转成 UTF-8」这种擅自决定。
///
/// 返回写入后的指纹 —— 前端必须拿它更新记录，否则自己的保存
/// 会在下一次检查时被当成"外部修改"。
#[tauri::command]
pub async fn write_text(
    path: String,
    content: String,
    label: Option<String>,
    bom: Option<bool>,
    eol: Option<String>,
) -> Result<StampDto, String> {
    blocking(move || {
        let label = label.unwrap_or_else(|| "UTF-8".into());
        let line_ending = fsservice::eol::Eol::from_label(eol.as_deref().unwrap_or("LF"));
        fsservice::write_text_as(&path, &content, &label, bom.unwrap_or(false), line_ending)
            .map_err(|e| format!("保存失败：{e}"))?;
        let s = fsservice::stamp(&path).map_err(|e| format!("{e}"))?;
        Ok(StampDto {
            mtime_ms: s.mtime_ms,
            size: s.size,
        })
    })
    .await
}

/// 文件系统监听（issue #33 ⑳）：从这一刻起，`root` 底下有东西变了就发一个
/// `fs-changed` 事件，负载是 `"git"`（只有 `.git/` 变了）或 `"files"`。
/// 换项目根前端会再调一次，旧的监听随之停掉；传空串 = 只停不起。
///
/// 事件在监听线程上 emit，Tauri 的 emit 是线程安全的、不阻塞。
/// 防抖和合并在 `fsservice::watch` 里，这儿只负责接线。
#[tauri::command]
pub fn watch_root(app: tauri::AppHandle, state: tauri::State<AppState>, root: String) -> Result<(), String> {
    if root.is_empty() {
        state.set_watch(None);
        return Ok(());
    }
    let w = fsservice::watch::watch(&root, move |c| {
        use tauri::Emitter;
        let kind = match c {
            fsservice::watch::Change::Git => "git",
            fsservice::watch::Change::Files => "files",
        };
        let _ = app.emit("fs-changed", kind);
    })?;
    state.set_watch(Some(w));
    Ok(())
}
