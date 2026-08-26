import { invoke } from "@tauri-apps/api/core";

export interface OpenResult {
  handle: number;
  name: string;
  size: number;
}

export interface LogStat {
  lineCount: number;
  indexedBytes: number;
  totalBytes: number;
  complete: boolean;
  /** 索引结构自身占用 —— 用来验证「内存与文件大小无关」 */
  indexBytes: number;
}

export const openLog = (path: string) => invoke<OpenResult>("open_log", { path });

export const logStat = (handle: number) => invoke<LogStat>("log_stat", { handle });

export const closeLog = (handle: number) => invoke<boolean>("close_log", { handle });

/**
 * 取一段行。走二进制 ArrayBuffer，不经 JSON —— 见 ARCHITECTURE.md §3.4。
 */
export const logLines = (handle: number, start: number, count: number) =>
  invoke<ArrayBuffer>("log_lines", { handle, start, count });

/** 启动参数带的文件（`lite-ide foo.log`），没有则为 null */
export const initialFile = () => invoke<string | null>("initial_file");

export const diag = (msg: string) => invoke<void>("diag", { msg });
