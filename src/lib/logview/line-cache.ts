import { decodeBlock } from "./block";
import { logLines } from "../ipc/commands";

/** 一次向 Rust 取多少行。太小则请求频繁，太大则单次延迟变高。 */
export const BLOCK_LINES = 512;

/** 最多缓存多少块：512 × 96 ≈ 5 万行，几 MB 量级，滚动时命中率足够。 */
const MAX_BLOCKS = 96;

/**
 * 块级 LRU 缓存。
 *
 * 虚拟滚动每帧都要问「第 N 行是什么」，不可能每行发一次 IPC；
 * 按块取 + LRU 是让滚动稳在 60fps 的关键。
 */
export class LineCache {
  private handle: number;
  private blocks = new Map<number, string[]>();
  private inflight = new Map<number, Promise<void>>();

  constructor(handle: number) {
    this.handle = handle;
  }

  /** 同步取一行；未命中返回 undefined，由调用方渲染占位并等待 onLoad */
  get(line: number): string | undefined {
    const id = Math.floor(line / BLOCK_LINES);
    const block = this.blocks.get(id);
    if (!block) return undefined;
    // LRU：命中即刷新到队尾
    this.blocks.delete(id);
    this.blocks.set(id, block);
    return block[line - id * BLOCK_LINES];
  }

  /** 确保 [from, to] 所在的块都在路上，返回本次触发的加载 */
  ensure(from: number, to: number, onLoad: () => void): void {
    const first = Math.floor(from / BLOCK_LINES);
    const last = Math.floor(to / BLOCK_LINES);
    for (let id = first; id <= last; id++) {
      if (this.blocks.has(id) || this.inflight.has(id)) continue;
      const p = logLines(this.handle, id * BLOCK_LINES, BLOCK_LINES)
        .then((buf) => {
          const { lines } = decodeBlock(buf);
          this.blocks.set(id, lines);
          this.evict();
        })
        .catch(() => {
          /* 句柄失效或越界：留空，下次滚动会重试 */
        })
        .finally(() => {
          this.inflight.delete(id);
          onLoad();
        });
      this.inflight.set(id, p);
    }
  }

  /** 行数增长（索引还在跑）时，最后一块可能不完整，丢掉让它重取 */
  invalidateTail(lineCount: number): void {
    const lastId = Math.floor(Math.max(0, lineCount - 1) / BLOCK_LINES);
    this.blocks.delete(lastId);
  }

  private evict(): void {
    while (this.blocks.size > MAX_BLOCKS) {
      const oldest = this.blocks.keys().next().value;
      if (oldest === undefined) break;
      this.blocks.delete(oldest);
    }
  }
}
