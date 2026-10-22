/**
 * 最近位置（⇧⌘E）里不碰 store 的那一半：两份来源合成一张。单独一个文件、不引任何东西 —— 测试在裸 node 里跑它
 * （`recent-locations.ts` 静态引了 nav / files / docs，那几个是 runes，裸 node 里加载不了）
 */
export interface Spot {
  path: string;
  line: number;
}

/**
 * 两份合成一张：跳转历史（新的在前）在前、最近文件的光标位置在后；同一个文件同一行只留一条。
 * 最近文件没有记过位置的（打开过、光标没动）不列 —— 没有「那一行」可显示
 */
export function spotsOf(back: Spot[], recentFiles: string[], lineOf: (path: string) => number | undefined, max = 30): Spot[] {
  const out: Spot[] = [];
  const seen = new Set<string>();
  const add = (s: Spot) => {
    const k = `${s.path}\n${s.line}`;
    if (seen.has(k) || out.length >= max) return;
    seen.add(k);
    out.push(s);
  };
  for (const s of [...back].reverse()) add(s);
  for (const p of recentFiles) {
    const line = lineOf(p);
    if (line !== undefined) add({ path: p, line });
  }
  return out;
}
