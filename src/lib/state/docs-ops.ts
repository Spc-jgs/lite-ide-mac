import { readText, fileStamp } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";
import { settled } from "./doc";
import type { TabState } from "./tab";

/**
 * 文档生命周期里**有人点了才跑**的那几条：冲突裁决（「保留我的 / 用磁盘上的」）、
 * 换编码重开、改「下次存成什么」（编码 / 换行符）。状态和保存 / 自动保存 / 草稿回写
 * 在 `docs.svelte.ts`，那些首屏和每次敲键盘都要。
 *
 * 分开放的理由同 `tabflow-ops.ts` / `git-ops.ts`（入口包瘦身，2026-09-21）：入口包只放
 * 首屏之前必须解析执行完的那一段。`docs.svelte.ts` 里每条是一行转发。
 * 循环引用是有意的：这里静态 import `docs`，那边动态 import 这里。
 *
 * 逻辑从 `docs.svelte.ts` 搬过来，一个字不改；`this.` 换成 `docs.`。
 */

export async function resolveConflict(tab: TabState, take: "disk" | "mine") {
  tab.conflict = false;
  if (take === "disk") {
    try {
      const t = await readText(tab.path, tab.encoding);
      Object.assign(tab, settled(t.content));
      tab.eol = t.eol;
      tab.stamp = await fileStamp(tab.path);
      docs.savedTick++;
    } catch (e) {
      notify.fail(String(e));
    }
  }
  // take === "mine"：什么都不做，保留编辑器里的内容，
  // 下次 ⌘S 会覆盖磁盘 —— 指纹已经更新过，不会再重复告警
}

/** 按新编码重新解码当前文件 */
export async function reopenWith(label: string) {
  const tab = tabs.active;
  if (!tab) return;
  try {
    if (tab.mode === "log") {
      // 日志模式只是换个 TextDecoder 标签，不用重开句柄
      tab.encoding = label;
      return;
    }
    if (tab.dirty) {
      notify.fail("有未保存的改动，请先保存（⌘S）再换编码重新打开", 3000);
      return;
    }
    const t = await readText(tab.path, label);
    tab.content = t.content;
    tab.encoding = t.encoding;
    tab.bom = t.bom;
    tab.lossy = t.lossy;
    tab.eol = t.eol;
    docs.savedTick++;
    notify.ok(`已按 ${t.encoding} 重新打开${t.lossy ? "（仍有解不出的字节）" : ""}`, 3000);
  } catch (e) {
    notify.fail(String(e));
  }
}

/**
 * 只改「将来用什么换行符存」（issue #37），和 `saveAsEncoding` 同一条路：编辑器里
 * 一律是 `\n`（`fsservice::eol`），换行符只在写盘那一下换回去，所以这里不碰内容。
 * 「混用」的文件选了任一种，保存后就统一了。
 */
export function setEol(eol: "LF" | "CRLF") {
  const tab = tabs.active;
  if (!tab || tab.mode !== "edit" || tab.eol === eol) return;
  tab.eol = eol;
  tab.fmt = true;
  tab.dirty = true;
  tabs.keep(tab.id);
  notify.ok(`下次保存将用 ${eol} 换行，按 ⌘S 生效`, 3600);
}

/** 只改「将来存成什么编码」，不动当前内容 */
export function saveAsEncoding(label: string, bom: boolean) {
  const tab = tabs.active;
  if (!tab || tab.mode !== "edit") return;
  tab.encoding = label;
  tab.bom = bom;
  // 内容没变但目标编码变了，得让用户知道要按 ⌘S 才会真的落盘。
  // `fmt` 是让这个 dirty 活过编辑器重挂的那一位（见 doc.ts）
  tab.fmt = true;
  tab.dirty = true;
  tabs.keep(tab.id);
  notify.ok(`下次保存将写成 ${label}${bom ? " + BOM" : ""}，按 ⌘S 生效`, 3600);
}
