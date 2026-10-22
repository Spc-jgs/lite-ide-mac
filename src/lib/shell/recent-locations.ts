/**
 * 最近位置（⇧⌘E，照 IDEA 的 Recent Locations；docs/TAKEOVER.md G6）：最近跳过的地方（⌘B、点搜索结果……那份回退历史）
 * 加上最近打开的文件各自停在的那一行，带那一行代码。⌘E 是「最近的文件」，这个是「最近的位置」。
 * 懒的：菜单那边一行 `import()`
 */
import { mount, unmount } from "svelte";
// 静态引：这三个本来就在入口包里。写成 `await import()` 的话 Rollup 要在入口里给每个造一个模块命名空间对象（三个 +306 B，量过；同 notify 那次，frontend.md）
import { nav } from "../state/nav.svelte";
import { files } from "../state/files.svelte";
import { docs } from "../state/docs.svelte";

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

let open: ReturnType<typeof mount> | null = null;

export async function openRecentLocations() {
  if (open) return;
  const { default: C } = await import("./RecentLocations.svelte");
  const spots = spotsOf(nav.back, files.recent, (p) => docs.posByPath.get(p)?.line);
  open = mount(C, {
    target: document.body,
    props: {
      spots,
      onclose: () => {
        if (open) void unmount(open);
        open = null;
      },
    },
  });
}
