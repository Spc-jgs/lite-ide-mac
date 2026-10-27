//! Java 智能（docs/IDEA.md 第 2 步）：文档同步、跳转、查用法、jar 里的类。业务在 `crate::java`，jdtls 的线和进程在 `lspsvc`。
//! 全部 async + 阻塞池：要等 jdtls 回话（第一次查用法实测最慢 6 秒），文档同步要写管道。

use super::*;
use tauri::Manager;

/// 打开了一个 Java 文件（或者重起之后前端重新登记）：这个项目的 jdtls 没有就起（**不等它起好**），登记这份文档。
/// 不是 Maven / Gradle 项目给 null（不起，也不算错）
#[tauri::command]
pub async fn java_open(window: tauri::Window, app: tauri::AppHandle, root: String, path: String, text: String) -> Result<Option<JavaStatusDto>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        if st.java.get(&root).is_none() && !crate::java::is_build_root(Path::new(&root)) {
            return Ok(None);
        }
        let i = st.java.ensure(&app, &owner, &root).map_err(|s| JavaStatusDto::from(&s).message)?;
        i.open(&path, &text);
        Ok(Some(JavaStatusDto::from(&i.status())))
    })
    .await
}

/// 改了（全文，见 java.rs 头上「文档同步用全文」）。没有 jdtls 就什么都不做 —— 不为一次改动起服务器
#[tauri::command]
pub async fn java_change(app: tauri::AppHandle, root: String, path: String, text: String) -> Result<(), String> {
    blocking(move || {
        if let Some(i) = app.state::<AppState>().java.get(&root) {
            i.change(&path, &text);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn java_save(app: tauri::AppHandle, root: String, path: String) -> Result<(), String> {
    blocking(move || {
        if let Some(i) = app.state::<AppState>().java.get(&root) {
            i.save(&path);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn java_close(app: tauri::AppHandle, root: String, path: String) -> Result<(), String> {
    blocking(move || {
        if let Some(i) = app.state::<AppState>().java.get(&root) {
            i.close(&path);
        }
        Ok(())
    })
    .await
}

fn instance(app: &tauri::AppHandle, root: &str) -> Result<std::sync::Arc<crate::java::Instance>, String> {
    app.state::<AppState>().java.get(root).ok_or_else(|| "此项目的 Java 语言服务器未运行".into())
}

/// 这个 Spring Boot 配置文件（`application*.yml` / `.properties`）能写的键：问 jdtls 要它所在模块的 classpath，读每个 jar 的配置元数据（`spring_meta.rs`）。
/// jdtls 没起就起它（只开了 yml 没开过 Java 文件也要能补全）；第一次要等导入完（最多 3 分钟），之后 jar 按改动时间缓存
#[tauri::command]
pub async fn spring_props(app: tauri::AppHandle, window: tauri::Window, root: String, path: String) -> Result<Vec<SpringPropDto>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        if !crate::spring_meta::is_config(&path) {
            return Ok(Vec::new());
        }
        let java = app.state::<AppState>().java.ensure(&app, &owner, &root).map_err(|s| match s.phase {
            crate::java::Phase::Failed(w) => w,
            _ => "此项目不是 Maven / Gradle 项目，或 Java 语言服务器无法启动".into(),
        })?;
        let cp = java.classpath(&path, std::time::Duration::from_secs(180))?;
        Ok(crate::spring_meta::collect(&cp)
            .into_iter()
            .map(|p| SpringPropDto {
                values: p.values.into_iter().map(|(value, desc)| SpringValueDto { value, desc }).collect(),
                name: p.name,
                ty: p.ty,
                desc: p.desc,
                default: p.default,
                deprecated: p.deprecated,
            })
            .collect())
    })
    .await
}

/// Spring Boot 配置键的值是枚举时（元数据没给 `hints`），那个枚举的常量（`spring_meta::enum_values`）。写值时才问，前端按类型记住
#[tauri::command]
pub async fn spring_enum(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, ty: String) -> Result<Vec<String>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let java = app.state::<AppState>().java.ensure(&app, &owner, &root).map_err(|s| match s.phase {
            crate::java::Phase::Failed(w) => w,
            _ => "此项目不是 Maven / Gradle 项目，或 Java 语言服务器无法启动".into(),
        })?;
        let cp = java.classpath(&path, std::time::Duration::from_secs(180))?;
        Ok(crate::spring_meta::enum_values(&cp, &ty))
    })
    .await
}

/// 跳到定义。位置的行、列从 0 数，列按 UTF-16
#[tauri::command]
pub async fn java_definition(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<Vec<JavaLocationDto>, String> {
    blocking(move || Ok(instance(&app, &root)?.definition(&path, line, character)?.into_iter().map(Into::into).collect())).await
}

/// 查用法（不含声明本身）。超过上限截断，`total` 是真条数
#[tauri::command]
pub async fn java_references(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<JavaRefsDto, String> {
    blocking(move || {
        let (locs, total) = instance(&app, &root)?.references(&path, line, character)?;
        let texts = crate::java::line_texts(&locs);
        let items = locs
            .into_iter()
            .zip(texts)
            .map(|(l, text)| JavaLocationDto { text, ..JavaLocationDto::from(l) })
            .collect();
        Ok(JavaRefsDto { items, total: total as u32 })
    })
    .await
}

/// 跳到实现（⌥⌘B）。和查用法同一个样子（带那一行的文字，列表里给人看）
#[tauri::command]
pub async fn java_implementation(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<JavaRefsDto, String> {
    blocking(move || {
        let (locs, total) = instance(&app, &root)?.implementation(&path, line, character)?;
        Ok(refs_dto(locs, total))
    })
    .await
}

/// 快速文档（F1）：签名 + Javadoc，一段 Markdown；没有文档是空串
#[tauri::command]
pub async fn java_hover(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<String, String> {
    blocking(move || instance(&app, &root)?.hover(&path, line, character)).await
}

/// 跳到父方法（⌘U）
#[tauri::command]
pub async fn java_super(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<JavaRefsDto, String> {
    blocking(move || {
        let locs = instance(&app, &root)?.super_impl(&path, line, character)?;
        let n = locs.len();
        Ok(refs_dto(locs, n))
    })
    .await
}

/// ⌘N 生成第一步（docs/TAKEOVER.md G5）：光标所在的类里能选哪些。`what`：constructor / getter / setter / accessors / equals / tostring / override。带着此刻的全文
#[tauri::command]
pub async fn java_gen_status(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32, what: String) -> Result<JavaGenChoicesDto, String> {
    blocking(move || {
        let c = instance(&app, &root)?.gen_status(&path, &text, line, character, &what)?;
        Ok(JavaGenChoicesDto {
            type_name: c.type_name,
            items: c.items.into_iter().map(|i| JavaGenItemDto { label: i.label, detail: i.detail, checked: i.checked, raw: i.raw }).collect(),
            note: c.note,
        })
    })
    .await
}

/// ⌘N 生成第二步：人勾好了，要改动。只改这个文件的直接交给编辑器（⌘Z 能撤），同快速修复那条路
#[tauri::command]
pub async fn java_gen_apply(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32, what: String, picked: Vec<String>) -> Result<JavaActionResultDto, String> {
    blocking(move || {
        let plan = instance(&app, &root)?.gen_apply(&path, &text, line, character, &what, &picked)?;
        if !plan.renames.is_empty() || plan.files.iter().any(|(p, _)| *p != path) {
            return Err("生成的更改涉及其他文件，当前版本不支持（仅支持修改光标所在的文件）".into());
        }
        Ok(JavaActionResultDto { edits: plan.files.into_iter().flat_map(|(_, e)| e).map(Into::into).collect(), outcome: None })
    })
    .await
}

fn hier_dto(n: crate::java::HierNode) -> JavaHierNodeDto {
    let site = n.site.map(|(line, character)| JavaLocationDto { line, character, end_line: line, end_character: character, ..JavaLocationDto::from(n.at.clone()) });
    JavaHierNodeDto { name: n.name, detail: n.detail, kind: n.kind, at: n.at.into(), site, raw: n.raw }
}

/// 调用层级 / 类型层级的根（docs/TAKEOVER.md G10）：光标底下那个方法 / 类型。`types`：类型层级（⌃H），否则调用层级（⌃⌥H）
#[tauri::command]
pub async fn java_hier_prepare(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32, types: bool) -> Result<Vec<JavaHierNodeDto>, String> {
    blocking(move || Ok(instance(&app, &root)?.hier_prepare(&path, line, character, types)?.into_iter().map(hier_dto).collect())).await
}

/// 一个节点的下一层。`dir`：`in` 谁调了它 / `out` 它调了谁 / `super` 父类型 / `sub` 子类型
#[tauri::command]
pub async fn java_hier_children(app: tauri::AppHandle, root: String, raw: String, dir: String) -> Result<JavaHierChildrenDto, String> {
    blocking(move || {
        let (items, total) = instance(&app, &root)?.hier_children(&raw, &dir)?;
        Ok(JavaHierChildrenDto { items: items.into_iter().map(hier_dto).collect(), total: total as u32 })
    })
    .await
}

/// 内嵌提示（docs/TAKEOVER.md G11）：从 `from` 行开头到 `(end_line, end_char)`（前端问的是看得见的那一截；终点要是文件里真有的位置）
#[tauri::command]
pub async fn java_inlay_hints(app: tauri::AppHandle, root: String, path: String, from: u32, end_line: u32, end_char: u32) -> Result<Vec<JavaInlayDto>, String> {
    blocking(move || {
        let hs = instance(&app, &root)?.inlay_hints(&path, from, end_line, end_char)?;
        Ok(hs.into_iter().map(|h| JavaInlayDto { line: h.line, character: h.character, label: h.label, param: h.param }).collect())
    })
    .await
}

/// 同名高亮（docs/TAKEOVER.md G11）：光标底下那个名字在这个文件里出现的地方
#[tauri::command]
pub async fn java_highlights(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<Vec<JavaHighlightDto>, String> {
    blocking(move || {
        let hs = instance(&app, &root)?.highlights(&path, line, character)?;
        Ok(hs.into_iter().map(|h| JavaHighlightDto { line: h.line, character: h.character, end_line: h.end_line, end_character: h.end_character, write: h.write }).collect())
    })
    .await
}

/// ⇧⇧ 的「类」（docs/TAKEOVER.md G2）。`libs`：连 jar / JDK 里的一起找（IDEA 的「包含非项目项」）。
/// 这个项目的 jdtls 没在跑给 null —— 前端按文件名找，不为一次搜索起一个语言服务器（起好要十几秒，那时人早搜完了）
#[tauri::command]
pub async fn java_types(app: tauri::AppHandle, root: String, query: String, libs: bool) -> Result<Option<JavaTypesDto>, String> {
    blocking(move || {
        let Some(i) = app.state::<AppState>().java.get(&root) else { return Ok(None) };
        let (hits, truncated) = i.types(&query, libs)?;
        let items = hits.into_iter().map(|t| JavaTypeDto { name: t.name, container: t.container, kind: t.kind, at: t.loc.into() }).collect();
        Ok(Some(JavaTypesDto { items, truncated }))
    })
    .await
}

fn refs_dto(locs: Vec<crate::java::Location>, total: usize) -> JavaRefsDto {
    let texts = crate::java::line_texts(&locs);
    let items = locs.into_iter().zip(texts).map(|(l, text)| JavaLocationDto { text, ..JavaLocationDto::from(l) }).collect();
    JavaRefsDto { items, total: total as u32 }
}

/// 补全。带着编辑器此刻的全文（先同步再问，见 `java::Instance::complete`）。位置行列从 0 数、列按 UTF-16
#[tauri::command]
pub async fn java_complete(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32) -> Result<JavaCompletionDto, String> {
    blocking(move || {
        let (items, incomplete, total) = instance(&app, &root)?.complete(&path, &text, line, character)?;
        let items = items
            .into_iter()
            .map(|i| JavaCompletionItemDto {
                label: i.label,
                kind: i.kind,
                detail: i.detail,
                description: i.description,
                edit: i.edit.map(Into::into),
                insert: i.insert,
                snippet: i.snippet,
                sort: i.sort,
                raw: i.raw,
            })
            .collect();
        Ok(JavaCompletionDto { items, incomplete, total: total as u32 })
    })
    .await
}

/// 选中的那一项要的自动 import 和说明文字
#[tauri::command]
pub async fn java_resolve(app: tauri::AppHandle, root: String, raw: String) -> Result<JavaResolvedDto, String> {
    blocking(move || {
        let r = instance(&app, &root)?.resolve(&raw)?;
        Ok(JavaResolvedDto { edits: r.edits.into_iter().map(Into::into).collect(), doc: r.doc })
    })
    .await
}

/// 格式化（⌥⌘L）：返回要做的改动（行列从 0 数、列按 UTF-16），前端在编辑器里一笔做完。`range` 为空 = 整份
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn java_format(
    app: tauri::AppHandle,
    root: String,
    path: String,
    text: String,
    range: Option<JavaRangeArg>,
    tab_size: u32,
    insert_spaces: bool,
) -> Result<Vec<JavaEditDto>, String> {
    blocking(move || {
        let range = range.map(|r| crate::java::Edit { line: r.line, character: r.character, end_line: r.end_line, end_character: r.end_character, text: String::new() });
        Ok(instance(&app, &root)?.format(&path, &text, range, tab_size, insert_spaces)?.into_iter().map(Into::into).collect())
    })
    .await
}

/// 优化 import（⌃⌥O，docs/TAKEOVER.md G22）：这个文件要改的那几处（前端一笔做完）。jdtls 没起 / 不是 Java 项目 → 报错
#[tauri::command]
pub async fn java_organize_imports(app: tauri::AppHandle, root: String, path: String, text: String) -> Result<Vec<JavaEditDto>, String> {
    blocking(move || Ok(instance(&app, &root)?.organize_imports(&path, &text)?.into_iter().map(Into::into).collect())).await
}

/// 光标那儿（或选中的那几行）能做的修复（⌥↵）
#[tauri::command]
pub async fn java_code_actions(app: tauri::AppHandle, root: String, path: String, text: String, range: JavaRangeArg, only: Option<String>) -> Result<Vec<JavaActionDto>, String> {
    blocking(move || {
        let r = crate::java::Edit { line: range.line, character: range.character, end_line: range.end_line, end_character: range.end_character, text: String::new() };
        Ok(instance(&app, &root)?.code_actions(&path, &text, r, only.as_deref())?.into_iter().map(|a| JavaActionDto { title: a.title, kind: a.kind, raw: a.raw }).collect())
    })
    .await
}

/// 做选中的那条修复。只改 `path` 这一个文件的：把改动交回编辑器（不写盘，⌘Z 能撤）；改到别的文件、或者要改文件名的：照改名那套落盘（`java::apply_rename`）
#[tauri::command]
pub async fn java_apply_action(app: tauri::AppHandle, root: String, path: String, raw: String, open: Vec<super::replace::OpenDocArg>) -> Result<JavaActionResultDto, String> {
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let plan = instance(&app, &root)?.resolve_action(&raw)?;
        if plan.renames.is_empty() && plan.files.iter().all(|(p, _)| *p == path) {
            let edits = plan.files.into_iter().flat_map(|(_, e)| e).map(Into::into).collect();
            return Ok(JavaActionResultDto { edits, outcome: None });
        }
        let out = crate::java::apply_rename(&plan, Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(JavaActionResultDto { edits: Vec::new(), outcome: Some(super::replace::outcome_dto(out)) })
    })
    .await
}

/// 一段范围（前端传）：行列从 0 数、列按 UTF-16
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRangeArg {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// 光标底下能不能改名（⇧F6 先问这个：不能改的地方不弹输入框）。null = 这里不能改名
#[tauri::command]
pub async fn java_prepare_rename(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32) -> Result<Option<JavaRenameTargetDto>, String> {
    blocking(move || {
        Ok(instance(&app, &root)?.prepare_rename(&path, &text, line, character)?.map(|t| JavaRenameTargetDto {
            line: t.range.line,
            character: t.range.character,
            end_line: t.range.end_line,
            end_character: t.range.end_character,
            placeholder: t.placeholder,
        }))
    })
    .await
}

/// 改名：问 jdtls 要改哪些，照跨文件替换那套落盘（`java::apply_rename`）。
/// `open`：开着的标签（同 `replace_apply`）。结果的形状同替换，前端走同一条路让开着的标签跟上
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn java_rename(
    app: tauri::AppHandle,
    root: String,
    path: String,
    text: String,
    line: u32,
    character: u32,
    new_name: String,
    open: Vec<super::replace::OpenDocArg>,
) -> Result<ReplaceOutcomeDto, String> {
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let i = instance(&app, &root)?;
        let plan = i.rename(&path, &text, line, character, &new_name)?;
        // 盘上改了哪些不用在这儿告诉 jdtls：它的文件监听会看到（`java::Instance::on_disk_changes`），撤销时也一样
        let out = crate::java::apply_rename(&plan, Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(super::replace::outcome_dto(out))
    })
    .await
}

/// 移动类（F6）能去哪几个包
#[tauri::command]
pub async fn java_move_destinations(app: tauri::AppHandle, root: String, path: String) -> Result<Vec<JavaMoveDestDto>, String> {
    blocking(move || {
        Ok(instance(&app, &root)?
            .move_destinations(&path)?
            .into_iter()
            .map(|d| JavaMoveDestDto { name: d.name, uri: d.uri, project: d.project, here: d.here })
            .collect())
    })
    .await
}

/// 移动类的预览：问 jdtls 要改哪些（不改），交给前端列出来
#[tauri::command]
pub async fn java_move_preview(app: tauri::AppHandle, root: String, path: String, text: String, dest: String) -> Result<JavaMovePreviewDto, String> {
    blocking(move || {
        let plan = instance(&app, &root)?.move_plan(&path, &text, &dest)?;
        Ok(move_preview(&plan, Path::new(&root)))
    })
    .await
}

/// 移动类：再问一次 jdtls（预览和现在之间可能有人改了文件），和预览时列的对不上就不做；对得上照改名那套落盘（`java::apply_rename`：两段提交、能撤销、开着的标签跟上）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn java_move(
    app: tauri::AppHandle,
    root: String,
    path: String,
    text: String,
    dest: String,
    expect: Vec<String>,
    open: Vec<super::replace::OpenDocArg>,
) -> Result<ReplaceOutcomeDto, String> {
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let plan = instance(&app, &root)?.move_plan(&path, &text, &dest)?;
        if move_preview(&plan, Path::new(&root)).files != expect {
            return Err("预览后有文件发生变化，需要修改的内容与预览不一致，请重新预览".into());
        }
        let out = crate::java::apply_rename(&plan, Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(super::replace::outcome_dto(out))
    })
    .await
}

/// 改签名（⌘F6）：光标那个方法现在的签名
#[tauri::command]
pub async fn java_method_sig(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32) -> Result<JavaMethodSigDto, String> {
    blocking(move || Ok(JavaMethodSigDto::of(instance(&app, &root)?.signature(&path, &text, line, character)?))).await
}

/// 改签名的预览：改成 `sig` 要动哪些文件（不改）。`tab_size` / `spaces`：编辑器的缩进（新写出来的参数照它排）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn java_method_sig_preview(
    app: tauri::AppHandle,
    root: String,
    path: String,
    text: String,
    line: u32,
    character: u32,
    sig: JavaMethodSigDto,
    tab_size: u32,
    spaces: bool,
) -> Result<JavaMovePreviewDto, String> {
    blocking(move || {
        let plan = instance(&app, &root)?.signature_plan(&path, &text, line, character, &sig.into_sig(), (tab_size, spaces))?;
        Ok(move_preview(&plan, Path::new(&root)))
    })
    .await
}

/// 改签名：再问一次 jdtls，和预览时列的文件对不上就不做；对得上照改名那套落盘（同 `java_move`）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn java_change_signature(
    app: tauri::AppHandle,
    root: String,
    path: String,
    text: String,
    line: u32,
    character: u32,
    sig: JavaMethodSigDto,
    tab_size: u32,
    spaces: bool,
    expect: Vec<String>,
    open: Vec<super::replace::OpenDocArg>,
) -> Result<ReplaceOutcomeDto, String> {
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let plan = instance(&app, &root)?.signature_plan(&path, &text, line, character, &sig.into_sig(), (tab_size, spaces))?;
        if move_preview(&plan, Path::new(&root)).files != expect {
            return Err("预览后有文件发生变化，需要修改的内容与预览不一致，请重新预览".into());
        }
        let out = crate::java::apply_rename(&plan, Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(super::replace::outcome_dto(out))
    })
    .await
}

fn move_preview(plan: &crate::java::RenamePlan, root: &Path) -> JavaMovePreviewDto {
    let rel = |p: &str| Path::new(p).strip_prefix(root).map_or_else(|_| p.to_string(), |r| r.to_string_lossy().into_owned());
    let mut files: Vec<String> = plan.files.iter().map(|(p, _)| rel(p)).collect();
    files.sort();
    let (from, to) = plan.renames.first().map(|(f, t)| (rel(f), rel(t))).unwrap_or_default();
    JavaMovePreviewDto { files, from, to }
}

/// jar 里一个类的内容（`jdt://…`）：有源码 jar 就是源码，没有就是反编译的
#[tauri::command]
pub async fn java_class_contents(app: tauri::AppHandle, root: String, uri: String) -> Result<String, String> {
    blocking(move || instance(&app, &root)?.class_contents(&uri)).await
}

/// 现在什么状态（前端刚挂上、错过了事件时补一次）。没起过就是 null
#[tauri::command]
pub async fn java_status(app: tauri::AppHandle, root: String) -> Result<Option<JavaStatusDto>, String> {
    blocking(move || Ok(app.state::<AppState>().java.status(&root).as_ref().map(JavaStatusDto::from))).await
}

/// 重起这个项目的 jdtls（状态栏的按钮）。开着的 Java 文档由前端接着重新 `java_open`
#[tauri::command]
pub async fn java_restart(window: tauri::Window, app: tauri::AppHandle, root: String) -> Result<(), String> {
    let owner = window.label().to_string();
    blocking(move || app.state::<AppState>().java.restart(&app, &owner, &root)).await
}

/// 参数提示：光标此刻在哪个调用的第几个参数上（带着编辑器此刻的全文，同补全）
#[tauri::command]
pub async fn java_signature(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32) -> Result<JavaSignaturesDto, String> {
    blocking(move || {
        let s = instance(&app, &root)?.signature_help(&path, &text, line, character)?;
        Ok(JavaSignaturesDto {
            signatures: s
                .list
                .into_iter()
                .map(|x| JavaSignatureDto { label: x.label, params: x.params.into_iter().map(|(start, end)| JavaParamDto { start, end }).collect(), doc: x.doc })
                .collect(),
            active: s.active,
            param: s.param,
            total: s.total,
        })
    })
    .await
}


/// MyBatis（docs/IDEA.md 第 24 节）：打开的 Java 接口 → `here` 是它的方法（行号），`there` 是 namespace 对得上的 XML 里的语句（绝对路径 + 行号）。
/// `text` 是编辑器里此刻的（没保存的也算）。不靠 jdtls：读文本、在项目里找。不是接口、没有对得上的 XML 都是空的（不算错）
#[tauri::command]
pub async fn mybatis_for_java(root: String, text: String) -> Result<MybatisLinksDto, String> {
    blocking(move || {
        let Some(fqn) = crate::mybatis::interface_name(&text) else { return Ok(MybatisLinksDto::default()) };
        let here = crate::mybatis::methods(&text).into_iter().map(|(name, line)| MybatisSpotDto { name, kind: String::new(), path: String::new(), line }).collect();
        let mut there = Vec::new();
        for rel in crate::mybatis::find_xml(Path::new(&root), &fqn, &super::search::skip_for(&root)) {
            let abs = Path::new(&root).join(&rel);
            let Some(xml) = read_small(&abs) else { continue };
            if crate::mybatis::namespace(&xml).as_deref() != Some(fqn.as_str()) {
                continue;
            }
            let path = abs.display().to_string();
            there.extend(crate::mybatis::statements(&xml).into_iter().map(|s| MybatisSpotDto { name: s.id, kind: s.kind, path: path.clone(), line: s.line }));
        }
        Ok(MybatisLinksDto { here, there })
    })
    .await
}

/// Spring 的注入点 → Bean（`spring_beans.rs`）：编辑器里此刻的文本（没保存的也算）里每个注入点，和项目里配得上的 Bean（绝对路径 + 行号）。
/// 一个都配不上的注入点不回 —— 前端只给配上了的画图标
#[tauri::command]
pub async fn spring_beans(root: String, text: String) -> Result<Vec<SpringInjectDto>, String> {
    blocking(move || {
        let injects = crate::spring_beans::injects(&text);
        if injects.is_empty() {
            return Ok(Vec::new());
        }
        let files = searchsvc::list_files(&root, &super::search::skip_for(&root)).unwrap_or_default();
        let all = crate::spring_beans::project_beans(Path::new(&root), &files);
        Ok(injects
            .into_iter()
            .filter_map(|i| {
                let beans: Vec<SpringBeanDto> = all
                    .iter()
                    .flat_map(|(p, bs)| bs.iter().filter(|b| crate::spring_beans::matches(b, &i.ty)).map(move |b| SpringBeanDto { label: b.label.clone(), path: p.display().to_string(), line: b.line }))
                    .collect();
                (!beans.is_empty()).then_some(SpringInjectDto { line: i.line, ty: i.ty, beans })
            })
            .collect())
    })
    .await
}

/// MyBatis：打开的 Mapper XML → `here` 是它的语句（行号），`there` 是 namespace 那个接口的方法（绝对路径 + 行号）。不是 Mapper XML、找不到接口都是空的
#[tauri::command]
pub async fn mybatis_for_xml(root: String, text: String) -> Result<MybatisLinksDto, String> {
    blocking(move || {
        let Some(fqn) = crate::mybatis::namespace(&text) else { return Ok(MybatisLinksDto::default()) };
        let here = crate::mybatis::statements(&text).into_iter().map(|s| MybatisSpotDto { name: s.id, kind: s.kind, path: String::new(), line: s.line }).collect();
        let files = searchsvc::list_files(&root, &super::search::skip_for(&root)).unwrap_or_default();
        let mut there = Vec::new();
        if let Some(rel) = crate::mybatis::find_java(&files, &fqn) {
            let abs = Path::new(&root).join(&rel);
            if let Some(java) = read_small(&abs) {
                let path = abs.display().to_string();
                there = crate::mybatis::methods(&java).into_iter().map(|(name, line)| MybatisSpotDto { name, kind: String::new(), path: path.clone(), line }).collect();
            }
        }
        Ok(MybatisLinksDto { here, there })
    })
    .await
}

/// 读一份源码（Mapper XML / 接口），最多 2 MB —— 再大的不是手写的 Mapper（Lithe 同一个数），超了不读
fn read_small(p: &Path) -> Option<String> {
    let meta = std::fs::metadata(p).ok()?;
    if meta.len() > 2 << 20 {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// Spring 的接口（docs/IDEA.md 第 25 节，随处搜索里的「接口」）：先用项目搜索找出带 `@RestController` / `@Controller` 的文件，只解析这些。
/// 不靠 jdtls。按路径排
#[tauri::command]
pub async fn spring_endpoints(root: String) -> Result<Vec<SpringEndpointDto>, String> {
    blocking(move || {
        let q = searchsvc::Query { pattern: r"@(Rest)?Controller\b".into(), case: true, word: false, regex: true };
        let mut files: Vec<String> = searchsvc::grep(&root, &q, 5000, &super::search::skip_for(&root)).unwrap_or_default().into_iter().map(|h| h.path).filter(|p| p.ends_with(".java")).collect();
        files.sort();
        files.dedup();
        let mut out = Vec::new();
        for rel in files {
            let abs = Path::new(&root).join(&rel);
            let Some(text) = read_small(&abs) else { continue };
            let file = abs.display().to_string();
            out.extend(crate::spring::endpoints(&text).into_iter().map(|e| SpringEndpointDto { methods: e.methods, path: e.path, controller: e.controller, method: e.method, file: file.clone(), line: e.line }));
        }
        out.sort_by(|a, b| a.path.cmp(&b.path).then(a.methods.cmp(&b.methods)));
        Ok(out)
    })
    .await
}

/// Spring 的配置键在哪（`@Value("${demo.greeting}")` 上 ⌘B，docs/IDEA.md 第 25 节）：项目里的 `application*` / `bootstrap*` 配置文件，
/// 按宽松绑定比（`greetingText` = `greeting-text`）。每个 profile 的文件都列；不带 profile 的排前面
#[tauri::command]
pub async fn spring_config(root: String, key: String) -> Result<Vec<SpringConfigHitDto>, String> {
    blocking(move || {
        let files = searchsvc::list_files(&root, &super::search::skip_for(&root)).unwrap_or_default();
        let mut out = Vec::new();
        for rel in files {
            let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
            if !crate::spring::is_config_file(&name) {
                continue;
            }
            let abs = Path::new(&root).join(&rel);
            let Some(text) = read_small(&abs) else { continue };
            for line in crate::spring::find_key(&name, &text, &key) {
                let at = text.lines().nth(line as usize - 1).unwrap_or("").trim().chars().take(200).collect();
                out.push(SpringConfigHitDto { path: abs.display().to_string(), name: name.clone(), line, text: at });
            }
        }
        // application.yml 在 application-dev.yml 前面：不带 profile 的是底子
        out.sort_by_key(|h| (h.name.contains('-'), h.path.clone(), h.line));
        Ok(out)
    })
    .await
}
