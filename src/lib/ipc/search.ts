/** 随处搜索 / 项目内搜索的命令包装（issue #32 瘦身）：浮层是懒的 */
import { invoke } from "@tauri-apps/api/core";
import type { Hit } from "./commands";

/**
 * 内容搜索的三个开关（#42），和 ⇧⌘F 浮层上的三个按钮一一对应，默认都关。
 * 没有 smart-case：语义只由这三个开关决定，界面上看得见（docs/REPLACE.md 第 3 节）
 */
export interface SearchOpts {
  case: boolean;
  word: boolean;
  regex: boolean;
}

export const NO_OPTS: SearchOpts = { case: false, word: false, regex: false };

/** 正则写错了会 reject，消息是「正则写错了：…」，可以直接给人看 */
export const grepProject = (root: string, pattern: string, opts: SearchOpts, limit = 200) =>
  invoke<Hit[]>("grep_project", { root, pattern, ...opts, limit });

/** 搜草稿目录的内容（M10 ③）。路径是绝对的；文件头里的命中已经滤掉 */
export const grepScratches = (pattern: string, opts: SearchOpts, limit = 60) =>
  invoke<Hit[]>("grep_scratches", { pattern, ...opts, limit });
