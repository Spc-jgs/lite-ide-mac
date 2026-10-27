/**
 * 跨文件替换（#42）的动作：扫描、预览、执行、撤销、启动时收拾上次的中断。跟着替换浮层懒加载，不进入口包。
 * 盘上的事全在 Rust（`replacesvc`），这里管两件前端独有的：**开着的标签怎么跟上**，以及卡片和浮层的状态。
 */
import { replace } from "./replace.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";
import { project } from "./project.svelte";
import { notify } from "./notify.svelte";
import { overlay } from "./overlay.svelte";
import { settled, stashed } from "./doc";
import { fileStamp, type ReplaceError, type ReplaceOutcome, type ReplaceSkip } from "../ipc/commands";
import { replaceApply, replacePending, replacePreview, replaceRecover, replaceScan, replaceUndo, type OpenDoc } from "../ipc/replace";
import { applyEdits, buildPicks } from "../search/replace-model";
import { worktree } from "./worktree.svelte";
import { git } from "./git.svelte";
import { rename as bmRename } from "./bookmarks";

/**
 * 此刻开着的编辑标签：路径、编辑器里的实时文本、有没有未保存改动。
 * Rust 拿它做三件事：有未保存改动的用编辑器里那份预览和执行；执行前核对它们没被改过；撤销时按此刻开没开选路
 */
export function openDocs(): OpenDoc[] {
  return tabs.list.filter((t) => t.mode === "edit").map((t) => ({ path: t.path, text: docs.liveText(t), dirty: t.dirty }));
}

const message = (e: unknown) => (e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? String((e as { message: unknown }).message) : String(e));

/*
 * 序号丢弃过期的结果：打字快的时候，先发的慢请求后到，会把新查询的结果盖回成旧的（QuickSearch 那条 `dead` 同一个形状）
 */
let scanSeq = 0;
let afterSeq = 0;

/** 查询或开关变了：重新扫。一次完整扫描在 5 万个文件上约 0.45s（docs/REPLACE.md 第 8 节），调用方负责 debounce */
export async function rescan() {
  const root = project.root;
  const q = replace.query;
  replace.error = null;
  replace.askNoUndo = null;
  const seq = ++scanSeq;
  if (!root || !q) {
    replace.scan = null;
    replace.after = [];
    replace.scanning = false;
    return;
  }
  replace.scanning = true;
  try {
    const s = await replaceScan(root, q, { ...overlay.searchOpts }, openDocs());
    if (seq !== scanSeq) return;
    replace.scan = s;
    await refreshAfter();
  } catch (e) {
    if (seq !== scanSeq) return;
    replace.scan = null;
    replace.after = [];
    replace.error = message(e);
  } finally {
    if (seq === scanSeq) replace.scanning = false;
  }
}

/** 替换串变了：只重算「改后」，不重新扫盘（Rust 留着上一次的扫描） */
export async function refreshAfter() {
  if (!replace.scan) return;
  const seq = ++afterSeq;
  try {
    const a = await replacePreview(replace.replacement);
    if (seq === afterSeq) replace.after = a;
  } catch (e) {
    if (seq === afterSeq) replace.error = message(e);
  }
}

/**
 * Rust 改完之后，开着的标签跟上。两种：
 * - **改到了盘上**（干净的标签）：和「外部改动重读」走同一条路 —— `settled` 换内容、`savedTick` 让已挂载的编辑器
 *   换成一笔事务（光标留着、⌘Z 能撤回）。**指纹顺手更新**：不然窗口下次拿焦点时会再被当成外部改动重读一遍，还弹一句「已被外部修改」
 * - **只改在编辑器里**（有未保存改动的标签）：改动落在实时文本上、作为草稿交回去，圆点还在 —— 那是用户自己的未完成工作
 */
export async function landInTabs(o: ReplaceOutcome) {
  for (const c of o.changed) {
    if (c.edits.length === 0) continue;
    const t = tabs.list.find((x) => x.path === c.path && x.mode === "edit");
    if (!t) continue;
    const next = applyEdits(docs.liveText(t), c.edits);
    if (c.onDisk) {
      Object.assign(t, settled(next));
      try {
        t.stamp = await fileStamp(t.path);
      } catch {
        /* 读不到指纹：下次焦点检查会重读一遍，内容是一样的 */
      }
    } else {
      Object.assign(t, stashed(t, next));
    }
  }
  docs.savedTick++;
  // 改了名的（Java 改类名连文件一起改、撤销时改回来）：开着它的标签挪到新路径，文件树重列。和文件树里改名走同一条路
  for (const r of o.renamed) {
    await worktree.renameOpenTabs(r.from, r.to, false, git.repo);
    // 书签跟着文件走（改类名、移动类、撤销时挪回来）
    bmRename(project.root, r.from, r.to);
  }
  if (o.renamed.length) worktree.treeTick++;
  // 改了盘上的文件：git 状态、文件树的颜色跟着刷（和保存之后同一个钩子）
  docs.hooks.afterSave?.();
}

