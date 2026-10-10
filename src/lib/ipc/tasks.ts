/** 任务（#48）的命令包装：只有任务列表 / 运行工具窗用，它们是懒的（同 ipc/pty.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { TaskList, TaskRun } from "./commands";

/** 这个项目的全部任务：`.lite-ide/tasks.json` 里的 + 自动认出的 package.json scripts */
export const taskList = (root: string) => invoke<TaskList>("task_list", { root });

/** 新建 `.lite-ide/tasks.json`（带注释掉的例子），已经有了就不动；返回路径 */
export const taskNewFile = (root: string) => invoke<string>("task_new_file", { root });

/** 跑一个任务（按名字，Rust 那边现找定义）。同名的还在跑会先停掉、等端口空了再起 */
export const taskRun = (root: string, name: string) => invoke<TaskRun>("task_run", { root, name });

/** 停：第一次软停（SIGINT 整组，5 秒后强杀），软停没完再叫一次立刻强杀。"stopping" / "killed" / "gone" */
export const taskStop = (id: number) => invoke<"stopping" | "killed" | "gone">("task_stop", { id });

/** 关掉标签：从表里摘掉，还在跑就软停 */
export const taskClose = (id: number) => invoke<void>("task_close", { id });
