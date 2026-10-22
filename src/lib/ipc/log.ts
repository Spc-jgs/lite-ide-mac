/** 日志引擎的命令包装（issue #32 瘦身）：`openLog` / `closeLog` 在 `commands.ts`（打开文件那条路要），其余只有 LogView 用 */
import { invoke } from "@tauri-apps/api/core";
import type { LogStat, FilterStat, RefreshResult, MergeInfo, MergeRows, LogPatterns } from "./commands";

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
  /** 合并视图：只看哪几份（第 i 位 = 第 i 份）；单文件不传 */
  sources?: number,
) =>
  invoke<boolean>("log_filter", {
    handle,
    levelBits,
    pattern,
    caseSensitive,
    collapseStacks,
    label,
    sources,
  });

export const logFilterStat = (handle: number) =>
  invoke<FilterStat | null>("log_filter_stat", { handle });

export const logLinesFiltered = (handle: number, start: number, count: number) =>
  invoke<ArrayBuffer>("log_lines_filtered", { handle, start, count });

/**
 * 一整行（#64 行详情）：列表里超过 64KB 的行截过，点开时另外取。`line` 是文件里的行号（0 起）；合并视图另给 `src`（第几份）。
 * 回的是二进制：8 字节真长度 + 最多 1MB 原文，用 `logview/line-detail.ts` 的 `decodeLineFull` 解
 */
export const logLineFull = (handle: number, src: number | null, line: number) =>
  invoke<ArrayBuffer>("log_line_full", { handle, src, line });

/** 视图行号 → 物理行号，过滤态下显示真实行号用 */
export const logFilterMap = (handle: number, start: number, count: number) =>
  invoke<number[]>("log_filter_map", { handle, start, count });

export const logRefresh = (handle: number) => invoke<RefreshResult>("log_refresh", { handle });

/** 跳到时间：第一条时间 ≥ `query`（`14:32` / `14:32:05` / `2026-08-24 14:32`）的行号；认不出时间戳就 null */
export const logSeekTime = (handle: number, query: string, near: number) =>
  invoke<number | null>("log_seek_time", { handle, query, near });

/** 几份日志按时间合成一条（docs/MERGE.md）。句柄和 `openLog` 的同一种，上面这些命令都认 */
export const openMerged = (paths: string[]) => invoke<MergeInfo>("open_merged", { paths });

/** 一段行各自来自哪份、是那份的第几行。`filtered` 时 start / count 数的是第几条命中 */
export const mergeRows = (handle: number, start: number, count: number, filtered: boolean) =>
  invoke<MergeRows>("merge_rows", { handle, start, count, filtered });

/** 改时钟偏移（毫秒，每份一个），整份重新归并 —— 行号全变，调用方要丢缓存、重跑过滤 */
export const mergeSetOffsets = (handle: number, offsets: number[]) =>
  invoke<MergeInfo>("merge_set_offsets", { handle, offsets });

/** 原生的「选择文件」面板，可多选；取消是空数组 */
export const pickFiles = (title: string, dir?: string | null) => invoke<string[]>("pick_files", { title, dir: dir ?? null });

/** 日志归类：第一次问时在后台起，之后返回当前快照（边归边看）；`restart` 是重新归类 */
export const logPatterns = (handle: number, limit: number, restart = false) =>
  invoke<LogPatterns>("log_patterns", { handle, limit, restart });

/** 日志归类的对比：这个句柄和 `base` 那几份一起归、每类两边各几次（docs/PATTERNS.md）；基准换了 Rust 那边自己重来 */
export const logPatternsCompare = (handle: number, base: string[], limit: number, restart = false) =>
  invoke<LogPatterns>("log_patterns_compare", { handle, base, limit, restart });
