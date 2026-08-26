import { invoke, type Channel } from "@tauri-apps/api/core";

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

export interface PathInfo {
  kind: "file" | "dir";
  mode: "edit" | "log";
  path: string;
  name: string;
  size: number;
  /** 判为 log 模式的原因，用于说明「为什么这个文件是只读的」 */
  reason: string;
}

export interface DirEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
}

/** 探测路径：目录还是文件，文件该用哪种模式打开 */
export const probePath = (path: string) => invoke<PathInfo>("probe_path", { path });

export const listDir = (path: string, showHidden = false) =>
  invoke<DirEntry[]>("list_dir", { path, showHidden });

export const readText = (path: string) => invoke<string>("read_text", { path });

export const writeText = (path: string, content: string) =>
  invoke<void>("write_text", { path, content });

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

/** 启动参数带的路径（`lite-ide foo.log` 或 `lite-ide ~/proj`），没有则为 null */
export const initialPath = () => invoke<string | null>("initial_path");

export const diag = (msg: string) => invoke<void>("diag", { msg });

// ─────────────────────────── 终端 ───────────────────────────

/** 起一个终端；输出通过 Channel 流式回传 */
export const ptySpawn = (
  cwd: string,
  cols: number,
  rows: number,
  onData: Channel<number[] | ArrayBuffer>,
) => invoke<number>("pty_spawn", { cwd, cols, rows, onData });

export const ptyWrite = (id: number, data: string) => invoke<void>("pty_write", { id, data });

export const ptyResize = (id: number, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });

export const ptyKill = (id: number) => invoke<boolean>("pty_kill", { id });

export const ptyAlive = (id: number) => invoke<boolean>("pty_alive", { id });

// ─────────────────────────── 搜索 ───────────────────────────

export interface Hit {
  path: string;
  line: number;
  text: string;
}

/** 列出项目文件（相对路径），模糊匹配在前端做 */
export const listProjectFiles = (root: string) =>
  invoke<string[]>("list_project_files", { root });

export const grepProject = (root: string, pattern: string, limit = 200) =>
  invoke<Hit[]>("grep_project", { root, pattern, limit });

export const ripgrepAvailable = () => invoke<boolean>("ripgrep_available");
