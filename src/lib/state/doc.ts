/**
 * 编辑标签的「文本三件套」状态机：磁盘那份 / 未保存的草稿 / 脏标记。
 *
 * 抽出来不是为了好看 —— 是这三个字段之间的关系一共出过三个会丢数据的 bug，
 * 而它们原来全长在 `App.svelte` 里，那两千五百行没有一行是可测的：
 *
 * 1. 「保存并关闭」写的是 `draft ?? content`，而当前标签的编辑器还活着 ——
 *    两个都不是它里面的实时文本。最短复现是「打开文件 → 打几个字 →
 *    ✕ → 保存并关闭」：写回磁盘的是**原文**，界面还说「已保存」。
 * 2. 外部改动后重读时忘了清 `draft`，切走再切回来会用陈草稿盖掉刚读回来的内容。
 * 3. （历史）只有 `content` 一个字段时，草稿会把基线顶掉，dirty 再也算不出来。
 *
 * 前两个是同一个形状：**同一件事在三个地方各写了一遍，其中一处少一行**。
 * 所以「读回磁盘」这件事这里只有 [`settled`] 一个出口，三处调用它。
 */

export interface Doc {
  /** 磁盘上那份。dirty 的基线 */
  content?: string;
  /** 未保存的草稿。只有和 `content` 不同才存在 */
  draft?: string;
  dirty: boolean;
}

/**
 * 这个标签当前**该保存**的文本。
 *
 * `live` 是当前挂载着的那个编辑器交出来的实时文本；这个标签没有活着的
 * 编辑器就传 `null`。
 *
 * 三者的优先级不能反：编辑器活着的时候它是唯一的真相。`draft` 只在
 * 换文件或销毁时回写一次、`content` 是磁盘那份 —— 两个都可能停在几步之前。
 *
 * 用 `??` 而不是 `||`：用户把整个文件清空时 `live` 是空串，那是一份
 * 合法的、要写回去的内容，不能掉到下一档去。
 */
export function textToSave(doc: Doc, live: string | null): string {
  return live ?? doc.draft ?? doc.content ?? "";
}

/**
 * 磁盘那份成了准（保存成功 / 外部改动后重读 / 冲突时选「用磁盘上的」）。
 *
 * **草稿必须一起清掉**：留着它，下次切回这个标签时 `initial` 取的是草稿，
 * 刚落定的内容当场被顶掉，而且会被算成「有未保存改动」。
 */
export function settled(content: string): Doc {
  return { content, draft: undefined, dirty: false };
}

/**
 * 编辑器把实时文本交回来（换文件或销毁之前）。
 *
 * 改回原样时要把草稿**清掉**而不是存一份和磁盘一样的：留着它，
 * 下次切回来就顶着一份「和磁盘相同的草稿」，基线判断从此多一层拐弯。
 */
export function stashed(doc: Doc, text: string): Doc {
  return text === (doc.content ?? "")
    ? { ...doc, draft: undefined, dirty: false }
    : { ...doc, draft: text, dirty: true };
}
