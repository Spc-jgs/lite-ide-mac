/**
 * 监视（IDEA 调试窗里的 Watches；docs/TAKEOVER.md G14）：按项目存的几个表达式，每停一次在当前帧上全算一遍。
 * 存进 localStorage、跨重启留着（同断点：调一个问题常常要好几天）。去掉两头空白、去重、空的不要，最多 30 个 ——
 * 每停一次每个都要在目标 JVM 里算一遍，几十个就是单步一下等好几秒
 */
const KEY = "lite-ide.watches";
export const WATCH_MAX = 30;

function table(): Record<string, string[]> {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return v && typeof v === "object" ? (v as Record<string, string[]>) : {};
  } catch {
    return {};
  }
}

export function clean(list: unknown): string[] {
  if (!Array.isArray(list)) return [];
  const out: string[] = [];
  for (const x of list) {
    const t = typeof x === "string" ? x.trim() : "";
    if (t && !out.includes(t)) out.push(t);
  }
  return out.slice(0, WATCH_MAX);
}

export function watchesOf(root: string): string[] {
  return clean(table()[root]);
}

export function setWatches(root: string, list: string[]): string[] {
  const t = table();
  const next = clean(list);
  if (next.length) t[root] = next;
  else delete t[root];
  try {
    localStorage.setItem(KEY, JSON.stringify(t));
  } catch {
    /* 写不下就只在这次运行里有 */
  }
  return next;
}
