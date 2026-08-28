import { decodeBlock } from "./block";
import { logLines, logLinesFiltered, logFilterMap } from "../ipc/commands";
import { BLOCK_LINES, overBudget } from "./cache-budget";

export { BLOCK_LINES };

export interface Row {
  text: string;
  /** 物理行号（未过滤时等于视图行号） */
  phys: number;
}

interface Block {
  texts: string[];
  /** 过滤态下每行对应的物理行号；未过滤时为 null，行号由视图行号推出 */
  phys: number[] | null;
  /** 这一块的文本字符数，用来算驱逐预算 —— 见 cache-budget.ts */
  chars: number;
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
  /** 这个文件的编码标签，交给 TextDecoder 用 */
  private encoding: string;
  private blocks = new Map<number, Block>();
  private inflight = new Set<number>();
  /** 缓存里所有块的文本字符数之和，增删块时同步维护 */
  private chars = 0;

  constructor(handle: number, filtered: boolean, encoding = "utf-8") {
    this.handle = handle;
    this.filtered = filtered;
    this.encoding = encoding;
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
          this.chars += b.chars;
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
      const texts = decodeBlock(buf, this.encoding).lines;
      return { texts, phys: null, chars: countChars(texts) };
    }
    // 内容与行号映射并行取，省一个来回
    const [buf, phys] = await Promise.all([
      logLinesFiltered(this.handle, start, BLOCK_LINES),
      logFilterMap(this.handle, start, BLOCK_LINES),
    ]);
    const texts = decodeBlock(buf, this.encoding).lines;
    return { texts, phys, chars: countChars(texts) };
  }

  /** 行数增长（索引还在跑 / tail 追加）时，末块可能不完整，丢掉让它重取 */
  invalidateTail(lineCount: number): void {
    const lastId = Math.floor(Math.max(0, lineCount - 1) / BLOCK_LINES);
    this.drop(lastId);
  }

  /** 丢一块，同时把它的字符数从账上减掉 —— 少减一次，预算就永久偏高 */
  private drop(id: number): void {
    const b = this.blocks.get(id);
    if (!b) return;
    this.chars -= b.chars;
    this.blocks.delete(id);
  }

  private evict(): void {
    while (overBudget(this.blocks.size, this.chars)) {
      const oldest = this.blocks.keys().next().value;
      if (oldest === undefined) break;
      this.drop(oldest);
    }
  }
}

const countChars = (texts: string[]): number => {
  let n = 0;
  for (const t of texts) n += t.length;
  return n;
};
