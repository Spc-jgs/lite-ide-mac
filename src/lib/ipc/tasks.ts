/** 任务（#48）的命令包装：只有任务列表 / 运行工具窗用，它们是懒的（同 ipc/pty.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { TaskList } from "./commands";

/** 这个项目的全部任务：`.lite-ide/tasks.json` 里的 + 自动认出的 package.json scripts */
export const taskList = (root: string) => invoke<TaskList>("task_list", { root });

/** 新建 `.lite-ide/tasks.json`（带注释掉的例子），已经有了就不动；返回路径 */
export const taskNewFile = (root: string) => invoke<string>("task_new_file", { root });
