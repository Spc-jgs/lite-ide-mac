/** `node --import tests/runes/register.mjs x.test.ts`：装上 hooks.mjs，然后把浏览器全局垫上 */
import { register } from "node:module";
register("./hooks.mjs", import.meta.url);

// 状态层碰的浏览器全局：`window`（桩把 __TAURI_INTERNALS__ 挂在上面）、`localStorage`（会话快照）。
// 都是最小的假，够状态层跑就行 —— 这里不是在模拟浏览器，是在让「和 DOM 无关的逻辑」能被叫到
const store = new Map();
globalThis.localStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => void store.set(k, String(v)),
  removeItem: (k) => void store.delete(k),
  clear: () => store.clear(),
  key: (i) => [...store.keys()][i] ?? null,
  get length() {
    return store.size;
  },
};
globalThis.window = globalThis;
