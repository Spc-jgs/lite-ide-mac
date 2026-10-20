/**
 * 桩：本地历史（docs/IDEA.md 第 21 节），对应 `src-tauri/src/commands/history.rs`（规则在 `crates/histsvc`）。
 * 规矩照真实现：保存时记「存下去的那份」，保存前盘上那份和上一版不一样就先补记一版（before）；和上一版一样的不重复记。
 * 桩里没有编码这回事（FILES 存的就是文字），`history_read` 原样给
 */
import { type A, NOT_MINE } from "./data";

type V = { ms: number; hash: string; size: number; kind: "saved" | "before"; text: string };
const STORE = new Map<string, V[]>();

function hashOf(s: string): string {
  let h = 0x811c9dc5;
  for (const b of new TextEncoder().encode(s)) h = Math.imul(h ^ b, 0x01000193) >>> 0;
  return h.toString(16).padStart(16, "0");
}

/** fs.ts 的 write_text 在写之前叫：`old` 是盘上原来那份（新文件是 undefined） */
export function mockHistoryRecord(path: string, old: string | undefined, next: string) {
  const list = STORE.get(path) ?? [];
  const add = (text: string, kind: V["kind"]) => {
    const hash = hashOf(text);
    if (list.length && list[list.length - 1].hash === hash) return;
    list.push({ ms: Date.now(), hash, size: new TextEncoder().encode(text).length, kind, text });
  };
  if (old !== undefined) add(old, "before");
  add(next, "saved");
  STORE.set(path, list);
}

export function historyCmd(cmd: string, a: A): unknown {
  switch (cmd) {
    case "history_list":
      return [...(STORE.get(String(a.path)) ?? [])].reverse().map(({ text: _t, ...e }) => e);
    case "history_read": {
      const v = (STORE.get(String(a.path)) ?? []).find((x) => x.hash === String(a.hash));
      if (!v) throw new Error("没有这一版了（可能超过 30 天被清掉了）");
      return v.text;
    }
    default:
      return NOT_MINE;
  }
}
