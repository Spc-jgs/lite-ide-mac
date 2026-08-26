/**
 * 极轻量的日志级别探测。
 *
 * M0 只对可见的几十行调用，用 indexOf 而非正则 —— 每帧成本可忽略，
 * 不干扰性能测量。完整的着色规则（时间戳、线程名、类名、堆栈）留给 M1。
 */
export type Level = "error" | "warn" | "info" | "debug" | null;

export function levelOf(line: string): Level {
  // 只看行首 64 字符：级别标记不会出现在正文深处
  const head = line.length > 64 ? line.slice(0, 64) : line;
  if (head.includes("ERROR") || head.includes("FATAL") || head.includes("SEVERE")) return "error";
  if (head.includes("WARN")) return "warn";
  if (head.includes("INFO")) return "info";
  if (head.includes("DEBUG") || head.includes("TRACE")) return "debug";
  return null;
}
