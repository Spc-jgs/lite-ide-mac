/**
 * 「运行」工具窗里的那几次运行（#48，docs/TASKS.md）。
 *
 * 只放首屏就要的那一点：导轨上要不要出「运行」按钮、角标几个在跑、⌃R 跑哪个。动作（起、停、关、听退出）在懒加载的
 * `runs-ops.ts` —— 同 `replace.svelte.ts` / `replace-ops.ts` 的分法，入口包只付字段的钱。
 *
 * **不进会话快照**：任务是活的子进程，跨进程恢复不了（同终端，`terms.svelte.ts` 头上那条）。
 */
import type { BuildProblems, DebugState, HttpResponse, PortHolder, StuckProc, TaskStale, TestResults } from "../ipc/commands";
import type { HttpReq } from "../http/http-file";
import type { ScriptResult } from "../http/http-script";

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
  /** `holder` 是停完以后还占着端口的那个（#59）：卡片说「已停止，但…」、按钮只结束它、不重新运行 */
  lingering?: boolean;
  /** Spring Boot 服务：在 Services 工具窗里，不进运行窗的标签（同 IDEA） */
  service: boolean;
  /** 什么时候起的（毫秒）：跑单测的只认这之后写的报告 */
  startedAt: number;
  /** 跑单测那次（「模块 [test …]」）的结果：跑完读 surefire 报告；还在跑、或者不是测试是 null */
  tests: TestResults | null;
  /** 调试着起的（docs/IDEA.md 第 18 节）：调试会话此刻的样子，"debug-state" 事件来一次换一次（`debug/debug-ops.ts`）；不是调试是 null */
  debug: DebugState | null;
  /** `.http` 里发的一个请求（docs/IDEA.md 第 19 节，`http/http-ops.ts`）：不是任务，id 从二十亿起；关、停、重跑按它分开走。不是请求是 null */
  http: HttpRun | null;
  /** 构建那次（⌘F9，docs/IDEA.md 第 22 节）的编译错误：跑完从输出里认（`javac.rs`）。还在跑、或者不是构建，没有这个字段 */
  problems?: BuildProblems | null;
  /** 停不掉的（#68）：宽限期过了还在「正在停」时问来的 —— 组里活着、但不归我们管的进程。只在「正在停」时画（RunView 头上那张卡片） */
  stuck?: StuckProc[] | null;
}

export interface HttpRun {
  /** 发给 Rust 的操作号（取消用） */
  op: number;
  /** 哪个 `.http` 文件 */
  path: string;
  /** 文件里写的那个请求（重发时按名字找回它） */
  req: HttpReq;
  /** 用的哪个环境 */
  env: string | null;
  /** 变量替换好之后发出去的 */
  method: string;
  url: string;
  resp: HttpResponse | null;
  /** 没发成（连不上、超时、取消）的原因 */
  error: string;
  /** 响应处理脚本跑出来的（`http/http-script.ts`）：设了哪些变量、哪几句没执行、哪些取不到。没写脚本是 null */
  script?: ScriptResult | null;
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
