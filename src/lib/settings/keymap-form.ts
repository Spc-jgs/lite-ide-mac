/**
 * 设置对话框「键位」那一页里不碰界面的那一半（docs/TAKEOVER.md R3）：改了还没应用的怎么合进覆盖表、哪些行能改、录到的键收不收。
 * 拆出来在裸 node 里测（tests/keymap-form.test.ts）—— 界面在 `KeymapPane.svelte`。
 */
import { KEYS, RESERVED, accelOf, editable, splitAccel, type KeyDef } from "../state/keymap";

/** 改了还没应用的：`{ 动作 id: 键位 }`，`""` = 不要键位，`null` = 回到默认 */
export type KeyEdits = Record<string, string | null>;

const defOf = new Map(KEYS.map((k) => [k.id, k]));

/**
 * 应用之后的覆盖表：在现有的上面叠这次改的。和默认一样的不存（`keymap.json` 只存改过的 —— 改回默认等于删掉那一条）
 */
export function staged(over: Readonly<Record<string, string>>, edits: KeyEdits): Record<string, string> {
  const out: Record<string, string> = { ...over };
  for (const [id, a] of Object.entries(edits)) {
    const d = defOf.get(id);
    if (a === null || !d || a === (d.accel ?? "")) delete out[id];
    else out[id] = a;
  }
  return out;
}

/** 一行为什么不能改（能改是 null）。说清理由：灰着又不说为什么，人会以为是坏了 */
export function lockedWhy(d: KeyDef): string | null {
  if (editable(d)) return null;
  if (d.gesture) return "这是手势，不是一个键";
  if (d.owner === "xterm") return "终端自己的键";
  if (d.owner === "key") return "这个键有特殊处理（终端里让给 shell、按住 ⌃ 连按切标签），不能改";
  return "鼠标手势，不是一个键";
}

/** 录到的一下收不收：收就给键位，不收给一句为什么 */
export function judge(accel: string): { accel: string } | { why: string } {
  if (!splitAccel(accel)) return { why: `${accel} 当不了快捷键：要带 ⌘ ⌥ ⌃ 之一（F1–F20 可以不带）` };
  const sys = RESERVED[accel];
  if (sys) return { why: `${accel} 是系统的「${sys}」，改到它上面会被系统吃掉、或者把${sys}弄没` };
  return { accel };
}

/**
 * 搜：名字、id、键位里有这几个字（不分大小写）。`exact` 是「按键找」录到的键位：只要键位正好是它的（IDEA 的 Find Shortcut）
 */
export function matches(d: KeyDef, over: Readonly<Record<string, string>>, q: string, exact: string | null): boolean {
  const a = accelOf(d, over) ?? d.gesture ?? "";
  if (exact) return a === exact || d.alias === exact;
  const s = q.trim().toLowerCase();
  return !s || d.label.toLowerCase().includes(s) || d.id.includes(s) || a.toLowerCase().includes(s);
}
