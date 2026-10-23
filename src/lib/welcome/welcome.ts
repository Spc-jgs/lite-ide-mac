/**
 * 欢迎页（docs/TAKEOVER.md G27）里能单测的那一半，连同只有它用的几个 IPC 封装（同 `image/zoom.ts`：封装跟着调用方走，
 * 放进 `ipc/` 下面的公共模块的话，多一个懒块共用它，入口里就多一张预加载表）。
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { RemoteProgress } from "../ipc/commands";

/** 最近项目现在在哪个分支：Rust 读 `.git/HEAD`，不起 git。不是仓库、目录没了都是 null */
export const recentBranch = (path: string) => invoke<string | null>("recent_branch", { path });
/** 从地址猜目录名（git 的规矩，Rust 那一份） */
export const cloneName = (url: string) => invoke<string>("git_clone_name", { url });
/** 原生的选目录面板，标题和起始目录可以给 */
export const pickDir = (title: string, dir: string | null) => invoke<string | null>("pick_folder", { title, dir });
/** 克隆到 `parent/name`，返回克隆下来的目录；`opId` 交给 `git_cancel` 喊停 */
export const gitClone = (parent: string, url: string, name: string, opId: number, onProgress: Channel<RemoteProgress>) =>
  invoke<string>("git_clone", { parent, url, name, opId, onProgress });

/** `/Users/me/x` → `~/x`。macOS 的家目录都在 `/Users/<名字>` 下；拿不准的原样给 */
export function tildify(path: string): string {
  const m = /^\/Users\/[^/]+(?=\/|$)/.exec(path);
  return m ? `~${path.slice(m[0].length)}` : path;
}

/** 项目名（最后一段） */
export function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1) || path;
}

/** 方块里那个字：同标题栏的项目挂件（首字母大写；中文名就是第一个字） */
export function initial(path: string): string {
  return ([...baseName(path)][0] ?? "?").toUpperCase();
}

/**
 * 按输入筛最近项目：名字或路径里有这几个字（不分大小写，空格分开的几段都要有）。
 * 名字里命中的排在只有路径命中的前面 —— 输 `lite` 要先看到 lite-ide，而不是住在 `~/lite/` 下面的别的项目
 */
export function filterRecent(list: string[], query: string): string[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return list;
  const hit = (s: string) => words.every((w) => s.includes(w));
  const byName = list.filter((p) => hit(baseName(p).toLowerCase()));
  const byPath = list.filter((p) => !byName.includes(p) && hit(tildify(p).toLowerCase()));
  return [...byName, ...byPath];
}

/** 克隆到哪儿的默认值：最近那个项目的上一级（人一般把项目放在同一个目录里）；一个都没有就空着，让人挑 */
export function defaultParent(recent: string[]): string {
  const p = recent[0];
  if (!p) return "";
  const i = p.lastIndexOf("/");
  return i > 0 ? p.slice(0, i) : "";
}

/** 克隆框能不能点「克隆」：地址、父目录、目录名都要有；目录名不能带 `/`、不能是 `.` `..`。不对就说为什么（空串 = 可以） */
export function cloneProblem(url: string, parent: string, name: string): string {
  if (!url.trim()) return "";
  if (url.trim().startsWith("-")) return "这不像一个仓库地址";
  if (!parent) return "选一个放它的目录";
  if (!name) return "起一个目录名";
  if (name.includes("/") || name === "." || name === "..") return "目录名里不能有 /";
  return "";
}

/** 喊停一个正在跑的克隆（和拉取共用 `git_cancel`，按 `(窗口, opId)` 认） */
export const cancelOp = (id: number) => invoke<boolean>("git_cancel", { id });
