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

import { spotsOf } from "./recent-spots";

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
