import { decodeBlock } from "./block";
import { logLines, logLinesFiltered, logFilterMap } from "../ipc/commands";

/** 一次向 Rust 取多少行。太小则请求频繁，太大则单次延迟变高。 */
export const BLOCK_LINES = 512;

/** 最多缓存多少块：512 × 96 ≈ 5 万行，几 MB 量级，滚动时命中率足够。 */
const MAX_BLOCKS = 96;

export interface Row {
  text: string;
  /** 物理行号（未过滤时等于视图行号） */
  phys: number;
}

interface Block {
  texts: string[];
  /** 过滤态下每行对应的物理行号；未过滤时为 null，行号由视图行号推出 */
  phys: number[] | null;
}

/**
 * 块级 LRU 缓存。
 *
 * 虚拟滚动每帧都要问「第 N 行是什么」，不可能每行发一次 IPC；
 * 按块取 + LRU 是让滚动稳在 60fps 的关键。
 *
 * 过滤态下多取一份「视图行 → 物理行」的映射，好让行号栏显示真实行号 ——
 * 过滤后还显示 1,2,3 会让人对不上原始文件。
 */
export class LineCache {
  private handle: number;
  private filtered: boolean;
  private blocks = new Map<number, Block>();
  private inflight = new Set<number>();

  constructor(handle: number, filtered: boolean) {
    this.handle = handle;
    this.filtered = filtered;
  }

  /** 同步取一行；未命中返回 undefined，由调用方渲染占位并等待 onLoad */
  get(line: number): Row | undefined {
    const id = Math.floor(line / BLOCK_LINES);
    const block = this.blocks.get(id);
    if (!block) return undefined;
    // LRU：命中即刷新到队尾
    this.blocks.delete(id);
    this.blocks.set(id, block);
    const i = line - id * BLOCK_LINES;
    const text = block.texts[i];
    if (text === undefined) return undefined;
    return { text, phys: block.phys ? block.phys[i] : line };
  }

  /** 确保 [from, to] 所在的块都在路上 */
  ensure(from: number, to: number, onLoad: () => void): void {
    const first = Math.floor(from / BLOCK_LINES);
    const last = Math.floor(to / BLOCK_LINES);
    for (let id = first; id <= last; id++) {
      if (this.blocks.has(id) || this.inflight.has(id)) continue;
      this.inflight.add(id);
      this.load(id)
        .then((b) => {
          this.blocks.set(id, b);
          this.evict();
        })
        .catch(() => {
          /* 句柄失效或越界：留空，下次滚动会重试 */
        })
        .finally(() => {
          this.inflight.delete(id);
          onLoad();
        });
    }
  }

  private async load(id: number): Promise<Block> {
    const start = id * BLOCK_LINES;
    if (!this.filtered) {
      const buf = await logLines(this.handle, start, BLOCK_LINES);
      return { texts: decodeBlock(buf).lines, phys: null };
    }
    // 内容与行号映射并行取，省一个来回
    const [buf, phys] = await Promise.all([
      logLinesFiltered(this.handle, start, BLOCK_LINES),
      logFilterMap(this.handle, start, BLOCK_LINES),
    ]);
    return { texts: decodeBlock(buf).lines, phys };
  }

  /** 行数增长（索引还在跑 / tail 追加）时，末块可能不完整，丢掉让它重取 */
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
