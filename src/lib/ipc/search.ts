/** 随处搜索 / 项目内搜索的命令包装（issue #32 瘦身）：浮层是懒的 */
import { invoke } from "@tauri-apps/api/core";
import type { Hit } from "./commands";

export const grepProject = (root: string, pattern: string, limit = 200) =>
  invoke<Hit[]>("grep_project", { root, pattern, limit });

/** 搜草稿目录的内容（M10 ③）。路径是绝对的；文件头里的命中已经滤掉 */
export const grepScratches = (pattern: string, limit = 60) =>
  invoke<Hit[]>("grep_scratches", { pattern, limit });
