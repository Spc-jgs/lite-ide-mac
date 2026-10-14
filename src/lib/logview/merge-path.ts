/**
 * 合并视图（docs/MERGE.md）前端用到的几样纯函数：标签的「路径」怎么编、图例上每份叫什么、偏移怎么写和读、
 * 「只看哪几份」怎么点。都是纯的，`tests/merge-path.test.ts` 在裸 node 里测。
 */

/**
 * 合并标签的 `path`。标签表、会话快照、「上次读到哪」都按 path 认标签 —— 合并标签也要有一个，而且同样几份文件
 * 合出来是同一个（再合一次就切过去，不开第二个）。用换行分隔：路径里几乎不会有换行（真有就不让合，`mergePath` 返回 null），
 * 而状态栏 / 面包屑的 tooltip 里换行正好一行一份。
 *
 * `tabflow.openPath` 认 `merge:` 开头（那边写的是字面量，不 import 这里：这个文件不该进入口包）
 */
export const MERGE_PREFIX = "merge:";

export function mergePath(paths: string[]): string | null {
  if (paths.some((p) => p.includes("\n"))) return null;
  return `${MERGE_PREFIX}\n${paths.join("\n")}`;
}

export function mergePaths(path: string): string[] | null {
  if (!path.startsWith(`${MERGE_PREFIX}\n`)) return null;
  const out = path.slice(MERGE_PREFIX.length + 1).split("\n").filter(Boolean);
  return out.length >= 2 ? out : null;
}

/** 去掉日志后缀：`order-service.log` → `order-service`、`app.log.1` → `app`、`gc.out` → `gc` */
function stem(name: string): string {
  const m = /^(.*?)\.(?:log|out|txt|json|jsonl)(?:\.\d+)?$/i.exec(name);
  return m && m[1] ? m[1] : name;
}

/**
 * 图例和每行行首那一格写的短名。**同名的带上父目录**：最常见的合并恰恰是同一个服务的几台机器
 * （`10.0.0.1/app.log`、`10.0.0.2/app.log`），只写 `app` 三格一模一样，等于没写
 */
export function shortNames(paths: string[]): string[] {
  const parts = paths.map((p) => p.split("/").filter(Boolean));
  const base = parts.map((ps) => stem(ps[ps.length - 1] ?? ""));
  return base.map((b, i) => {
    if (base.filter((x) => x === b).length < 2) return b;
    const parent = parts[i][parts[i].length - 2];
    return parent ? `${parent}/${b}` : b;
  });
}

/** 标签上的名字：`gateway + order + 另 2 份` */
export function mergeTitle(names: string[]): string {
  if (names.length <= 3) return names.join(" + ");
  return `${names.slice(0, 2).join(" + ")} + 另 ${names.length - 2} 份`;
}

const UNIT: Record<string, number> = { h: 3_600_000, m: 60_000, s: 1000, ms: 1 };

/**
 * 读人写的偏移：`+8h`、`-350ms`、`+1m30s`、`2s`、`0`。不带正负号就是正的。读不懂返回 null（输入框变红，不猜）
 */
export function parseOffset(text: string): number | null {
  const t = text.trim().replace(/\s+/g, "").replace(/^−/, "-");
  if (t === "0" || t === "") return 0;
  const m = /^([+-]?)((?:\d+(?:\.\d+)?(?:ms|h|m|s))+)$/.exec(t);
  if (!m) return null;
  let total = 0;
  for (const part of m[2].matchAll(/(\d+(?:\.\d+)?)(ms|h|m|s)/g)) total += Number(part[1]) * UNIT[part[2]];
  return Math.round(m[1] === "-" ? -total : total);
}

/** 偏移写回人话：`+8h`、`−350ms`、`+1m30s`；0 是空串（图例上不印） */
export function fmtOffset(ms: number): string {
  if (ms === 0) return "";
  let left = Math.abs(ms);
  let out = "";
  for (const [u, n] of [["h", 3_600_000], ["m", 60_000], ["s", 1000]] as const) {
    if (left >= n) {
      out += `${Math.floor(left / n)}${u}`;
      left %= n;
    }
  }
  if (left > 0) out += `${left}ms`;
  // 负号用 U+2212：和 + 一样宽，图例上一排对得齐
  return `${ms < 0 ? "−" : "+"}${out}`;
}

/** n 份都看的掩码 */
export const allSources = (n: number) => (n >= 32 ? 0xffff_ffff : (1 << n) - 1) >>> 0;

/**
 * 点图例上的一份。和级别胶囊同一套手感（FilterBar 的 `toggle`）：全亮时点一份 = 只看它；只剩它亮着时再点 = 复原；
 * 其余情况翻转这一份。「只看这一份」是合并之后最常要的（先看网关、再看订单服务）
 */
export function toggleSource(mask: number, i: number, n: number): number {
  const all = allSources(n);
  const bit = (1 << i) >>> 0;
  if (mask === bit) return all;
  if (mask === all) return bit;
  const next = (mask ^ bit) >>> 0;
  return next === 0 ? all : next;
}

/** 时间戳精度那句话：借了什么。全带日期的不说 */
export function clockNote(clock: string, fillDate: string | null): string {
  if (clock === "timeOnly") return fillDate ? `只有时分秒，按 ${fillDate} 对齐` : "只有时分秒";
  if (clock === "noYear") return fillDate ? `没有年份，按 ${fillDate.slice(0, 4)} 年对齐` : "没有年份";
  if (clock === "none") return "开头 2000 行没认出时间戳，整份排在最前";
  return "";
}
