/**
 * 多久之前。粗一点没关系：注解和草稿列表看的都是「大概什么时候」，
 * 精确时间在 tooltip 里。
 *
 * 从 `editor/blame.ts` 搬出来（issue #40）：草稿列表也要它，而 blame.ts
 * 引着 CM6 —— 为一个 12 行的函数把编辑器拽进侧边栏的 chunk 不值。
 */
export function ago(unix: number, now = Date.now() / 1000): string {
  const s = Math.max(0, now - unix);
  if (s < 60) return "刚刚";
  const m = s / 60;
  if (m < 60) return `${Math.floor(m)} 分钟前`;
  const h = m / 60;
  if (h < 24) return `${Math.floor(h)} 小时前`;
  const d = h / 24;
  if (d < 30) return `${Math.floor(d)} 天前`;
  const mo = d / 30;
  if (mo < 12) return `${Math.floor(mo)} 个月前`;
  return `${Math.floor(d / 365)} 年前`;
}
