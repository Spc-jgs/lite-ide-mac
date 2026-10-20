/** 本地历史（docs/IDEA.md 第 21 节）的命令包装：只有本地历史那个面板用，它是懒的 */
import { invoke } from "@tauri-apps/api/core";
import type { HistoryEntry } from "./commands";

/** 这个文件存过的版本，最新的在前 */
export const historyList = (path: string) => invoke<HistoryEntry[]>("history_list", { path });

/** 一版的文字（按这个文件现在的编码解码） */
export const historyRead = (path: string, hash: string, label?: string) => invoke<string>("history_read", { path, hash, label: label ?? null });
