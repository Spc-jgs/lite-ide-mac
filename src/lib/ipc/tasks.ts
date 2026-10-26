/** 任务（#48）的命令包装：只有任务列表 / 运行工具窗用，它们是懒的（同 ipc/pty.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { BuildProblems, Coverage, MavenProject, MavenResolved, Service, StuckProc, TaskHeap, TaskList, TaskRun, TaskStale, TestResults } from "./commands";

/** 这个项目的全部任务：`.lite-ide/tasks.json` 里的 + 自动认出的 package.json scripts */
export const taskList = (root: string) => invoke<TaskList>("task_list", { root });

/** 新建 `.lite-ide/tasks.json`（带注释掉的例子），已经有了就不动；返回路径 */
export const taskNewFile = (root: string) => invoke<string>("task_new_file", { root });

/** 跑一个任务（按名字，Rust 那边现找定义）。同名的还在跑会先停掉、等端口空了再起。`debug`：带 JDWP 起、挂上调试器（只有 Spring Boot 应用，docs/IDEA.md 第 18 节） */
export const taskRun = (root: string, name: string, debug = false) => invoke<TaskRun>("task_run", { root, name, debug });

/** 停：第一次软停（SIGINT 整组，5 秒后强杀），软停没完再叫一次立刻强杀。"stopping" / "killed" / "gone" */
export const taskStop = (id: number) => invoke<"stopping" | "killed" | "gone">("task_stop", { id });
/** 停不掉的那些（#68）：宽限期过了还在「正在停」时问 —— 组里活着、但不归我们管的进程 */
export const taskStuck = (id: number) => invoke<StuckProc[]>("task_stuck", { id });
/** 组合配置的那几份；不是组合配置回 null（docs/TAKEOVER.md R1）。起之前问一句，只读应用自己的那份文件 */
export const runCompoundItems = (root: string, name: string) => invoke<string[] | null>("run_compound_items", { root, name });

/** 关掉标签：从表里摘掉，还在跑就软停 */
export const taskClose = (id: number) => invoke<void>("task_close", { id });

/** 结束现在占着 `port` 的那个进程（Rust 那边重新查是谁），等端口空出来 */
export const taskFreePort = (port: number) => invoke<void>("task_free_port", { port });

/** 这个项目上次没停干净的任务 */
export const taskStale = (root: string) => invoke<TaskStale[]>("task_stale", { root });

/** 上次没停干净的：`kill` = 结束它们，否则从账上划掉、不管 */
export const taskStaleResolve = (root: string, kill: boolean) => invoke<void>("task_stale_resolve", { root, kill });

/** 跑单测那次（「模块 [test 选择器]」）的结果：`sinceMs` 之后写的 surefire 报告 */
export const testResults = (root: string, name: string, sinceMs: number) => invoke<TestResults>("test_results", { root, name, sinceMs });

/** 带覆盖率跑的那次（「模块 [cover …]」）跑完：出报告（G15） */
export const coverageReport = (root: string, name: string) => invoke<Coverage>("coverage_report", { root, name });

/** 构建（⌘F9）那次的编译错误：从这次运行的输出文件里认 */
export const buildProblems = (log: string) => invoke<BuildProblems>("build_problems", { log });

/** Services 工具窗：认出来的 Spring Boot 服务，在跑的带上它在听的端口 */
export const servicesList = (root: string) => invoke<Service[]>("services_list", { root });

/** 一次运行此刻在听的端口：Services 里「跑着、还没端口」时隔一会儿问一次（只是一次 lsof，不重新认服务） */
export const taskPorts = (id: number) => invoke<number[]>("task_ports", { id });
/** 这几次运行的 JVM 堆（docs/IDEA.md 第 27 节）：读 JVM 的性能计数，不是 JVM 的、还在编译的不回 */
export const taskHeap = (ids: number[]) => invoke<TaskHeap[]>("task_heap", { ids });

/** Maven 工具窗：读 pom 就有的（模块树、阶段、profile、工具栏开关） */
export const mavenProject = (root: string) => invoke<MavenProject>("maven_project", { root });
/** 插件和依赖：每个 reactor 跑一次 mvn，会等到跑完 */
export const mavenResolve = (root: string) => invoke<MavenResolved>("maven_resolve", { root });
/** 工具栏的开关、勾的 profile（按项目记，跑目标和起服务都带上） */
export const mavenPrefsSet = (root: string, offline: boolean, skipTests: boolean, profiles: string[]) =>
  invoke<void>("maven_prefs_set", { root, offline, skipTests, profiles });
