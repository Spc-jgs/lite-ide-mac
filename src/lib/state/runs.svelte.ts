/**
 * 「运行」工具窗里的那几次运行（#48，docs/TASKS.md）。
 *
 * 只放首屏就要的那一点：导轨上要不要出「运行」按钮、角标几个在跑、⌃R 跑哪个。动作（起、停、关、听退出）在懒加载的
 * `runs-ops.ts` —— 同 `replace.svelte.ts` / `replace-ops.ts` 的分法，入口包只付字段的钱。
 *
 * **不进会话快照**：任务是活的子进程，跨进程恢复不了（同终端，`terms.svelte.ts` 头上那条）。
 */
export type RunStatus = "running" | "stopping" | "stopped" | "done" | "failed";

export interface RunTab {
  id: number;
  root: string;
  name: string;
  command: string;
  /** 输出文件 */
  log: string;
  status: RunStatus;
  code: number | null;
}

class Runs {
  list = $state<RunTab[]>([]);
  activeId = $state<number | null>(null);

  /** 还活着的（跑着 / 正在软停）有几个：导轨角标 */
  alive(): number {
    return this.list.filter((r) => r.status === "running" || r.status === "stopping").length;
  }
}

export const runs = new Runs();
