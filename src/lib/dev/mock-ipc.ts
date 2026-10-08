/**
 * 浏览器里的 Tauri IPC 桩 —— 只在 `pnpm dev` 且不在 Tauri 里时装载。
 *
 * 为什么值得留着：调 UI 若走 Tauri，每改一行都要等约 40 秒重新编译壳；
 * 挂上这个桩后在浏览器里改，热更新是毫秒级。它进不了生产产物，靠的是 `main.ts` 里
 * `if (import.meta.env.DEV) await import(...)` 那个死分支（生产构建里条件是常量假，整句在语法层面就没了），
 * **不是 tree-shaking** —— 原来静态 import 的写法就漏过 1,195 字节的假数据（frontend.md「别靠 tree-shaking」那节）。
 *
 * 喂的数据与 Rust 侧格式严格一致（含 log_lines 的线格式二进制），
 * 否则桩就失去了验证价值。
 */
import { appCmd } from "./mock/app";
import { fsCmd } from "./mock/fs";
import { scratchCmd } from "./mock/scratch";
import { searchCmd } from "./mock/search";
import { ptyCmd } from "./mock/pty";
import { logCmd } from "./mock/log";
import { gitCmd } from "./mock/git";
import { remoteCmd } from "./mock/remote";
import { type A, bump, FILES, NOT_MINE } from "./mock/data";

export function installMockIpc(): void {
  // 开发期钩子：在控制台模拟「文件被编辑器外改动」，用来验证冲突处理
  (window as unknown as Record<string, unknown>).__mockEditFileOutside = (
    path: string,
    content: string,
  ) => {
    FILES[path] = content;
    bump(path);
  };

  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    metadata: { currentWebview: { label: "main" }, currentWindow: { label: "main" } },
    transformCallback: (cb: unknown) => {
      const id = Math.floor(Math.random() * 1e9);
      (window as unknown as Record<string, unknown>)[`_cb${id}`] = cb;
      return id;
    },
    invoke: async (cmd: string, args: Record<string, never>) => {
      const a = args as unknown as A;
      // 按领域分文件（mock/）：挨个问，头一个认领的说了算 —— 命令名不重复，所以顺序无所谓。
      // 都不认就是 null，同原来 switch 的 default
      for (const handle of [appCmd, fsCmd, scratchCmd, searchCmd, ptyCmd, logCmd, gitCmd, remoteCmd]) {
        const r = await handle(cmd, a);
        if (r !== NOT_MINE) return r;
      }
      return null;
    },
  };

  /*
   * 事件插件的内部对象。
   *
   * `@tauri-apps/api/event` 的 unlisten 走的是**这个**对象上的
   * `unregisterListener`，不是 __TAURI_INTERNALS__ 上的。少了它，
   * App 里那个 onDragDropEvent 的清理函数一跑就抛
   * 「Cannot read properties of undefined」—— 而且是 uncaught，
   * 每次热更新刷一条，正是它要淹掉的那类真错误。
   *
   * 桩与真实现分叉就失去了全部价值。这里补齐它。
   */
  (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => {},
  };

  /*
   * 从控制台触发一条菜单动作：`__mockMenu("help-log")`。
   *
   * **浏览器里没有菜单栏**，而归菜单的动作有二十多条。它们在这里的另一条路
   * 是「连按两下 ⇧」的随处搜索 —— 那条路在自动化里按不出来
   * （裸修饰键的 keydown 传不下去），于是「打开应用日志」这类功能
   * 在浏览器里**一次都验不到**，只能等打成 `.app` 再说，而那是 45 秒一轮。
   *
   * 实现上不去猜哪个回调是菜单的：`@tauri-apps/api` 的事件负载自带
   * `event` 字段，每个监听器自己会对名字。所以广播给全部回调，
   * 认不认是它们自己的事。
   */
  (window as unknown as Record<string, unknown>).__mockMenu = (id: string) => {
    let n = 0;
    for (const k of Object.keys(window)) {
      if (!k.startsWith("_cb")) continue;
      const cb = (window as unknown as Record<string, unknown>)[k];
      if (typeof cb !== "function") continue;
      try {
        (cb as (e: unknown) => void)({ event: "menu", id: 0, payload: id });
        n++;
      } catch {
        /* 不是菜单的那些回调收到这个形状会抛，正常 */
      }
    }
    return `广播给 ${n} 个回调`;
  };
  // eslint-disable-next-line no-console
  console.info("[dev] Tauri IPC 桩已装载 —— 数据是假的，用于纯前端调试");
}
