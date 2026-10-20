/**
 * 此刻挂着的编辑器：路径 → CM6 的视图。给「编辑器外面的东西要在编辑器里做一笔能 ⌘Z 撤的修改」用 ——
 * 本地历史的「恢复成这一版」（docs/IDEA.md 第 21 节）。跨文件替换、改名不走这儿：它们改的是盘，开着的标签换成一份草稿（replace-ops 的 landInTabs）。
 * 跟着编辑器那个懒加载的块走，不进入口包。Editor 挂载时登记、销毁时注销（和 `docs.onEditorLive` 同一个时机）
 */
import type { EditorView } from "@codemirror/view";

const views = new Map<string, EditorView>();

export function registerView(path: string, v: EditorView) {
  views.set(path, v);
}

/**
 * 注销**只删自己那条**：同一个路径要是先后挂过两个编辑器（新的先登记、旧的后销毁），按路径一删会把新的那条也删掉。
 * 这是防着的，没有实际撞见过 —— 2026-10-20 验收里「恢复了却一个字没变」先疑到这儿，查下来是验收脚本点错了行（见 JOURNAL 同日）。
 * 真撞见过的是另一半：同一个编辑器换文件（不重建视图、只换路径）时原来只注销不重新登记，换过去的文件「恢复」找不到编辑器 —— Editor 里换完路径会再登记一次。
 * 同 frontend.md「交出去的时候要带上自己的身份」
 */
export function unregisterView(path: string, v: EditorView | null) {
  if (views.get(path) === v) views.delete(path);
}

export const viewOf = (path: string): EditorView | null => views.get(path) ?? null;
