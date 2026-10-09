import type { ReplacePending, ReplaceScan } from "../ipc/commands";

/**
 * 跨文件替换（#42）的状态。**放 store 不放浮层**：浮层一 Esc 就销毁，而查询、替换串、你取消勾选的那几处
 * 要留着 —— ⇧⌘R 再开回到原样（docs/REPLACE.md 第 4 节；IntelliJ 靠「Open in Find Window」换个不会关的窗口解决同一件事）。
 * 动作在 `replace-ops.ts`（懒加载，跟着浮层来），这里只放会被入口包里的人读的字段。
 */
class ReplaceState {
  query = $state("");
  replacement = $state("");
  /** 取消勾选的那几处（`hitKey`：文件 + 行 + 列）。整个换新的 Set 才触发更新 */
  unchecked = $state<Set<string>>(new Set());
  /** 折叠起来的文件（相对路径） */
  collapsed = $state<Set<string>>(new Set());
  scan = $state<ReplaceScan | null>(null);
  /** `scan.files[i].hits[j]` 一一对应的「改后」那一行 */
  after = $state<{ text: string; spans: [number, number][] }[][]>([]);
  scanning = $state(false);
  applying = $state(false);
  /** 扫描 / 执行没做成的那句话（正则写错了、上次中断没处理…） */
  error = $state<string | null>(null);
  /** 改前全文太大、撤销不了：浮层里问一句「仍然替换？」，`bytes` 是多大 */
  askNoUndo = $state<number | null>(null);

  /**
   * 刚做完的那次，卡片上「已替换 3 个文件 12 处 · 撤销」。`root` 是哪个项目的：
   * 替换日志整个应用一份，在别的窗口里也看得见 —— 卡片只在那个项目的窗口里出（Q5 的补充）
   */
  done = $state<{ root: string; files: number; hits: number } | null>(null);
  /** 启动时查到上次中断（提交到一半崩了）：卡片上「退回 / 保留现状」 */
  interrupted = $state<ReplacePending | null>(null);
}

export const replace = new ReplaceState();
