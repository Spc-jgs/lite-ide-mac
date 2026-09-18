/**
 * IPC 命令的封装 + 手写的 DTO。
 *
 * **DTO 全在这一个文件里**（`dto_sync.rs` 只读它）。命令封装则按「首屏之前要不要」分家
 * （issue #32 瘦身，2026-09-17）：这里留的是窗口出现之前就会调的 —— 读文件、写盘、草稿、
 * 会话、`gitStatus` 那几条；只有懒加载模块才用的搬去了同目录的 `git.ts` / `log.ts` /
 * `pty.ts` / `fs.ts` / `search.ts`，包装函数跟着调用方走，入口包不用带它们（−2.5 KB）。
 * 新加命令时先问一句「窗口出现之前有用吗」，没用就别放这儿。
 */
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
  /**
   * 生成物目录（`node_modules` `target` `dist` `build` …）。
   *
   * **它在树里、点得开** —— 这个标记只影响长相：压暗、不自动展开。
   * 原来这类目录是**根本不返回**的，于是一个真叫 `build/` 的源码目录
   * 在界面上凭空消失，还没有任何提示（issue #13）。
   */
  generated: boolean;
  /**
   * 这个名字**有第二种可能**（`dist` / `build` / `vendor`）——
   * `build/` 在 CMake 项目里放的是构建脚本，是源码。
   *
   * 名字只是怀疑，`ignoredDirs()` 的答案才是证据。`generated` 为假时
   * 这一位没有意义。见 issue #13。
   */
  contested: boolean;
}

/** 探测路径：目录还是文件，文件该用哪种模式打开 */
export const probePath = (path: string) => invoke<PathInfo>("probe_path", { path });

/**
 * 这个项目里哪些目录被 git 忽略了（相对项目根）。**按项目问一次，不是按目录。**
 *
 * `null` 和 `[]` 是两件事：`null` = 问不到 git（不是仓库、git 不在），
 * 那时退回按名字判；`[]` = 问到了，一个都没忽略。合成一个的话，
 * 一个干净的非 git 目录会被当成「git 说什么都没忽略」，`node_modules` 就不压暗了。
 */
export const ignoredDirs = (root: string) => invoke<string[] | null>("ignored_dirs", { root });

export interface TextFile {
  content: string;
  /** WHATWG 编码标签，如 UTF-8 / GBK */
  encoding: string;
  bom: boolean;
  /** 有解不出的字节；带着它保存会把那些字节永久换成 U+FFFD */
  lossy: boolean;
  /**
   * 盘上的换行符：`LF` / `CRLF` / `CR` / `mixed`。`content` 已经统一成 \n，
   * 保存时把它传回 `writeText` 才能原样写回 —— 和编码同一条规矩（issue #33 ③）
   */
  eol: string;
}

/** 读全文并探测编码；label 非空时按指定编码读 */
export const readText = (path: string, label?: string) =>
  invoke<TextFile>("read_text", { path, label: label ?? null });

/** 只探测编码，读头部采样 —— 日志模式用它决定 TextDecoder 的标签 */
export const detectEncoding = (path: string) => invoke<string>("detect_encoding", { path });

export interface Stamp {
  mtimeMs: number;
  size: number;
}

/** 文件指纹，用于判断是否被外部改动过 */
export const fileStamp = (path: string) => invoke<Stamp>("file_stamp", { path });

/** 在 Finder 里选中并显示。路径不在盘上时 reject */
export const revealInFinder = (path: string) => invoke<void>("reveal_in_finder", { path });

/**
 * 草稿目录的绝对路径。**不保证它已经在盘上** —— 只是想看看目录在哪，
 * 不该因为看一眼就留下一个空目录。
 */
export const scratchDir = () => invoke<string>("scratch_dir");

