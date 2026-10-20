/**
 * 调试停下时「正停在哪一行」（docs/IDEA.md 第 18 节）：编辑器把那一行整行标出来（同 IDEA 的执行点）。
 * 写的是调试面板（停下 / 选了另一帧 / 接着跑了），读的是编辑器 —— 同 `breakpoints.ts`，单独一个小文件
 */
export interface ExecPoint {
  path: string;
  line: number;
  /** 是不是最顶上那一帧（真的停在这儿）；选了下面的帧时是「从这儿调进去的」，颜色淡一档 */
  top: boolean;
}

let cur: ExecPoint | null = null;
const subs = new Set<() => void>();

export const execPoint = () => cur;

export function setExecPoint(p: ExecPoint | null) {
  if (cur?.path === p?.path && cur?.line === p?.line && cur?.top === p?.top) return;
  cur = p;
  for (const f of subs) f();
}

export function onExecPoint(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}
