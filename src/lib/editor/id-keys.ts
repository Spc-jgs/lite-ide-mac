/**
 * 编辑器里的键按**动作 id** 挂（docs/TAKEOVER.md R3）：`{ id: "find-usages", run }`，键位从 `keymap.ts` 取生效的那个（默认 ⊕ 你改的）。
 *
 * 原来每个扩展把键写死在自己的 `keymap.of([…])` 里（⌥F7 写成 `Alt-F7`），`keymap.ts` 里那行 `cm6` 只是说明书 —— 两边各写一份，
 * 你在设置里改了也改不到编辑器。现在键只写在 `keymap.ts` 一处。
 *
 * **改了当场生效**：每张表包在自己的 compartment 里，覆盖表一换（`onOverrides`），所有开着的编辑器各自 reconfigure。
 * 不是「新开的标签才生效」—— IDEA 改完键位，开着的编辑器里立刻就是新的。
 */
import { Compartment, type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, keymap, type Command, type KeyBinding } from "@codemirror/view";
import { KEYS, accelOf, onOverrides, toCmKey } from "../state/keymap";
import "../state/keys-live";

export interface IdBinding {
  /** `keymap.ts` 里的 id（owner 是 cm6；`save` 是例外，见 keymap.ts 那一行） */
  id: string;
  run: Command;
  /** 同 `KeyBinding.shift`：按着 ⇧ 时跑这个（查找下一处 / 上一处共用一个键） */
  shift?: Command;
  scope?: string;
  preventDefault?: boolean;
}

const byId = new Map(KEYS.map((k) => [k.id, k]));

/** 按生效的键位换成 CM6 的 `KeyBinding`。没有键位（你改成了「不要」）的那条不挂 */
export function resolve(bs: readonly IdBinding[]): KeyBinding[] {
  const out: KeyBinding[] = [];
  for (const b of bs) {
    const def = byId.get(b.id);
    if (!def) throw new Error(`keymap.ts 里没有 ${b.id}`);
    const key = toCmKey(accelOf(def));
    if (key) out.push({ key, run: b.run, shift: b.shift, scope: b.scope, preventDefault: b.preventDefault });
  }
  return out;
}

/** 开着的编辑器里每一张按 id 挂的表：覆盖表一换，逐个重配 */
const live = new Set<{ view: EditorView; comp: Compartment; bs: readonly IdBinding[] }>();
onOverrides(() => {
  for (const r of live) {
    if (r.view.dom.isConnected) r.view.dispatch({ effects: r.comp.reconfigure(keymap.of(resolve(r.bs))) });
  }
});

/**
 * 一张按 id 挂的键位表。外面照旧套 `Prec.high(...)` 这些（compartment 重配不动外面那层优先级）
 */
export function idKeymap(bs: readonly IdBinding[]): Extension {
  const comp = new Compartment();
  return [
    comp.of(keymap.of(resolve(bs))),
    ViewPlugin.define((view) => {
      const r = { view, comp, bs };
      live.add(r);
      return { destroy: () => void live.delete(r) };
    }),
  ];
}