/**
 * 新建一份草稿，返回新路径。
 *
 * `stem` 是不带扩展名的名字，**由这边按本地时间生成** —— Rust 的 std 里
 * 没有本地时区，为一个文件名拽一个日期库进去不值。撞名由 Rust 侧加序号。
 */
/**
 * 草稿的锚点（M10）：在哪个项目 / 分支 / 提交 / 文件行写的，四样都可为空。
 * 存在文件头的 frontmatter 里，Rust 侧列表时解出来；解析器两边各一份
 * （`fsservice::frontmatter` / `state/frontmatter.ts`），判据一致。
 */
export interface ScratchAnchor {
  project: string;
  branch: string;
  head: string;
  /** `相对路径:行` */
  at: string;
}

/** 新建草稿。`anchor` 有一样不空就写进文件头 */
export const createScratch = (stem: string, anchor: ScratchAnchor | null = null) =>
  invoke<string>("create_scratch", { stem, anchor });

/** 草稿列表里的一条（issue #40） */
export interface ScratchEntry {
  name: string;
  path: string;
  /** 修改时间，Unix 毫秒。取不到时为 0 */
  mtimeMs: number;
  /** 正文第一行有字的内容（跳过文件头），Rust 侧截到 80 字符；空文件是空串 */
  firstLine: string;
  /** 文件头里的锚点；没有头就是 null */
  anchor: ScratchAnchor | null;
}

/** 「安装命令行工具…」的结果（issue #40） */
export interface CliInstall {
  /** 脚本真身的路径 */
  script: string;
  /** `/usr/local/bin/lite` 装上了没 */
  linked: boolean;
  replaced: boolean;
  /** 没装上时让人自己跑的那一句 `sudo ln -sf …` */
  linkCmd: string;
}

/** 装 `lite` 命令。开发构建（不在 .app 里）会 reject */
export const installCli = () => invoke<CliInstall>("install_cli");

/** 草稿目录里有什么，最近的在前。目录还不存在就是空列表，不是错误 */
export const listScratches = () => invoke<ScratchEntry[]>("list_scratches");

/**
 * 丢掉一份**一个字都没写过**的草稿 —— 应用里唯一一条真删除。
 *
 * 判据(必须是普通文件、必须 0 字节、必须在草稿目录里)全在 Rust 侧,
 * 这边传什么都绕不过去。有内容的草稿走的是「保存并关闭 / 丢弃改动」那条路,
 * 到不了这里。
 */
export const discardEmptyScratch = (path: string) =>
  invoke<void>("discard_empty_scratch", { path });

/** 原地改名，返回新路径。目标已存在时 reject（fs::rename 本身会静默覆盖） */
/** 移到废纸篓。应用里没有第二条删除路径 —— 不存在真删除 */
export const trashEntry = (path: string) => invoke<void>("trash_entry", { path });

/**
 * 保存并返回新指纹 —— 必须拿它更新记录，否则自己的保存会被当成外部修改。
 * 按 label 指定的编码写回；不传就是 UTF-8。
 */
/** `eol` 不传按 LF 写。凡是从 `readText` 来的内容都该把它的 `eol` 传回来 */
export const writeText = (path: string, content: string, label?: string, bom?: boolean, eol?: string) =>
  invoke<Stamp>("write_text", { path, content, label: label ?? null, bom: bom ?? false, eol: eol ?? null });

export const openLog = (path: string) => invoke<OpenResult>("open_log", { path });
export const closeLog = (handle: number) => invoke<boolean>("close_log", { handle });

/**
 * 启动时该打开的路径：命令行参数里的，加上系统在前端就绪前送来的
 * （Finder 双击 / 拖 Dock / `open -a`，issue #40）。空数组 = 什么都没指。
 *
 * **必须先挂好 `OPEN_PATHS_EVENT` 的监听再调它** —— 这一次调用把 Rust 侧
 * 标成「前端就绪」，之后送来的路径改为直接发事件，中间那一拍到的就丢了。
 */
