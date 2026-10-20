/**
 * 「运行」工具窗里的那几次运行（#48，docs/TASKS.md）。
 *
 * 只放首屏就要的那一点：导轨上要不要出「运行」按钮、角标几个在跑、⌃R 跑哪个。动作（起、停、关、听退出）在懒加载的
 * `runs-ops.ts` —— 同 `replace.svelte.ts` / `replace-ops.ts` 的分法，入口包只付字段的钱。
 *
 * **不进会话快照**：任务是活的子进程，跨进程恢复不了（同终端，`terms.svelte.ts` 头上那条）。
 */
import type { DebugState, PortHolder, TaskStale, TestResults } from "../ipc/commands";

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
  /** 失败是因为端口被占：谁占着（运行窗上方那张卡片） */
  holder: PortHolder | null;
  /** Spring Boot 服务：在 Services 工具窗里，不进运行窗的标签（同 IDEA） */
  service: boolean;
  /** 什么时候起的（毫秒）：跑单测的只认这之后写的报告 */
  startedAt: number;
  /** 跑单测那次（「模块 [test …]」）的结果：跑完读 surefire 报告；还在跑、或者不是测试是 null */
  tests: TestResults | null;
  /** 调试着起的（docs/IDEA.md 第 18 节）：调试会话此刻的样子，"debug-state" 事件来一次换一次（`debug/debug-ops.ts`）；不是调试是 null */
  debug: DebugState | null;
}

class Runs {
  list = $state<RunTab[]>([]);
  activeId = $state<number | null>(null);
  /** 上次应用崩了、没停干净的任务（这个项目的）：确认条上那张卡片。启动后项目根定了问一次 */
  stale = $state<{ root: string; list: TaskStale[] } | null>(null);
  /** 这个项目认出了几个 Spring Boot 服务：导轨上要不要出 Services（ui.md 第十条「有内容才出现」）。`runs-ops.detectServices` 问 Rust */
  services = $state(0);

  /** 运行窗里的（不算 Services 那几个）：导轨上要不要出「运行」 */
  plain(): RunTab[] {
    return this.list.filter((r) => !r.service);
  }

  /** 还活着的（跑着 / 正在软停）有几个：导轨角标。运行窗和 Services 各数各的 */
  alive(service = false): number {
    return this.list.filter((r) => r.service === service && (r.status === "running" || r.status === "stopping")).length;
  }
}

export const runs = new Runs();
