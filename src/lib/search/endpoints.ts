/**
 * 随处搜索里的「接口」（docs/IDEA.md 第 25 节，IDEA Ultimate 的 Endpoints）：怎么排。拆出来是为了在裸 node 里测（tests/endpoints.test.ts）。
 *
 * 两种输入：
 * - **一个真 URL**（日志里复制的 `/api/orders/42?x=1`、`http://localhost:8080/api/orders/42`）：去掉协议、主机、查询串，
 *   按模板对 —— `{id}` 对任意一段。整个对上的排前面，只对上开头的（还没打完）其次。同 IDEA：粘一个日志里的地址就能找到是谁处理的
 * - **别的**：模糊匹配「路径 + 控制器.方法名」（`orders get`、`OrderController`）
 */
import type { SpringEndpoint } from "../ipc/commands";
import { rank, segments } from "./fuzzy";

export interface EndpointHit {
  item: SpringEndpoint;
  /** 路径那一段的高亮 */
  seg: { t: string; hit: boolean }[];
}

/** 像不像一个地址：`/` 开头，或者带协议 */
export const looksLikeUrl = (q: string) => /^\s*(https?:\/\/|\/)/i.test(q);

/** `http://host:8080/api/orders/42?x=1#a` → `/api/orders/42`；末尾的 `/` 去掉（根除外） */
export function urlPath(q: string): string {
  let s = q.trim().replace(/^https?:\/\/[^/]*/i, "");
  s = s.replace(/[?#].*$/, "");
  if (!s.startsWith("/")) s = `/${s}`;
  return s.length > 1 ? s.replace(/\/+$/, "") : s;
}

/** 模板的每一段：`{名字}` 对任意一段（不含 `/`），别的逐字对（不分大小写：大小写不同的 URL 在 Spring 里默认也对不上，但打字时不该因为这个找不到） */
function segs(p: string) {
  return p.split("/").filter(Boolean);
}
const any = (t: string) => /^\{[^}]*\}$/.test(t);

/** 0 = 整个对上，1 = 对上开头（还没打完），-1 = 对不上 */
export function urlMatch(template: string, path: string): number {
  const t = segs(template);
  const u = segs(path);
  if (u.length > t.length) return -1;
  for (let i = 0; i < u.length; i++) {
    const last = i === u.length - 1;
    if (any(t[i])) continue;
    const a = t[i].toLowerCase();
    const b = u[i].toLowerCase();
    // 最后一段可能还没打完：对上开头就行
    if (last ? !a.startsWith(b) : a !== b) return -1;
  }
  return u.length === t.length && (u.length === 0 || any(t[u.length - 1]) || t[u.length - 1].toLowerCase() === u[u.length - 1].toLowerCase()) ? 0 : 1;
}

export function rankEndpoints(list: SpringEndpoint[], q: string, limit: number): EndpointHit[] {
  if (!q.trim()) return list.slice(0, limit).map((item) => ({ item, seg: [{ t: item.path, hit: false }] }));
  if (looksLikeUrl(q)) {
    const path = urlPath(q);
    const scored = list.map((item) => ({ item, s: urlMatch(item.path, path) })).filter((x) => x.s >= 0);
    // 整个对上的在前；同一档里变量少的在前、再短的在前 —— 同 Spring 自己挑处理者的规矩（越具体越优先）：
    // `/api/orders/search` 两个都对得上时，处理它的是写死 search 的那个，不是 `{id}`（测试原来按长短排，`{id}` 短，排反了）
    const vars = (p: string) => (p.match(/\{[^}]*\}/g) ?? []).length;
    scored.sort((a, b) => a.s - b.s || vars(a.item.path) - vars(b.item.path) || a.item.path.length - b.item.path.length);
    return scored.slice(0, limit).map(({ item }) => ({ item, seg: [{ t: item.path, hit: true }] }));
  }
  return rank(list, q, (e) => `${e.path} ${e.controller}.${e.method}`, limit).map((r) => ({
    item: r.item,
    // 高亮只画在路径那一段上（控制器.方法名另一列）
    seg: segments(r.item.path, r.positions.filter((p) => p < r.item.path.length)),
  }));
}
