/**
 * 草稿自动保存的**判据**（issue #40 第一层）。纯函数，零 import —— `tests/` 里裸 node 跑。
 *
 * # 为什么只有草稿自动存
 *
 * 草稿一出生就有真实路径、由机器命名、住在应用自己的目录里 —— 当初选
 * 「scratch 而不是 Untitled」（JOURNAL 2026-09-09）已经把「存哪、叫什么」
 * 两个问题拆掉了，但「要不要存」还留着让人按 ⌘S。Sublime 赢在 hot exit：
 * 三个问题一个都不用答。草稿自动落盘就是把最后那个也拆掉。
 *
 * **项目文件一律不自动存**，这是有意的：半成品写进盘会触发 watcher、构建工具
 * 和 git 差异，那些代价是真的；而草稿目录里什么都没有在盯着它。
 *
 * # 时机
 *
 * 停止输入 `AUTOSAVE_IDLE_MS` 后一次；标签失活、关闭、窗口失焦、退出各补一次
 * （`force`）。写失败退避 `AUTOSAVE_RETRY_MS` 再试 —— 盘满、没权限这种
 * 不会在 500ms 内自己好，每半秒撞一次只是刷屏。
 */

/** 停止输入多久之后落盘。太短会在打字中间写盘，太长会让「切走前没存上」的窗口变宽 */
export const AUTOSAVE_IDLE_MS = 500;
/** 上一次写失败之后，至少隔多久再试 */
export const AUTOSAVE_RETRY_MS = 5000;

export interface AutosaveInput {
  /** 这个标签是不是草稿目录下的 */
  scratch: boolean;
  /** 编辑模式（日志 / 差异 / 合并没有「保存」这回事） */
  editing: boolean;
  dirty: boolean;
  /** 外部改过而本地也有改动 —— 那是现成的「用磁盘上的 / 保留我的」流程的事 */
  conflict: boolean;
  /** 距上次输入过了多久（毫秒）。没输入过就是 Infinity */
  idleMs: number;
  /** 距上次写失败过了多久（毫秒）。没失败过就是 null */
  failedMs: number | null;
  /** 强制：切走 / 关闭 / 失焦 / 退出，不等空闲期 */
  force: boolean;
}

/**
 * 现在该不该把这个标签写进盘。
 *
 * 顺序有讲究：**「是不是草稿」排第一**，非草稿后面什么条件都不看 ——
 * 这一条去掉的话测试要红（项目文件永远不自动存）。
 */
export function autosaveDue(i: AutosaveInput): boolean {
  if (!i.scratch) return false;
  if (!i.editing || !i.dirty || i.conflict) return false;
  if (i.failedMs !== null && i.failedMs < AUTOSAVE_RETRY_MS) return false;
  if (i.force) return true;
  return i.idleMs >= AUTOSAVE_IDLE_MS;
}

/**
 * 草稿「存了没存」给界面看的那一个状态（2026-09-17 体感那轮）。
 *
 * 调研里被抱怨的是「合上盖子前那一下：到底存没存」。草稿是自动存的，可界面原来
 * 只有那颗给项目文件用的「未保存」圆点：每敲一串字亮半秒、灭掉，标题还写着
 * 「关闭前会问」—— 而草稿关闭前根本不问（`autosaveBeforeClose` 静默存）。
 * 一个每半秒闪一次、说的还不对的信号，等于没有信号。
 *
 * 三个状态，标签栏和状态栏共用这一个函数，两处不会说出两套话：
 * - `pending`：改了、还没落盘（半秒内会落）。圆点不亮 —— 这不是要人管的状态；
 *   状态栏写「自动保存…」，像 Google Docs 的 Saving…
 * - `saved`：盘上就是编辑器里的。状态栏写「已自动保存」
 * - `failed`：上一次写失败（盘满、没权限）。这是唯一要人管的：圆点亮、警示色，
 *   状态栏说「⌘S 重试」。`docs.autosaveSweep` 已经弹过一次红字，这里是常驻的提醒
 *
 * 非草稿返回 null：它们走原来那套（圆点 = 未保存 = 关闭前会问）。
 */
export type ScratchSaveState = "pending" | "saved" | "failed";

export function scratchSaveState(i: { scratch: boolean; dirty: boolean; failed: boolean }): ScratchSaveState | null {
  if (!i.scratch) return null;
  if (i.failed) return "failed";
  return i.dirty ? "pending" : "saved";
}

export const SCRATCH_SAVE_LABEL: Record<ScratchSaveState, string> = {
  pending: "自动保存…",
  saved: "已自动保存",
  failed: "自动保存失败 · ⌘S 重试",
};
