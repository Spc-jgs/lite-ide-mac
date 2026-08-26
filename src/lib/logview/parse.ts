/**
 * 日志行分段解析 —— 多格式。
 *
 * 现实里的日志五花八门，只认一种格式等于对其余全部降级成纯文本。
 * 这里对常见的八类做结构化拆分，认不出来的按 plain 处理（仍有级别着色）。
 *
 * 解析结果是**通用的分段列表**而不是固定字段：每种格式吐自己的段，
 * 渲染层只按 `cls` 上色。加一种新格式不必动渲染代码。
 *
 * 只对可见的几十行调用，成本可忽略。索引阶段的级别探测是另一套（Rust 侧）。
 */

export type Level = "error" | "warn" | "info" | "debug" | "trace" | null;

/** 分段的语义类别，决定着色 */
export type PartClass =
  | "ts" // 时间戳
  | "level" // 级别
  | "thread" // 线程 / 进程
  | "logger" // logger 名 / 模块 / 来源
  | "key" // 结构化日志的键
  | "meta" // IP、方法、状态码之类的附属信息
  | "msg" // 正文
  | "dim"; // 分隔符等次要内容

export interface Part {
  text: string;
  cls: PartClass;
}

export interface Segments {
  lvl: Level;
  /** 异常堆栈的续行 */
  stack: boolean;
  parts: Part[];
}

export type LogFormat =
  | "java" // 2026-08-24 14:03:21.442 INFO  [thread] c.l.Svc - msg
  | "python" // 2026-08-24 14:03:21,442 - module - INFO - msg
  | "rust" // [2026-08-24T14:03:21Z INFO module] msg
  | "logfmt" // time="..." level=info msg="..."
  | "json" // {"time":"...","level":"error","msg":"..."}
  | "nginx" // 1.2.3.4 - - [24/Aug/2026:14:03:21 +0800] "GET /x HTTP/1.1" 200 1234
  | "syslog" // Aug 24 14:03:21 host proc[123]: msg
  | "iso" // 2026-08-24T14:03:21.442Z ERROR msg
  | "plain";

export const FORMAT_LABEL: Record<LogFormat, string> = {
  java: "Java / Logback",
  python: "Python logging",
  rust: "Rust env_logger",
  logfmt: "logfmt",
  json: "JSON 结构化",
  nginx: "Nginx / Apache",
  syslog: "syslog",
  iso: "ISO 时间戳",
  plain: "纯文本",
};

// ─────────────────────────── 级别 ───────────────────────────

const LEVEL_WORDS: [string, Level][] = [
  ["ERROR", "error"], ["FATAL", "error"], ["SEVERE", "error"], ["CRITICAL", "error"],
  ["WARNING", "warn"], ["WARN", "warn"],
  ["INFO", "info"], ["NOTICE", "info"],
  ["DEBUG", "debug"],
  ["TRACE", "trace"], ["VERBOSE", "trace"],
];

/** 把一个词规范成级别；认不出返回 null */
export function levelOfWord(word: string): Level {
  const up = word.toUpperCase();
  for (const [w, l] of LEVEL_WORDS) {
    if (up === w) return l;
  }
  return null;
}

/** 在一段文本里找级别关键字，返回 [级别, 起点, 长度] */
function findLevel(head: string): [Level, number, number] | null {
  let best: [Level, number, number] | null = null;
  for (const [w, l] of LEVEL_WORDS) {
    const i = head.indexOf(w);
    if (i >= 0 && (!best || i < best[1])) best = [l, i, w.length];
  }
  return best;
}

// ─────────────────────────── 格式探测 ───────────────────────────

const RE = {
  javaTs: /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}[.,]\d{1,9}/,
  isoTs: /^\d{4}-\d{2}-\d{2}T[\d:.]+(?:Z|[+\-]\d{2}:?\d{2})?/,
  pythonSep: /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3} +- /,
  rust: /^\[\d{4}-\d{2}-\d{2}T[\d:.]+Z? +[A-Z]+ /,
  nginx: /^\S+ \S+ \S+ \[[^\]]+\] "/,
  syslog: /^[A-Z][a-z]{2} +\d{1,2} \d{2}:\d{2}:\d{2} \S+ /,
  logfmt: /(^|\s)(level|lvl)=("?)[a-zA-Z]+\3/,
};

