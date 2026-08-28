/**
 * 行缓存的驱逐预算。
 *
 * 单独成文件是为了能被 `tests/` 直接跑 —— `line-cache.ts` 引了 IPC，
 * 裸 node 加载不了它。这里只有纯计算，没有任何 import。
 *
 * # 为什么不能只按块数限
 *
 * 原来的上限是「最多 96 块」，注释写的是「512 × 96 ≈ 5 万行，几 MB 量级」。
 * 那个估算暗含了一个前提：**每行一百来字节**。而日志模式的触发条件之一
 * 恰恰是 `maxLineLen > 10k`（见 ARCHITECTURE §1）—— 也就是说，
 * 长行文件是这个模式的**目标用户**，不是意外情况。
 *
 * 一份每行 10KB 的访问日志（带完整请求体的那种，Java 后端很常见），
 * 滚过 49,152 行 = 缓存里躺着 **480MB**。而 ARCHITECTURE §7 写的是
 * 「1GB 日志常驻内存 < 200MB，**与文件大小无关**」。按块数限，
 * 这条保证就只对短行文件成立。
 *
 * # 为什么还要保底块数
 *
 * 光按字节限会走到另一个极端：10KB/行的文件，8M 字符只够 1.5 块，
 * 缓存装不下一屏的上下文，滚动会来回抖。所以是**两条同时生效**：
 * 块数超了一定驱逐；字节超了也驱逐，但至少留 `MIN_BLOCKS` 块。
 *
 * 长行文件的最坏占用因此是 `MIN_BLOCKS × 512 × 行长`，
 * 10KB/行时约 40MB —— 有上限，且远在预算之内。
 */

/** 一次向 Rust 取多少行。太小则请求频繁，太大则单次延迟变高。 */
export const BLOCK_LINES = 512;

/** 块数上限：512 × 96 ≈ 5 万行，短行文件下滚动命中率足够。 */
export const MAX_BLOCKS = 96;

/**
 * 字符数上限（JS 字符串是 UTF-16，约合 16MB）。
 *
 * 定这个数的依据：普通日志一行一百来字节，塞满 96 块也才 590 万字符，
 * **撞不到这条线**（先撞块数上限）。它只在长行文件上生效。
 */
export const MAX_CHARS = 8_000_000;

/** 字节超标时至少保留的块数，保证滚动还有上下文可用。 */
export const MIN_BLOCKS = 4;

/**
 * 还该不该继续赶人。
 *
 * @param blocks 当前缓存的块数
 * @param chars  这些块里文本的总字符数
 */
export function overBudget(blocks: number, chars: number): boolean {
  if (blocks > MAX_BLOCKS) return true;
  return chars > MAX_CHARS && blocks > MIN_BLOCKS;
}