export const initialPaths = () => invoke<string[]>("initial_paths");

/** 应用已在运行时系统又送来的路径。负载是 `string[]`，见 Rust 侧 `open.rs` */
export const OPEN_PATHS_EVENT = "open-paths";

export const diag = (msg: string) => invoke<void>("diag", { msg });

/**
 * 往 `~/Library/Logs/com.liteide.app/app.log` 写一条。
 *
 * **和 `diag` 分工不同**：`diag` 去 stderr、默认关、给「我现在在看」用；
 * 这条落盘、默认开、给「以后有人回头查」用。所以走这条的**只有异常** ——
 * 把执行轨迹也塞进去会把真正的错误埋掉。
 *
 * 自己 `catch` 掉：它跑在错误处理路径上，一个会二次抛的日志函数
 * 只会让原来那个错误更难看清。
 */
export const appLog = (level: "info" | "warn" | "error", source: string, msg: string) =>
  invoke<void>("app_log", { level, source, msg }).catch(() => {});

/** 日志文件的路径 —— 拿它开一个标签，用这个应用自己的日志引擎看 */
export const appLogPath = () => invoke<string>("app_log_path");

/**
 * Git 控制台里的一条（issue #29）。
 *
 * `ms` 是 Unix 毫秒，**Rust 侧刻意不格式化** —— 格式化要知道时区，
 * 而这边有 `Date`，按用户的本地时区显示才对。
 */
export interface GitCmd {
  ms: number;
  cwd: string;
  /** 完整 argv，**含加固参数**。看得到跑的是什么，正是这个控制台的第一个用途 */
  argv: string[];
  /** `null` = 没跑起来（git 不在），或者被主动掐掉了。两种都算失败 */
  code: number | null;
  durMs: number;
  err: string;
  errTruncated: boolean;
}

/** 清空应用日志（两份都清）。判据在 Rust 侧，前端只是按一下 */
export const clearAppLog = () => invoke<void>("clear_app_log");

/**
 * 启动完成时往 `app.log` 写一行预算数（issue #28）。
 *
 * **前端来叫是因为「启动完成」只有前端知道** —— Rust 的 `setup()` 返回时
 * 窗口还是白的，会话恢复和首屏渲染都在后头。`boot` 和 `self` 那两个数
 * 在 Rust 侧现量（`budget.rs`），这里只送过去四个前端才数得出来的。
 *
 * 自己 catch：一条量不出来的预算数不该变成界面上一句红字。
 */
export const reportBudget = (tabs: number, terms: number, editors: number, nodes: number) =>
  invoke<void>("report_budget", { tabs, terms, editors, nodes }).catch(() => {});

/**
 * 这份构建带不带 Web Inspector（issue #20）。
 *
 * `pnpm app:bundle:devtools` 和 `pnpm app:bundle` 装在同一个路径上，
 * 而「盘上只留一份 .app」是硬纪律 —— 所以只能让它自报家门。
 * 拿不到就当正式版：一个**误报成调试版**的正式版会让人白白重打一次包。
 */
export const devtoolsBuild = () => invoke<boolean>("devtools_build").catch(() => false);

/**
 * 文件系统监听（issue #33 ⑳）：从这一刻起 `root` 底下有变化就来一个 `fs-changed`
 * 事件（负载 `"git"` | `"files"`）。换根再调一次即可，旧的自动停；空串 = 停。
 * 起不来（路径没了、FSEvents 出错）只是少了实时刷新，焦点刷新那条路还在 —— 吞掉。
 */
export const watchRoot = (root: string) => invoke<void>("watch_root", { root }).catch(() => {});

// ─────────────────────────── 终端 ───────────────────────────

// ─────────────────────────── 搜索 ───────────────────────────

export interface Hit {
  path: string;
  line: number;
  text: string;
}