function formatOfLine(line: string): LogFormat | null {
  if (!line) return null;
  if (line[0] === "{" && line.includes('"') && (line.includes('level') || line.includes('lvl') || line.includes('severity'))) {
    return "json";
  }
  if (RE.rust.test(line)) return "rust";
  if (RE.nginx.test(line)) return "nginx";
  if (RE.pythonSep.test(line)) return "python";
  if (RE.javaTs.test(line)) return "java";
  if (RE.syslog.test(line)) return "syslog";
  if (RE.logfmt.test(line)) return "logfmt";
  if (RE.isoTs.test(line)) return "iso";
  return null;
}

/**
 * 从样本行投票选出格式。
 *
 * 投票而不是看第一行：日志开头常有启动横幅、空行、乱七八糟的东西，
 * 只看一行很容易被带偏。
 */
export function detectFormat(lines: string[]): LogFormat {
  const votes = new Map<LogFormat, number>();
  let counted = 0;
  for (const line of lines) {
    const f = formatOfLine(line.trim());
    if (!f) continue;
    votes.set(f, (votes.get(f) ?? 0) + 1);
    if (++counted >= 40) break;
  }
  let best: LogFormat = "plain";
  let top = 0;
  for (const [f, n] of votes) {
    if (n > top) {
      top = n;
      best = f;
    }
  }
  return best;
}

// ─────────────────────────── 各格式解析 ───────────────────────────

const STACK_PREFIXES = ["\tat ", "    at ", "  at ", "\t... ", "\tSuppressed:", "Caused by:"];

function isStackLine(line: string): boolean {
  for (const p of STACK_PREFIXES) {
    if (line.startsWith(p)) return true;
  }
  // Java 未捕获异常的首行：com.foo.BarException: message
  return /^[\w.$]+(Exception|Error|Throwable)\b/.test(line);
}

const plain = (line: string, lvl: Level = null, stack = false): Segments => ({
  lvl,
  stack,
  parts: [{ text: line, cls: "msg" }],
});

/**
 * 时间戳之后的公共结构：`级别 [线程] logger - 正文`。
 *
 * Java/Logback 与 ISO 时间戳的日志只有时间戳写法不同，后半截一模一样，
 * 共用一份解析——否则同一种结构要维护两遍，迟早分叉。
 */
function parseTimestampedRest(rest: string, parts: Part[]): Level {
  const lv = findLevel(rest.slice(0, 24));
  let lvl: Level = null;
  if (lv) {
    const [l, at, len] = lv;
    lvl = l;
    if (at > 0) parts.push({ text: rest.slice(0, at), cls: "dim" });
    parts.push({ text: rest.slice(at, at + len), cls: "level" });
    rest = rest.slice(at + len);
  }

  // 线程 [name]
  const lb = rest.indexOf("[");
  if (lb >= 0 && lb <= 3) {
    const rb = rest.indexOf("]", lb);
    if (rb > lb) {
      if (lb > 0) parts.push({ text: rest.slice(0, lb), cls: "dim" });
      parts.push({ text: rest.slice(lb, rb + 1), cls: "thread" });
      rest = rest.slice(rb + 1);
    }
  }

  // logger - message
  const sep = rest.search(/\s[-:]\s/);
  if (sep >= 0) {
    parts.push({ text: rest.slice(0, sep), cls: "logger" });
    parts.push({ text: rest.slice(sep, sep + 3), cls: "dim" });
    parts.push({ text: rest.slice(sep + 3), cls: "msg" });
  } else {
    parts.push({ text: rest, cls: "msg" });
  }
  return lvl;
}

/** Java / Logback / Log4j：空格分隔的时间戳 */
function parseJava(line: string): Segments {
  const m = RE.javaTs.exec(line);
  if (!m) return parseGeneric(line);
  const parts: Part[] = [{ text: m[0], cls: "ts" }];
  const lvl = parseTimestampedRest(line.slice(m[0].length), parts);
  return { lvl, stack: false, parts };
}