/** 跳过了的那几个，一句话说清（多了只列前三个） */
function skippedText(s: ReplaceSkip[]): string {
  if (s.length === 0) return "";
  const head = s.slice(0, 3).map((x) => `${x.rel}：${x.text}`).join("；");
  return `；跳过 ${s.length} 个：${head}${s.length > 3 ? " …" : ""}`;
}

/** 执行。`allowNoUndo`：用户已经点了「这次撤销不了，仍然替换」 */
export async function apply(allowNoUndo = false) {
  const s = replace.scan;
  const root = project.root;
  if (!s || !root || replace.applying) return;
  const picks = buildPicks(s, replace.unchecked);
  if (picks.length === 0) return;
  replace.applying = true;
  replace.error = null;
  try {
    const o = await replaceApply(replace.replacement, picks, openDocs(), allowNoUndo);
    replace.askNoUndo = null;
    await landInTabs(o);
    const hits = o.changed.reduce((n, c) => n + c.count, 0);
    replace.done = o.undo ? { root, files: o.changed.length, hits } : null;
    // 盘上已经变了，这份扫描作废（拿它再执行一次只会全被判成「预览之后被改过」）
    replace.scan = null;
    replace.after = [];
    replace.unchecked = new Set();
    overlay.replaceOpen = false;
    const text = `已替换 ${o.changed.length} 个文件 ${hits} 处${skippedText(o.skipped)}`;
    // 撤销的退路就在这一句后面（2026-10-21：原来另有一张卡片把同一句话再说一遍）。胶囊走了还能从「编辑 › 撤销上次的替换 / 重命名」撤
    const act = o.undo ? UNDO : undefined;
    if (o.skipped.length) notify.fail(text, 8000, act);
    else notify.ok(text, undefined, act);
  } catch (e) {
    const err = e as Partial<ReplaceError>;
    if (err?.code === "too-big") replace.askNoUndo = err.bytes ?? 0;
    else replace.error = message(e);
  } finally {
    replace.applying = false;
  }
}

/** 胶囊上那个「撤销」（替换、Java 改名、快速修复做完都带它） */
export const UNDO = { label: "撤销", run: () => void undo() };

/**
 * 「编辑 › 撤销上次的替换 / 重命名」：胶囊走了以后的那条路，应用重启过也在（替换日志落在盘上，`checkPending` 启动时读回 `replace.done`）。
 * 菜单项一直亮着（灰不灰要把替换的状态拽进入口包）：没东西可撤时说一句为什么
 */
export function undoFromMenu() {
  if (!replace.done || replace.done.root !== project.root) {
    notify.hint("没有可撤销的替换或重命名（仅保留最近一次）");
    return;
  }
  void undo();
}

/** 撤销最近一次。先确认日志还是这个项目的那一次 —— 日志整个应用一份，别的窗口可能已经又替换过了 */
export async function undo() {
  const d = replace.done;
  try {
    const p = await replacePending();
    if (!p || p.state !== "done" || (d && p.root !== d.root)) {
      replace.done = null;
      notify.fail("无法撤销：该替换已不是最近一次（其他窗口中执行过新的替换）");
      return;
    }
    const o = await replaceUndo(openDocs());
    await landInTabs(o);
    replace.done = null;
    const text = `已撤销 ${o.changed.length} 个文件的更改${skippedText(o.skipped)}`;
    if (o.skipped.length) notify.fail(text, 8000);
    else notify.ok(text);
  } catch (e) {
    notify.fail(message(e));
  }
}

/**
 * 窗口开了一个项目：看看有没有上次留下的替换日志（docs/REPLACE.md 12.1 / 12.2）。
 * - 准备中断的：原文件一个没动，悄悄收拾掉临时文件
 * - 提交中断的、是这个项目的：卡片上问「退回 / 保留现状」（要你选，留卡片）
 * - 做完了的、是这个项目的：记下来，「编辑 › 撤销上次的替换 / 重命名」能撤（应用重启了也能撤）。**不弹**：
 *   原来启动时给一张「上次的替换还能撤销」卡片 —— 启动时冒出来的、不要你选的东西就是噪声（ui.md 第二之二条）
 */
export async function checkPending(root: string) {
  try {
    const p = await replacePending();
    if (!p) return;
    if (p.state === "preparing") {
      await replaceRecover(true);
      return;
    }
    if (p.root !== root) return;
    if (p.state === "committing") replace.interrupted = p;
    else replace.done ??= { root: p.root, files: p.files, hits: 0 };
  } catch {
    /* 问不到就算了：不影响别的功能，下次启动再问 */
  }
}

/** 中断卡片上的两个按钮 */
export async function resolveInterrupted(rollback: boolean) {
  const p = replace.interrupted;
  replace.interrupted = null;
  try {
    const o = await replaceRecover(rollback);
    await landInTabs(o);
    if (rollback) notify.ok(`已还原上次的替换（${o.changed.length} 个文件）`);
    else if (p) replace.done = { root: p.root, files: p.changed, hits: 0 };
  } catch (e) {
    notify.fail(message(e));
  }
}
