/**
 * 覆盖率（docs/TAKEOVER.md G15，IDEA 的 Run with Coverage）：最近一次带覆盖率跑出来的报告。编辑器行号旁的颜色（`editor/coverage-ext.ts`）和
 * 底部「覆盖率」工具窗都读它。只留最近一次、只在内存里（IDEA 也是：换一次跑就换一份，「清掉」就没了）。不是 Svelte 的状态：两个懒块各自订阅
 */
import type { Coverage, CoverageFile } from "../ipc/commands";

export interface Lines {
  covered: Set<number>;
  partial: Set<number>;
  missed: Set<number>;
}

let report: { name: string; files: CoverageFile[]; total: number } | null = null;
let byPath = new Map<string, Lines>();
const subs = new Set<() => void>();

export function setCoverage(name: string, c: Coverage) {
  report = { name, files: c.files, total: c.total };
  byPath = new Map(c.files.map((f) => [f.path, { covered: new Set(f.covered), partial: new Set(f.partial), missed: new Set(f.missed) }]));
  for (const f of subs) f();
}

export function clearCoverage() {
  report = null;
  byPath = new Map();
  for (const f of subs) f();
}

export const coverageOf = (path: string): Lines | undefined => byPath.get(path);
export const lastCoverage = () => report;

/** 一个文件的行覆盖率（跑到一部分的算跑到），0..1；没有可执行的行算 1 */
export function filePct(f: { covered: unknown[]; partial: unknown[]; missed: unknown[] }): number {
  const all = f.covered.length + f.partial.length + f.missed.length;
  return all ? (f.covered.length + f.partial.length) / all : 1;
}

/** 全部文件合起来的行覆盖率和行数 */
export function totalPct(files: CoverageFile[]): { pct: number; hit: number; all: number } {
  let hit = 0;
  let all = 0;
  for (const f of files) {
    hit += f.covered.length + f.partial.length;
    all += f.covered.length + f.partial.length + f.missed.length;
  }
  return { pct: all ? hit / all : 1, hit, all };
}

export function onCoverage(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}