/** Python logging：2026-08-24 14:03:21,442 - module - INFO - msg */
function parsePython(line: string): Segments {
  const m = /^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3}) +- +(.*)$/.exec(line);
  if (!m) return parseGeneric(line);
  const parts: Part[] = [{ text: m[1], cls: "ts" }, { text: " - ", cls: "dim" }];
  const fields = m[2].split(" - ");
  let lvl: Level = null;
  fields.forEach((f, i) => {
    const asLevel = levelOfWord(f.trim());
    if (asLevel && lvl === null) {
      lvl = asLevel;
      parts.push({ text: f, cls: "level" });
    } else if (i === fields.length - 1) {
      parts.push({ text: f, cls: "msg" });
    } else {
      parts.push({ text: f, cls: "logger" });
    }
    if (i < fields.length - 1) parts.push({ text: " - ", cls: "dim" });
  });
  return { lvl, stack: false, parts };
}

/** Rust env_logger：[2026-08-24T14:03:21Z INFO module] msg */
function parseRust(line: string): Segments {
  const m = /^\[([\d\-T:.]+Z?) +([A-Z]+)(?: +([^\]]+))?\] ?(.*)$/.exec(line);
  if (!m) return parseGeneric(line);
  const parts: Part[] = [
    { text: "[", cls: "dim" },
    { text: m[1], cls: "ts" },
    { text: " ", cls: "dim" },
    { text: m[2], cls: "level" },
  ];
  if (m[3]) {
    parts.push({ text: " ", cls: "dim" }, { text: m[3], cls: "logger" });
  }
  parts.push({ text: "] ", cls: "dim" }, { text: m[4], cls: "msg" });
  return { lvl: levelOfWord(m[2]), stack: false, parts };
}

/** logfmt：time="..." level=info msg="..." */
function parseLogfmt(line: string): Segments {
  const parts: Part[] = [];
  let lvl: Level = null;
  // key=value，value 可能带引号
  const re = /([\w.\-]+)=("(?:[^"\\]|\\.)*"|\S*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(line)) !== null) {
    if (m.index > last) parts.push({ text: line.slice(last, m.index), cls: "dim" });
    const key = m[1];
    const raw = m[2];
    const val = raw.startsWith('"') ? raw.slice(1, -1) : raw;
    const keyLower = key.toLowerCase();

    if (keyLower === "level" || keyLower === "lvl" || keyLower === "severity") {
      lvl = levelOfWord(val);
      parts.push({ text: key, cls: "key" }, { text: "=", cls: "dim" }, { text: raw, cls: "level" });
    } else if (keyLower === "time" || keyLower === "ts" || keyLower === "timestamp") {
      parts.push({ text: key, cls: "key" }, { text: "=", cls: "dim" }, { text: raw, cls: "ts" });
    } else if (keyLower === "msg" || keyLower === "message") {
      parts.push({ text: key, cls: "key" }, { text: "=", cls: "dim" }, { text: raw, cls: "msg" });
    } else {
      parts.push({ text: key, cls: "key" }, { text: "=", cls: "dim" }, { text: raw, cls: "meta" });
    }
    last = m.index + m[0].length;
  }
  if (last < line.length) parts.push({ text: line.slice(last), cls: "msg" });
  if (parts.length === 0) return parseGeneric(line);
  return { lvl, stack: false, parts };
}

const JSON_TS_KEYS = ["time", "ts", "timestamp", "@timestamp", "date", "eventTime"];
const JSON_LEVEL_KEYS = ["level", "lvl", "severity", "levelname", "log.level"];
const JSON_MSG_KEYS = ["msg", "message", "text", "event"];