/** ⌘P / ⌘Click 用的文件索引。`truncated`：到了 Rust 侧 5 万的上限，后面的没看 */
export interface ProjectFiles {
  files: string[];
  truncated: boolean;
}

/** 列出项目文件（相对路径），模糊匹配在前端做 */
export const listProjectFiles = (root: string) =>
  invoke<ProjectFiles>("list_project_files", { root });

// ─────────────────────────── Git ───────────────────────────

export interface GitEntry {
  /** 相对仓库根 */
  path: string;
  /** 暂存区状态字符：`.MADRCU` */
  index: string;
  /** 工作区状态字符 */
  work: string;
  untracked: boolean;
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
  /** HEAD 的短 sha，空仓库是空串 */
  head: string;
  /** 一个提交都还没有 */
  unborn: boolean;
  /**
   * **只有文件。** 整个未跟踪的目录不在这里 —— 改动列表要回答
   * 「我改了哪些文件」，一个目录点不开差异，也说不清里面到底多了什么。
   */
  entries: GitEntry[];
  /**
   * 整个未跟踪的目录（路径以 `/` 结尾，相对仓库根）。
   *
   * 里面的文件已经摊开进了 `entries`；这份名单只给文件树 ——
   * 它靠这个前缀给目录本身上「未跟踪」的色，而不是只显示冒泡标记。
   */
  untrackedDirs: string[];
  truncated: boolean;
}

/** 找路径所属仓库根；不是仓库返回 null（正常情况，Git 功能整体隐身） */
export const gitRoot = (path: string) => invoke<string | null>("git_root", { path });

export const gitStatus = (root: string) => invoke<GitStatus>("git_status", { root });

export interface DiffText {
  text: string;
  /**
   * 超过 Rust 侧上限（1MB）被掐断了。
   *
   * 界面必须把这件事说出来 —— 一份看着完整、其实少了后半截的差异，
   * 比一句「显示不下」危险得多。
   */
  truncated: boolean;
}

/**
 * 文件在 HEAD 里的内容（issue #33 ④）：编辑器拿它当基线，在前端实时算改动行。
 * `null` = 不在 HEAD 里。形状复用 `DiffText`：一段文本 + 有没有被上限截断。
 */
export const gitHeadText = (root: string, path: string) =>
  invoke<DiffText | null>("git_head_text", { root, path });

/** blame 的一段（issue #33 ⑭）。`sha` 全零 = 未提交的行 */
export interface BlameHunk {
  sha: string;
  short: string;
  author: string;
  /** 作者时间，unix 秒 */
  time: number;
  summary: string;
  /** 现文件里的起始行（1-based） */
  start: number;
  count: number;
}
export interface Blame {
  hunks: BlameHunk[];
  /** 输出被 1MB 上限截断了：后面的行没有注解 */
  truncated: boolean;
}
export const gitBlame = (root: string, path: string) => invoke<Blame>("git_blame", { root, path });

/** 一条 stash（issue #33 ⑪） */
export interface GitStash {
  /** `stash@{N}` 里的 N */
  index: number;
  /** git 给的那句：`WIP on main: a1b2c3d 上一条提交的标题` */
  message: string;
}
export const gitTrustScan = (root: string) => invoke<TrustScan>("git_trust_scan", { root });
export const gitTrustGrant = (root: string, fingerprint: string) =>
  invoke<void>("git_trust_grant", { root, fingerprint });

export const gitStashList = (root: string) => invoke<GitStash[]>("git_stash_list", { root });
export const gitStage = (root: string, paths: string[]) =>
  invoke<void>("git_stage", { root, paths });

export const gitUnstage = (root: string, paths: string[]) =>
  invoke<void>("git_unstage", { root, paths });

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

/** 切分支；create 为真时新建。工作区脏时 git 会拒绝，错误原样上抛 */
/**
 * 切分支失败时拿到的东西。**不是一个字符串。**
 *
 * `kind === "local-changes"` 时 `files` 是挡路的那些文件 —— 界面据此给出
 * 「去提交 / 丢弃这些改动」两个按钮，而不是把 git 那句
 * "Please commit your changes or stash them" 原样贴出来。
 */
