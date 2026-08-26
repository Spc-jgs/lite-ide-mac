/**
 * 日志行分段解析。
 *
 * 目标形态（Java 应用最常见的输出）：
 *   2026-08-24 14:03:21.442 INFO  [http-nio-exec-4] c.l.OrderService - 处理完成
 *   └── 时间戳 ──────────┘ └级别┘ └── 线程 ──────┘ └── logger ──┘   └ 消息 ┘
 *
 * 认不出来就整行当消息 —— 日志格式千变万化，宁可降级也不能猜错着色。
 * 只对可见的几十行调用，成本可忽略；索引阶段的级别探测是另一套（Rust 侧）。
 */

export type Level = "error" | "warn" | "info" | "debug" | "trace" | null;

export interface Segments {
  lvl: Level;
  /** 是否是异常堆栈的续行（\tat ... / Caused by: ...） */
  stack: boolean;
  ts: string;
  level: string;
  thread: string;
  logger: string;
  msg: string;
}

const LEVELS: [string, Level][] = [
  ["ERROR", "error"],
  ["FATAL", "error"],
  ["SEVERE", "error"],
  ["WARN", "warn"],
  ["INFO", "info"],
  ["DEBUG", "debug"],
  ["TRACE", "trace"],
];

/** 只在行首这么多字符里找级别 —— 与 Rust 侧 SCAN_HEAD 保持一致的语义 */
const HEAD = 64;

const bare = (msg: string, stack = false): Segments => ({
  lvl: null,
  stack,
  ts: "",
  level: "",
  thread: "",
  logger: "",
  msg,
});

export function parse(line: string): Segments {
  // 堆栈续行：制表符或空格缩进后接 at / Caused by / ... N more
  if (line.startsWith("\tat ") || line.startsWith("    at ") || line.startsWith("  at ")) {
    return bare(line, true);
  }
  if (line.startsWith("Caused by:") || line.startsWith("\t... ") || line.startsWith("\tSuppressed:")) {
    return bare(line, true);
  }

  const head = line.length > HEAD ? line.slice(0, HEAD) : line;
  let at = -1;
  let word = "";
  let lvl: Level = null;
  for (const [kw, l] of LEVELS) {
    const i = head.indexOf(kw);
    if (i >= 0 && (at < 0 || i < at)) {
      at = i;
      word = kw;
      lvl = l;
    }
  }
  if (at < 0) {
    // 没有级别标记 —— 未捕获异常的首行通常长这样
    return bare(line, /^[\w.]+(Exception|Error)\b/.test(line));
  }

  const ts = line.slice(0, at).trimEnd();
  let rest = line.slice(at + word.length);

  // 线程名：紧跟其后的 [...]
  let thread = "";
  const lb = rest.indexOf("[");
  if (lb >= 0 && lb <= 3) {
    const rb = rest.indexOf("]", lb);
    if (rb > lb) {
      thread = rest.slice(lb, rb + 1);
      rest = rest.slice(rb + 1);
    }
  }

  // logger 与消息以 " - " 或 " : " 分隔
  let logger = "";
  let msg = rest.trimStart();
  const sep = rest.search(/\s[-:]\s/);
  if (sep >= 0) {
    logger = rest.slice(0, sep).trim();
    msg = rest.slice(sep + 3);
  }

  return { lvl, stack: false, ts, level: word, thread, logger, msg };
}

/** 把一段文本按关键字切成 [普通, 命中, 普通, ...]，用于搜索高亮 */
export function highlight(text: string, needle: string, caseSensitive: boolean): string[] {
  if (!needle) return [text];
  const hay = caseSensitive ? text : text.toLowerCase();
  const pat = caseSensitive ? needle : needle.toLowerCase();
  const out: string[] = [];
  let pos = 0;
  for (;;) {
    const i = hay.indexOf(pat, pos);
    if (i < 0) break;
    out.push(text.slice(pos, i), text.slice(i, i + pat.length));
    pos = i + pat.length;
  }
  out.push(text.slice(pos));
  return out;
}
