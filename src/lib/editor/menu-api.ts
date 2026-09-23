import type { JumpHit } from "./jump";

/**
 * 编辑器右键时交给菜单的东西（`Editor.svelte` 的 `onContextMenu`）。
 * 剪切 / 复制 / 粘贴要碰编辑器内部的选区，只能由编辑器给；其余的项菜单自己拿 store 做。
 */
export interface EditorMenuApi {
  /** 右键那个位置能不能 ⌘B —— 能就是跳到哪 */
  hit: JumpHit | null;
  hasSelection: boolean;
  cut: () => Promise<void>;
  copy: () => Promise<void>;
  /** 读剪贴板可能被 WebView 拦下，读不到会抛 */
  paste: () => Promise<void>;
  /** 菜单关了把焦点还给编辑器 */
  focus: () => void;
}
