/** 跨文件替换（#42）的命令包装。跟着替换浮层懒加载，不进入口包 */
import { invoke } from "@tauri-apps/api/core";
import type { ReplaceAfter, ReplaceOutcome, ReplacePending, ReplaceScan } from "./commands";
import type { SearchOpts } from "./search";

/** 开着的标签：路径、编辑器里的文本、有没有未保存改动 */
export interface OpenDoc {
  path: string;
  text: string;
  dirty: boolean;
}

export const replaceScan = (root: string, pattern: string, opts: SearchOpts, open: OpenDoc[]) =>
  invoke<ReplaceScan>("replace_scan", { root, pattern, ...opts, open });

/** 替换串变了只重算「改后」，不重新扫盘（Rust 侧留着上一次的扫描） */
export const replacePreview = (replacement: string) => invoke<ReplaceAfter[][]>("replace_preview", { replacement });

/** 失败时 reject 一个 `ReplaceError`（不是字符串）：too-big 要问用户、pending 要先处理上次的中断 */
export const replaceApply = (replacement: string, picks: { rel: string; hits: number[] }[], open: OpenDoc[], allowNoUndo = false) =>
  invoke<ReplaceOutcome>("replace_apply", { replacement, picks, open, allowNoUndo });

export const replaceUndo = (open: OpenDoc[]) => invoke<ReplaceOutcome>("replace_undo", { open });

export const replacePending = () => invoke<ReplacePending | null>("replace_pending");

export const replaceRecover = (rollback: boolean) => invoke<ReplaceOutcome>("replace_recover", { rollback });
