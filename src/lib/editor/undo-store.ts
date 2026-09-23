/**
 * 切标签时把撤销历史存起来，切回来接着用。
 *
 * # 为什么
 *
 * 编辑器是 `{#key tab.id}` 包着的（`shell/Content.svelte`）：切一次标签，整个编辑器销毁重建，
 * `EditorState` 连同撤销栈一起没了。2026-09-23 在桩里对照过：不切标签 ⌘Z 正常；打几个字、
 * 切走再切回来，⌘Z 撤不回来。09-17 那轮补的「回到上次看到哪」只救了光标和视口。
 *
 * # 存数据，不存对象
 *
 * 存的是 `state.toJSON({ history: historyField })`，重建时 `EditorState.fromJSON` 配上**新的**
 * 扩展。不能把旧的 `EditorState` 原样塞回去：它的扩展里有闭包，引用着已经销毁的那个组件实例
 * 的 `view` / props —— 跳转、保存、改动标记会悄悄对着一个死掉的编辑器干活。
 *
 * # 文档对不上就扔
 *
 * 只有存下时的文档和这次要显示的**一字不差**才恢复。切走期间文件被外部改了（git checkout、
 * 构建工具重写），旧的撤销步骤对着的是另一份文本，恢复出来 ⌘Z 会把字撤到错的位置上。
 *
 * # 只在内存里
 *
 * 不进会话快照：撤销栈可能很大，而跨重启的撤销 IDEA 也不给。条目按最近使用留 `MAX` 个。
 */
import { EditorState, type EditorStateConfig } from "@codemirror/state";
import { historyField } from "@codemirror/commands";

const MAX = 40;

const store = new Map<string, { doc: string; json: unknown }>();

/** 编辑器要走了：把这份文档的撤销历史（连同选区）存下 */
export function saveHistory(path: string, state: EditorState) {
  if (!path) return;
  store.delete(path);
  store.set(path, { doc: state.doc.toString(), json: state.toJSON({ history: historyField }) });
  if (store.size > MAX) store.delete(store.keys().next().value!);
}

/**
 * 建一个新的 state：有这份文档存下的历史、而且文档一字不差，就带着历史恢复；否则全新的。
 * 取过就删 —— 同一份历史不该被两个编辑器各拿一次（分屏里同一个文件开两份）。
 */
export function stateWithHistory(path: string, doc: string, config: EditorStateConfig): EditorState {
  const hit = store.get(path);
  store.delete(path);
  if (hit && hit.doc === doc) {
    try {
      return EditorState.fromJSON(hit.json, config, { history: historyField });
    } catch {
      // 历史的格式和这版 CM6 对不上（升级之后）—— 丢掉，不能让它挡住打开文件
    }
  }
  return EditorState.create({ ...config, doc });
}
