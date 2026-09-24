/** 文件树用的目录 / 新建 / 移动 / 改名（issue #32 瘦身）：文件树是懒的，`readText` / `writeText` / `probePath` 这些首屏就要的还在 `commands.ts` */
import { invoke } from "@tauri-apps/api/core";
import type { DirEntry } from "./commands";

/**
 * 列一层目录。点文件照列；生成物目录也列、只带 `generated` 标记由界面压暗（issue #13）；
 * 不列的只有版本库目录和系统杂物（`excludes::is_hidden`）。每个子目录带一条单层目录链（`chain`）
 */
export const listDir = (path: string) => invoke<DirEntry[]>("list_dir", { path });

/** 界面上给用户挑的编码清单：[标签, 说明][] */
export const listEncodings = () => invoke<[string, string][]>("list_encodings");

/**
 * 新建文件或目录，返回新路径。
 *
 * 递的是「哪个目录、叫什么」而不是拼好的路径：**join 和名字校验都在 Rust 侧**，
 * 前端少一个把文件写到别处去的机会。撞名一律 reject，绝不覆盖。
 */
export const createEntry = (dir: string, name: string, isDir: boolean) =>
  invoke<string>("create_entry", { dir, name, isDir });

/** 挪进另一个目录（文件树拖拽，issue #33 ⑨），名字不变，返回新路径 */
export const moveEntry = (path: string, dest: string) => invoke<string>("move_entry", { path, dest });

export const renameEntry = (path: string, name: string) =>
  invoke<string>("rename_entry", { path, name });

/**
 * 剪贴板里的纯文本（编辑器右键菜单的「粘贴」）。走 Rust 的 pbpaste 而不是 `navigator.clipboard`：
 * WKWebView 对网页主动读剪贴板每次都要再点一下系统的「Paste」气泡。超过 8 MB 会 reject —— 见
 * `fsservice::clipboard_text`
 */
export const readClipboard = () => invoke<string>("read_clipboard");
