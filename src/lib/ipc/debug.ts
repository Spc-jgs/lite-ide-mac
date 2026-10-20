/** 断点调试（docs/IDEA.md 第 18 节）的命令包装：只有调试那几块用，它们是懒的（同 ipc/tasks.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { DebugState, DebugVar, DebugVars } from "./commands";

/** 这次运行的调试会话此刻什么样；不在调试是 null */
export const debugState = (run: number) => invoke<DebugState | null>("debug_state", { run });

/** 这个文件的断点换成 `lines`（整份，从 1 数）。正在调试的话当场下到 JVM 里，回「对上了」的那些行 */
export const debugBreakpoints = (root: string, path: string, lines: number[]) => invoke<number[]>("debug_breakpoints", { root, path, lines });

export type DebugAction = "continue" | "next" | "stepIn" | "stepOut" | "pause";
export const debugStep = (run: number, action: DebugAction) => invoke<void>("debug_step", { run, action });

/** 换看另一个停着的线程 */
export const debugThread = (run: number, thread: number) => invoke<DebugState>("debug_thread", { run, thread });

/** 一帧的局部变量（`frame`）或者一个变量的下一层（`reference`，数组 / 集合带上 `indexed`） */
export const debugVars = (run: number, a: { frame?: number; reference?: number; indexed?: number }) =>
  invoke<DebugVars>("debug_vars", { run, frame: a.frame ?? null, reference: a.reference ?? null, indexed: a.indexed ?? 0 });

export const debugEval = (run: number, frame: number, expr: string) => invoke<DebugVar>("debug_eval", { run, frame, expr });
