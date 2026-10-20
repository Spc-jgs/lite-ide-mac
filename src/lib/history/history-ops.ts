/**
 * 打开本地历史（docs/IDEA.md 第 21 节）：菜单「文件 › 本地历史…」、编辑器右键。懒加载 —— 入口里一个字节不多
 * （菜单动作和编辑器右键菜单都是懒块，动态引它）。面板挂到 body 上（`mount`），不进 overlay 那张表：那张表在入口包里。
 */
import { mount, unmount } from "svelte";
import { tabs } from "../state/tabs.svelte";
import { docs } from "../state/docs.svelte";
import { project } from "../state/project.svelte";
import { notify } from "../state/notify.svelte";
import { readText } from "../ipc/commands";
import { viewOf } from "../editor/live-views";
import HistoryPanel from "./HistoryPanel.svelte";

let open: { comp: Record<string, unknown>; host: HTMLElement } | null = null;

function close() {
  if (!open) return;
  void unmount(open.comp);
  open.host.remove();
  open = null;
}

/** 等编辑器挂上（刚打开的标签要一会儿才有视图），最多等 3 秒 */
async function waitView(path: string) {
  for (let i = 0; i < 60; i++) {
    const v = viewOf(path);
    if (v) return v;
    await new Promise((r) => setTimeout(r, 50));
  }
  return null;
}

export function openHistory(path: string | null = tabs.active?.path ?? null) {
  if (!path) {
    notify.hint("先打开一个文件：本地历史是一个文件一个文件地记的");
    return;
  }
  close();
  const tab = tabs.byPath(path);
  const root = project.root;
  const rel = root && path.startsWith(`${root}/`) ? path.slice(root.length + 1) : path.slice(path.lastIndexOf("/") + 1);
  const host = document.createElement("div");
  document.body.append(host);
  const comp = mount(HistoryPanel, {
    target: host,
    props: {
      path,
      rel,
      label: tab?.encoding ?? "UTF-8",
      current: async () => viewOf(path)?.state.doc.toString() ?? (tab ? docs.liveText(tab) : (await readText(path)).content),
      onclose: close,
      onrestore: (text: string, when: string) => void restore(path, text, when),
    },
  });
  open = { comp, host };
}

/** 在编辑器里换成那一版：一笔修改（⌘Z 撤得回），不写盘 */
async function restore(path: string, text: string, when: string) {
  close();
  let v = viewOf(path);
  if (!v) {
    const { tabflow } = await import("../state/tabflow.svelte");
    await tabflow.openPath(path, { preview: false });
    v = await waitView(path);
  }
  if (!v) {
    notify.fail("编辑器没打开，没换成");
    return;
  }
  v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text }, userEvent: "input.paste", scrollIntoView: true });
  v.focus();
  notify.ok(`换成了 ${when} 那一版，还没保存（⌘Z 撤回）`);
}
