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
  /**
   * 经 Rust 的 pbpaste 读剪贴板（不弹 WKWebView 的确认）；超 8 MB 或读不到会抛。
   * 剪贴板里没有文字时返回 false、什么都不改 —— 空串替换选区等于删掉选区
   */
  paste: () => Promise<boolean>;
  /** 菜单关了把焦点还给编辑器 */
  focus: () => void;
}
