/**
 * 设置对话框的纯逻辑（docs/SETTINGS.md 第 13 节）：哪一项归哪一类、搜索、改了还没应用的那些怎么显示、「应用」时交给 Rust 哪一批。
 * 不碰 Svelte、不碰 IPC，`tests/settings-form.test.ts` 在裸 node 里测
 */
import type { SettingDef, SettingsPage } from "../ipc/commands";

export type Cat = "appearance" | "terminal" | "java" | "maven" | "system" | "project" | "keymap";

/** 左边那一列。「此项目」只在打开了项目时出现（调用方过滤） */
export const CATS: { id: Cat; label: string }[] = [
  { id: "appearance", label: "外观" },
  // 照 IDEA 的设置树：外观下面就是键位（docs/TAKEOVER.md R3）。不是 settings.json 里的项，单独一页（KeymapPane）
  { id: "keymap", label: "键位" },
  { id: "terminal", label: "终端" },
  { id: "java", label: "Java 与 JDK" },
  { id: "maven", label: "Maven" },
  // IDEA 的 Appearance & Behavior › System Settings › Updates：放最后、在「此项目」前（它是全局的）
  { id: "system", label: "更新" },
  { id: "project", label: "此项目" },
];

/** 一个键归哪一类：按键名的前缀（`editor.` `terminal.` `java.` `maven.` `update.`），和 settings.json 里的写法一个分法 */
export function catOf(key: string): Cat {
  if (key.startsWith("terminal.")) return "terminal";
  if (key.startsWith("java.")) return "java";
  if (key.startsWith("maven.")) return "maven";
  if (key.startsWith("update.")) return "system";
  return "appearance";
}

/** 搜索：名字、说明、键名里有这几个字（不分大小写）。空的都算 */
export function matches(d: SettingDef, q: string): boolean {
  const s = q.trim().toLowerCase();
  if (!s) return true;
  return [d.label, d.doc, d.key].some((x) => x.toLowerCase().includes(s));
}

/** 一项的默认值（`default` 是 JSON 字面量） */
export function defaultOf(d: SettingDef): unknown {
  try {
    return JSON.parse(d.default);
  } catch {
    return null;
  }
}

/** 改了还没应用的：键 → 新值；`null` = 恢复默认 */
export type Edits = Record<string, unknown>;

/** 这一项现在该显示什么：改了没应用的 > 生效的。恢复默认（null）显示默认值 */
export function shown(page: SettingsPage, edits: Edits, d: SettingDef): unknown {
  if (d.key in edits) return edits[d.key] === null ? defaultOf(d) : edits[d.key];
  return page.values[d.key] ?? defaultOf(d);
}

/** 这一项算不算「改过」（左边那条色条、「恢复默认」按钮）：settings.json 里写了它，或者这次改了它；这次点了恢复默认的不算 */
export function isSet(page: SettingsPage, edits: Edits, key: string): boolean {
  if (key in edits) return edits[key] !== null;
  return page.written.includes(key);
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/**
 * 「应用」时交给 Rust 的那一批：只放真变了的 —— 改回原样的、对一个本来就没写的键点「恢复默认」的，都不算。
 * 不算的意思是 settings.json 一个字节都不动：人只是点开看了看，文件不该因此被改写
 */
export function changes(page: SettingsPage, edits: Edits): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(edits)) {
    if (v === null) {
      if (page.written.includes(k)) out[k] = null;
    } else if (!same(v, page.values[k])) {
      // 和生效的一样就是没改：文件里写了的，写回同一个值；没写的，生效的是默认值、人只是填了一遍默认值
      out[k] = v;
    }
  }
  return out;
}

/** 整数输入框的文字 → 值。空的、不是整数、越界的回错误（越界不替人夹：设置页里看得见范围，填错了说出来） */
export function parseIntField(d: SettingDef, text: string): { value: number } | { error: string } {
  const t = text.trim();
  if (!/^-?\d+$/.test(t)) return { error: "要填一个整数" };
  const n = Number(t);
  if (d.min !== null && n < d.min) return { error: `最小 ${d.min}` };
  if (d.max !== null && n > d.max) return { error: `最大 ${d.max}` };
  return { value: n };
}