/** JSON 结构化日志 */
function parseJson(line: string): Segments {
  let obj: Record<string, unknown>;
  try {
    obj = JSON.parse(line) as Record<string, unknown>;
  } catch {
    // 不是完整的一行 JSON（可能被截断），退回通用处理
    return parseGeneric(line);
  }
  if (typeof obj !== "object" || obj === null) return parseGeneric(line);

  const pick = (keys: string[]): [string, unknown] | null => {
    for (const k of keys) {
      if (k in obj) return [k, obj[k]];
    }
    return null;
  };
  const str = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v));

  const parts: Part[] = [];
  const ts = pick(JSON_TS_KEYS);
  const lv = pick(JSON_LEVEL_KEYS);
  const msg = pick(JSON_MSG_KEYS);
  const shown = new Set<string>();

  if (ts) {
    parts.push({ text: str(ts[1]), cls: "ts" });
    shown.add(ts[0]);
  }
  let lvl: Level = null;
  if (lv) {
    lvl = levelOfWord(str(lv[1]));
    parts.push({ text: " " + str(lv[1]).toUpperCase(), cls: "level" });
    shown.add(lv[0]);
  }
  if (msg) {
    parts.push({ text: "  " + str(msg[1]), cls: "msg" });
    shown.add(msg[0]);
  }
  // 其余字段按 key=value 附在后面，别把结构化信息丢了
  for (const [k, v] of Object.entries(obj)) {
    if (shown.has(k)) continue;
    parts.push({ text: "  " + k, cls: "key" }, { text: "=", cls: "dim" }, { text: str(v), cls: "meta" });
  }
  return { lvl, stack: false, parts };
}

/** Nginx / Apache combined access log */
function parseNginx(line: string): Segments {
  const m = /^(\S+) (\S+) (\S+) \[([^\]]+)\] "([^"]*)" (\d{3}) (\S+)(.*)$/.exec(line);
  if (!m) return parseGeneric(line);
  const status = Number(m[6]);
  // access log 没有级别字段，用状态码推：5xx 当 error，4xx 当 warn
  const lvl: Level = status >= 500 ? "error" : status >= 400 ? "warn" : "info";
  const req = m[5].split(" ");
  return {
    lvl,
    stack: false,
    parts: [
      { text: m[1], cls: "meta" },
      { text: ` ${m[2]} ${m[3]} `, cls: "dim" },
      { text: `[${m[4]}]`, cls: "ts" },
      { text: " ", cls: "dim" },
      { text: req[0] ?? "", cls: "level" },
      { text: " " + (req.slice(1).join(" ") || ""), cls: "msg" },
      { text: " " + m[6], cls: status >= 400 ? "level" : "meta" },
      { text: " " + m[7] + m[8], cls: "dim" },
    ],
  };
}

/** syslog：Aug 24 14:03:21 host proc[123]: msg */
function parseSyslog(line: string): Segments {
  const m = /^([A-Z][a-z]{2} +\d{1,2} \d{2}:\d{2}:\d{2}) (\S+) ([^:]+): ?(.*)$/.exec(line);
  if (!m) return parseGeneric(line);
  const lv = findLevel(m[4].slice(0, 32));
  return {
    lvl: lv ? lv[0] : null,
    stack: false,
    parts: [
      { text: m[1], cls: "ts" },
      { text: " " + m[2], cls: "meta" },
      { text: " " + m[3], cls: "logger" },
      { text: ": ", cls: "dim" },
      { text: m[4], cls: "msg" },
    ],
  };
}

/** ISO 时间戳打头（T 分隔），后半截与 Java 同构 */
function parseIso(line: string): Segments {
  const m = RE.isoTs.exec(line);
  if (!m) return parseGeneric(line);
  const parts: Part[] = [{ text: m[0], cls: "ts" }];
  const lvl = parseTimestampedRest(line.slice(m[0].length), parts);
  return { lvl, stack: false, parts };
}

/** 认不出结构时的兜底：只找级别，其余整行当正文 */
function parseGeneric(line: string): Segments {
  const lv = findLevel(line.slice(0, 64));
  if (!lv) return plain(line);
  const [l, at, len] = lv;
  return {
    lvl: l,
    stack: false,
    parts: [
      { text: line.slice(0, at), cls: "dim" },
      { text: line.slice(at, at + len), cls: "level" },
      { text: line.slice(at + len), cls: "msg" },
    ],
  };
}

const PARSERS: Record<LogFormat, (line: string) => Segments> = {
  java: parseJava,
  python: parsePython,
  rust: parseRust,
  logfmt: parseLogfmt,
  json: parseJson,
  nginx: parseNginx,
  syslog: parseSyslog,
  iso: parseIso,
  plain: parseGeneric,
};

/** 按指定格式解析一行 */
export function parse(line: string, fmt: LogFormat = "java"): Segments {
  if (line.length === 0) return plain(line);
  // 堆栈续行不属于任何格式，先拦下来
  if (isStackLine(line)) return plain(line, null, true);
  return PARSERS[fmt](line);
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
