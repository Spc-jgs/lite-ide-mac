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

export interface Stamp {
  mtimeMs: number;
  size: number;
}

/** 文件指纹，用于判断是否被外部改动过 */
export const fileStamp = (path: string) => invoke<Stamp>("file_stamp", { path });

/** 保存并返回新指纹 —— 必须拿它更新记录，否则自己的保存会被当成外部修改 */
export const writeText = (path: string, content: string) =>
  invoke<Stamp>("write_text", { path, content });

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
  collapseStacks: boolean,
) => invoke<boolean>("log_filter", { handle, levelBits, pattern, caseSensitive, collapseStacks });

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

// ─────────────────────────── Git ───────────────────────────

export interface GitEntry {
  /** 相对仓库根 */
  path: string;
  /** 暂存区状态字符：`.MADRCU` */
  index: string;
  /** 工作区状态字符 */
  work: string;
  untracked: boolean;
  /** 折叠的未跟踪目录（路径以 / 结尾），文件树要按前缀匹配 */
  isDir: boolean;
  conflicted: boolean;
  staged: boolean;
  unstaged: boolean;
  orig: string | null;
}

export interface GitStatus {
  root: string;
  branch: string;
  upstream: string;
  ahead: number;
  behind: number;
  detached: boolean;
  /** 一个提交都还没有 */
  unborn: boolean;
  entries: GitEntry[];
  truncated: boolean;
}

export interface GitCommit {
  sha: string;
  short: string;
  author: string;
  when: string;
  subject: string;
}

/** 找路径所属仓库根；不是仓库返回 null（正常情况，Git 功能整体隐身） */
export const gitRoot = (path: string) => invoke<string | null>("git_root", { path });

export const gitStatus = (root: string) => invoke<GitStatus>("git_status", { root });

export const gitDiff = (root: string, path: string, staged: boolean, untracked: boolean) =>
  invoke<string>("git_diff", { root, path, staged, untracked });

export const gitStage = (root: string, paths: string[]) =>
  invoke<void>("git_stage", { root, paths });

export const gitUnstage = (root: string, paths: string[]) =>
  invoke<void>("git_unstage", { root, paths });

/** 不可撤销 —— 调用前必须让用户确认过 */
export const gitDiscard = (root: string, paths: string[], untracked: string[]) =>
  invoke<void>("git_discard", { root, paths, untracked });

export const gitCommit = (root: string, message: string, amend = false) =>
  invoke<string>("git_commit", { root, message, amend });

export const gitLog = (root: string, path = "", limit = 50) =>
  invoke<GitCommit[]>("git_log", { root, path, limit });

/** 某个版本里的文件内容；不存在时是空串 */
export const gitShow = (root: string, rev: string, path: string) =>
  invoke<string>("git_show", { root, rev, path });

// ────────────── Git：历史 · 分支 · 工作树 ──────────────

export interface GitLogEntry {
  sha: string;
  short: string;
  author: string;
  email: string;
  when: string;
  date: string;
  subject: string;
  /** 父提交完整 sha；合并提交有多个，泳道图靠它连线 */
  parents: string[];
  refs: string[];
}

export interface GitBranch {
  name: string;
  sha: string;
  upstream: string;
  isHead: boolean;
  isRemote: boolean;
  when: string;
  subject: string;
}

export interface GitWorktree {
  path: string;
  sha: string;
  branch: string;
  detached: boolean;
  bare: boolean;
  locked: boolean;
  current: boolean;
}

export const gitLogEntries = (root: string, limit = 200, all = false, path = "") =>
  invoke<GitLogEntry[]>("git_log_entries", { root, limit, all, path });

export const gitCommitFiles = (root: string, sha: string) =>
  invoke<GitEntry[]>("git_commit_files", { root, sha });

export const gitCommitDiff = (root: string, sha: string, path = "") =>
  invoke<string>("git_commit_diff", { root, sha, path });

export const gitBranches = (root: string) => invoke<GitBranch[]>("git_branches", { root });

/** 切分支；create 为真时新建。工作区脏时 git 会拒绝，错误原样上抛 */
export const gitSwitch = (root: string, name: string, create = false) =>
  invoke<string>("git_switch", { root, name, create });

export const gitWorktrees = (root: string) => invoke<GitWorktree[]>("git_worktrees", { root });

/**
 * 新建工作树，返回新目录绝对路径 —— 可以直接当项目根打开。
 * 分支存不存在由 Rust 侧判断并决定加不加 `-b`。
 */
export const gitWorktreeAdd = (root: string, path: string, branch: string) =>
  invoke<string>("git_worktree_add", { root, path, branch });

/** 会删掉那个目录，调用前必须确认 */
export const gitWorktreeRemove = (root: string, path: string, force = false) =>
  invoke<void>("git_worktree_remove", { root, path, force });
