//! 任务（#48）：列出来、新建 tasks.json。起和停在第 3 步。业务在 `taskdefs`（定义从哪来）和 `tasksvc`（进程）。

use super::*;

/// 列出这个项目的全部任务。读盘（tasks.json、各层 package.json），走阻塞池
#[tauri::command]
pub async fn task_list(root: String) -> Result<TaskListDto, String> {
    blocking(move || {
        let f = crate::taskdefs::discover(Path::new(&root));
        Ok(TaskListDto {
            tasks: f
                .defs
                .into_iter()
                .map(|d| TaskDefDto {
                    name: d.name,
                    command: d.command,
                    cwd: d.cwd,
                    source: match d.source {
                        crate::taskdefs::Source::File => "file",
                        crate::taskdefs::Source::Package { .. } => "package",
                    }
                    .into(),
                })
                .collect(),
            file: f.file,
            problems: f.problems,
        })
    })
    .await
}

/// 新建 `.lite-ide/tasks.json`（带注释掉的例子），已经有了就不动。返回路径，前端拿去开成标签
#[tauri::command]
pub async fn task_new_file(root: String) -> Result<String, String> {
    blocking(move || {
        crate::taskdefs::create_file(Path::new(&root))
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| format!("建不了 {}：{e}", crate::taskdefs::FILE))
    })
    .await
}
