/**
 * 活着的终端交出来的口子，按终端标签的 id 记（#45 发送到终端）。
 *
 * 不放在 `state/terms.svelte.ts`：那个在入口包里（标签栏、菜单灰态都读终端列表），而用到这里的只有
 * 终端组件和「发送到终端」，两个都是懒的 —— 放进去入口包白涨几百字节。
 */

/**
 * 一个**活着、能接输入**的终端交出来的东西。终端组件自己的东西（xterm、pty id）
 * 不出组件，外面只拿这三样 —— 和编辑器交 `onLive` 是同一个形状。
 */
export interface TermHook {
  /** 当成你按了 ⌘V：程序开了 bracketed paste 就带上那对标记（claude 认作粘贴，不会一个字一个字地触发补全） */
  paste(text: string): void;
  /** 前台进程此刻的工作目录；拿不到是 null */
  cwd(): Promise<string | null>;
  focus(): void;
}

/** 就绪了的终端（`attach`）。不是 $state：没人按它渲染 */
const hooks = new Map<number, TermHook>();
const waiting = new Map<number, ((h: TermHook | null) => void)[]>();

/** 终端组件在 shell 能接输入时交上来、销毁时交 null */
export function attach(id: number, h: TermHook | null) {
  if (h) hooks.set(id, h);
  else hooks.delete(id);
  for (const r of waiting.get(id) ?? []) r(h);
  waiting.delete(id);
}

/**
 * 等这个终端就绪。刚 `open` 的那个要等组件懒加载、shell 起来、画出提示符 —— 平时不到一秒，
 * 但登录 shell 冷启动实测到过一分钟（ptysvc 的 `PROMPT_BUDGET` 那段），所以等足 60 秒：
 * 终端就在眼前开着，人看得见它在起；早早报「没发出去」反而要再按一次。等不到就 null，调用方说一声，不悬着。
 */
export function ready(id: number, ms = 60_000): Promise<TermHook | null> {
  const h = hooks.get(id);
  if (h) return Promise.resolve(h);
  return new Promise((resolve) => {
    const done = (x: TermHook | null) => {
      clearTimeout(timer);
      resolve(x);
    };
    const timer = setTimeout(() => {
      const rest = (waiting.get(id) ?? []).filter((r) => r !== done);
      if (rest.length) waiting.set(id, rest);
      else waiting.delete(id);
      resolve(null);
    }, ms);
    waiting.set(id, [...(waiting.get(id) ?? []), done]);
  });
}
