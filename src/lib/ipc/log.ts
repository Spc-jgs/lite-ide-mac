/** 日志引擎的命令包装（issue #32 瘦身）：`openLog` / `closeLog` 在 `commands.ts`（打开文件那条路要），其余只有 LogView 用 */
import { invoke } from "@tauri-apps/api/core";
import type { LogStat, FilterStat, RefreshResult } from "./commands";

export const logStat = (handle: number) => invoke<LogStat>("log_stat", { handle });

/** 取一段行。走二进制 ArrayBuffer，不经 JSON —— 见 ARCHITECTURE.md §3.4 */
export const logLines = (handle: number, start: number, count: number) =>
  invoke<ArrayBuffer>("log_lines", { handle, start, count });

/**
 * 启动过滤；返回 false 表示条件为空、已清除过滤。
 *
 * `label` 是文件编码 —— 关键字要先编成文件那套字节才搜得到，
 * 否则在 GBK 日志里搜中文永远是零命中。
 */
export const logFilter = (
  handle: number,
  levelBits: number,
  pattern: string,
  caseSensitive: boolean,
  collapseStacks: boolean,
  label = "UTF-8",
) =>
  invoke<boolean>("log_filter", {
    handle,
    levelBits,
    pattern,
    caseSensitive,
    collapseStacks,
    label,
  });

export const logFilterStat = (handle: number) =>
  invoke<FilterStat | null>("log_filter_stat", { handle });

export const logLinesFiltered = (handle: number, start: number, count: number) =>
  invoke<ArrayBuffer>("log_lines_filtered", { handle, start, count });

/** 视图行号 → 物理行号，过滤态下显示真实行号用 */
export const logFilterMap = (handle: number, start: number, count: number) =>
  invoke<number[]>("log_filter_map", { handle, start, count });

export const logRefresh = (handle: number) => invoke<RefreshResult>("log_refresh", { handle });

/** 跳到时间：第一条时间 ≥ `query`（`14:32` / `14:32:05` / `2026-08-24 14:32`）的行号；认不出时间戳就 null */
export const logSeekTime = (handle: number, query: string, near: number) =>
  invoke<number | null>("log_seek_time", { handle, query, near });
