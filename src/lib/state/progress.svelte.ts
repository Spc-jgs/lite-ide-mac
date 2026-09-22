import { untrack } from "svelte";
import { notify } from "./notify.svelte";
import { remote } from "./remote.svelte";

/**
 * 后台任务的进度，**全应用只有状态栏一处显示**（2026-09-22，照 IDEA 右下角那条）。
 *
 * # 为什么收成一处
 *
 * 之前五种「后台在跑」四种长相：拉取推送是编辑器顶上一张卡片带 3px 条；日志索引是状态栏
 * 一句「索引中…」；过滤扫描是过滤条上的计数；提交跑钩子是状态栏左边「正在提交…」；
 * 列目录是就地一个环。日志索引和过滤明明有真百分比（`indexedBytes / totalBytes`、
 * `scannedLines`）却没用上。人分不清这些是不是同一类事 —— 它们就是同一类事：
 * 一件在后台跑、跑完会自己消失、有的能取消。
 *
 * # 三个来源，一个列表
 *
 * - 显式登记：`progress.start(label)` 拿到句柄，`set` 更新、`end` 收掉（日志索引 / 扫描 / 过滤）。
 *   **`end` 必须在 finally 或 effect 的 cleanup 里**，漏掉就是一条永远转着的任务。
 * - `remote.syncing`：拉取 / 推送 / 抓取。那份状态本来就在，这里只是把它读成任务，
 *   不在 remote-ops 里再写一遍 —— 同一个事实两处写，早晚漂。
 * - `notify.doing`：提交 / 切分支这类没有百分比的写操作（`gitDo` 300ms 之后才放出来）。
 *   文案保持「正在提交…」一整句，smoke.sh 按它找。
 *
 * 状态栏只画**最新**的那个，多于一个时加「+N」。IDEA 也是这么做的：并发的后台任务本来
 * 就少，为它们做一个列表浮层是杀鸡用牛刀。
 */
export interface Task {
  id: string;
  label: string;
  /** 0–100；null = 不知道多久（来回跑的条） */
  percent: number | null;
  cancel: (() => void) | null;
}

export interface TaskHandle {
  set(patch: { label?: string; percent?: number | null }): void;
  end(): void;
}

const WHAT = { pull: "拉取", push: "推送", fetch: "抓取" } as const;

class Progress {
  /** 显式登记的 */
  tasks = $state<Task[]>([]);
  #seq = 0;

  /*
   * 三个改写都 `untrack`：调用方多半在 `$effect` 里（LogPane 看着 `stat` 变就 set 一次），
   * 改写前要读一遍 `this.tasks`，不 untrack 的话那个 effect 就依赖上了自己写的东西 ——
   * `effect_update_depth_exceeded`，日志一打开就撞（2026-09-22 实测）。
   */
  start(label: string, opts: { percent?: number | null; cancel?: () => void } = {}): TaskHandle {
    const id = `t${++this.#seq}`;
    untrack(() => {
      this.tasks = [...this.tasks, { id, label, percent: opts.percent ?? null, cancel: opts.cancel ?? null }];
    });
    return {
      set: (patch) => {
        untrack(() => {
          this.tasks = this.tasks.map((t) => (t.id === id ? { ...t, ...patch } : t));
        });
      },
      end: () => {
        untrack(() => {
          this.tasks = this.tasks.filter((t) => t.id !== id);
        });
      },
    };
  }

  /** 三个来源合成的列表，新的在后 */
  get all(): Task[] {
    const out: Task[] = [...this.tasks];
    const s = remote.syncing;
    if (s) {
      out.push({
        id: "remote",
        label: `${WHAT[s.what]} · ${s.phase}`,
        percent: s.percent,
        // push 进行中不给取消：中途掐掉远程状态不确定（remote-ops 那边同一条判据）
        cancel: s.what === "push" ? null : () => remote.cancel(),
      });
    }
    if (notify.doing) out.push({ id: "doing", label: `正在${notify.doing}…`, percent: null, cancel: null });
    return out;
  }

  /** 状态栏画的那一个 */
  get current(): Task | null {
    const all = this.all;
    return all.length ? all[all.length - 1] : null;
  }
}

export const progress = new Progress();
