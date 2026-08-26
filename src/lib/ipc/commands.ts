import { invoke } from "@tauri-apps/api/core";

export interface OpenResult {
  handle: number;
  name: string;
  size: number;
}

/** 顺序同 Rust 侧 Level：error / warn / info / debug / trace / other */
export type LevelCounts = [number, number, number, number, number, number];

export interface LogStat {
  lineCount: number;
  indexedBytes: number;
  totalBytes: number;
  complete: boolean;
  /** 索引结构自身占用 —— 用来验证「内存与文件大小无关」 */
  indexBytes: number;
  levels: LevelCounts;
  levelsComplete: boolean;
  levelsScanned: number;
}

export interface FilterStat {
  hits: number;
  complete: boolean;
  scannedLines: number;
}

export interface RefreshResult {
  kind: "none" | "grew" | "rotated";
  newLines: number;
  lineCount: number;
}

export const openLog = (path: string) => invoke<OpenResult>("open_log", { path });
export const logStat = (handle: number) => invoke<LogStat>("log_stat", { handle });
export const closeLog = (handle: number) => invoke<boolean>("close_log", { handle });

/** 取一段行。走二进制 ArrayBuffer，不经 JSON —— 见 ARCHITECTURE.md §3.4 */
export const logLines = (handle: number, start: number, count: number) =>
  invoke<ArrayBuffer>("log_lines", { handle, start, count });

/** 启动过滤；返回 false 表示条件为空、已清除过滤 */
export const logFilter = (
  handle: number,
  levelBits: number,
  pattern: string,
  caseSensitive: boolean,
) => invoke<boolean>("log_filter", { handle, levelBits, pattern, caseSensitive });

export const logFilterStat = (handle: number) =>
  invoke<FilterStat | null>("log_filter_stat", { handle });

export const logLinesFiltered = (handle: number, start: number, count: number) =>
  invoke<ArrayBuffer>("log_lines_filtered", { handle, start, count });

/** 视图行号 → 物理行号，过滤态下显示真实行号用 */
export const logFilterMap = (handle: number, start: number, count: number) =>
  invoke<number[]>("log_filter_map", { handle, start, count });

export const logRefresh = (handle: number) => invoke<RefreshResult>("log_refresh", { handle });

/** 启动参数带的文件（`lite-ide foo.log`），没有则为 null */
export const initialFile = () => invoke<string | null>("initial_file");

export const diag = (msg: string) => invoke<void>("diag", { msg });
