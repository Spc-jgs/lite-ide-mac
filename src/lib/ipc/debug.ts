/** 断点调试（docs/IDEA.md 第 18 节）的命令包装：只有调试那几块用，它们是懒的（同 ipc/tasks.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { DebugHotswap, DebugState, DebugVar, DebugVars } from "./commands";

/** 这次运行的调试会话此刻什么样；不在调试是 null */
export const debugState = (run: number) => invoke<DebugState | null>("debug_state", { run });

/** 这个文件的断点换成 `lines`（整份，从 1 数），`conds` 一一对应（条件断点的条件，空串是普通断点）。正在调试的话当场下到 JVM 里，回「对上了」的那些行 */
/** `conds` / `hits` / `logs` 和 `lines` 一一对应：条件、命中次数、日志断点那句（空串 = 没有） */
export const debugBreakpoints = (root: string, path: string, lines: number[], conds: string[] = [], hits: string[] = [], logs: string[] = []) =>
  invoke<number[]>("debug_breakpoints", { root, path, lines, conds, hits, logs });

/** 异常断点（G14）：未捕获的停 / 被捕获的也停 / 只看这几种（类的全名，空 = 全部） */
export const debugExceptions = (root: string, uncaught: boolean, caught: boolean, types: string[]) => invoke<void>("debug_exceptions", { root, uncaught, caught, types });

/** 运行到光标（⌥F9）：停着时跑到这个文件的这一行（从 1 数）再停 */
export const debugRunTo = (run: number, path: string, line: number) => invoke<void>("debug_run_to", { run, path, line });

export type DebugAction = "continue" | "next" | "stepIn" | "stepOut" | "pause";
export const debugStep = (run: number, action: DebugAction) => invoke<void>("debug_step", { run, action });

/** 换看另一个停着的线程 */
export const debugThread = (run: number, thread: number) => invoke<DebugState>("debug_thread", { run, thread });

/** 一帧的局部变量（`frame`）或者一个变量的下一层（`reference`，数组 / 集合带上 `indexed`） */
export const debugVars = (run: number, a: { frame?: number; reference?: number; indexed?: number }) =>
  invoke<DebugVars>("debug_vars", { run, frame: a.frame ?? null, reference: a.reference ?? null, indexed: a.indexed ?? 0 });

export const debugEval = (run: number, frame: number, expr: string) => invoke<DebugVar>("debug_eval", { run, frame, expr });

/** 「重新加载改过的类」（docs/IDEA.md 第 26 节）：这个窗口里、这个项目正在调试的 JVM，把和它起来时比变了的 class 换进去 */
export const debugHotswap = (root: string) => invoke<DebugHotswap>("debug_hotswap", { root });