export interface SwitchErr {
  /** `local-changes` / `other` */
  kind: string;
  message: string;
  /** 挡路的文件，只有 `kind === "local-changes"` 时非空 */
  files: string[];
  /** git 的原话 */
  raw: string;
}

/** 一条白名单之外的 `.git/config` 键（issue #24），确认卡片里列给人看 */
export interface TrustSuspect {
  key: string;
  value: string;
  /** `file:.git/config` 那种；被 include 进来的指向别的文件 */
  origin: string;
}

/**
 * 开仓库前的信任扫描（issue #24）。`trusted` 为假时 Git 整块不启用（复用「不是 git 仓库」
 * 那套隐身），挂件位置留一条能点的提示；点「信任」把 `fingerprint` 原样传回 `gitTrustGrant`。
 */
export interface TrustScan {
  root: string;
  trusted: boolean;
  suspects: TrustSuspect[];
  /** 会执行的钩子名。只列出来知情，不影响 trusted */
  hooks: string[];
  fingerprint: string;
}

/** 删分支失败时拿到的东西。`kind === "not-merged"` 时界面给「仍然删除」 */
export interface BranchErr {
  /** `not-merged` / `other` */
  kind: string;
  message: string;
  /** git 的原话 */
  raw: string;
}

// ── 菜单栏 ───────────────────────────────────────────────────────────

/**
 * 开原生的「选择文件夹」面板。取消返回 null。
 *
 * 面板必须由 Rust 侧开 —— HTML 的 `<input webkitdirectory>` 给的是
 * 一堆文件条目而不是目录路径，而且拿不到绝对路径。
 */
export const pickFolder = () => invoke<string | null>("pick_folder");
/** 「另存为…」的原生保存面板；取消返回 null。覆盖确认面板自己做 */
export const pickSavePath = (dir: string | null, name: string) =>
  invoke<string | null>("pick_save_path", { dir, name });

/** 刷新「最近打开」子菜单。列表存在会话快照里，变了就把整张表推过来 */
export const setRecent = (paths: string[]) => invoke<void>("set_recent", { paths });

/**
 * 按当下的上下文让菜单项变灰。
 *
 * 没有标签时的「保存」、不是 Git 仓库时的「改动列表」—— 灰掉的菜单项
 * 本身就是一句解释：不是坏了，是现在用不上。
 */
export const syncMenuState = (hasTab: boolean, hasRepo: boolean, hasTerm: boolean, hasRoot: boolean) =>
  invoke<void>("sync_menu_state", { hasTab, hasRepo, hasTerm, hasRoot });

/** 交给系统默认浏览器打开。Rust 侧只放行 https —— 见那边的注释 */
export const openExternal = (url: string) => invoke<void>("open_external", { url });

// ── 拉取与推送 ───────────────────────────────────────────────────────

/**
 * 一条进度。
 *
 * `percent` / `done` / `total` 可能是 null —— git 的进度文案不是稳定接口
 * （`LC_ALL=C` 只保证是英文，不保证措辞不变）。认不出来时 `phase` 里是
 * 整段原文，界面显示成一行状态而不是进度条。
 */
export interface RemoteProgress {
  phase: string;
  percent: number | null;
  done: number | null;
  total: number | null;
  finished: boolean;
}

/**
 * 远程操作失败。
 *
 * `kind` 决定界面显示什么；`raw` 是 git 的原话，**必须留着能展开看** ——
 * 转译错了的时候人得有办法绕过我们（同差异视图的 `truncated`）。
 */
export interface RemoteErr {
  kind: "auth-https" | "auth-ssh" | "cancelled" | "rejected" | "conflict" | "other";
  message: string;
  raw: string;
}

