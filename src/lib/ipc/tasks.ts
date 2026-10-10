/** 任务（#48）的命令包装：只有任务列表 / 运行工具窗用，它们是懒的（同 ipc/pty.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { TaskList, TaskRun, TaskStale } from "./commands";

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

/** 结束现在占着 `port` 的那个进程（Rust 那边重新查是谁），等端口空出来 */
export const taskFreePort = (port: number) => invoke<void>("task_free_port", { port });

/** 这个项目上次没停干净的任务 */
export const taskStale = (root: string) => invoke<TaskStale[]>("task_stale", { root });

/** 上次没停干净的：`kill` = 结束它们，否则从账上划掉、不管 */
export const taskStaleResolve = (root: string, kill: boolean) => invoke<void>("task_stale_resolve", { root, kill });
