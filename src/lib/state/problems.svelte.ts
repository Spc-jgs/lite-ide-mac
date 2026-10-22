/**
 * 问题工具窗（⌘6，docs/TAKEOVER.md G4）的「实时」那一半：语言服务器报给每个开着的文件的诊断。
 *
 * 为什么只有开着的文件：jdtls 的自动构建 10-20 关掉了（它和 Maven 抢着写 `target/classes`，坏 class 骗过了构建，JOURNAL 那天），
 * 关了之后它只对 didOpen 过的文件做实时检查。全项目的编译错误来自 ⌘F9 那次 Maven 构建（`runs` 里那一格的 `problems`）——
 * 问题工具窗两样一起列，各自说清是哪来的。
 *
 * `java-lsp.ts`（懒的）收到 `java-diagnostics` 时写进来，问题工具窗（懒的）读；没有别人 import 它，不进入口包
 */
import type { JavaDiagnostic } from "../ipc/commands";

export interface FileProblems {
  root: string;
  path: string;
  /** 真条数；超过上限时 `items` 只有前一截（Rust 那边先错误后警告地截） */
  total: number;
  items: JavaDiagnostic[];
}

class ProblemsState {
  /** 路径 → 这个文件此刻的诊断。没有问题的文件不在里面 */
  byPath = $state<Record<string, FileProblems>>({});

  set(d: FileProblems) {
    // 只收错误、警告（3 信息 4 提示不进问题窗，同 IDEA 的默认）
    const items = d.items.filter((x) => x.severity <= 2);
    if (items.length) this.byPath[d.path] = { ...d, items, total: items.length < d.items.length ? items.length : d.total };
    else delete this.byPath[d.path];
  }

  /** 文件关了（didClose）：它的诊断不再更新，留着会骗人 */
  drop(path: string) {
    delete this.byPath[path];
  }
}

export const problems = new ProblemsState();
