import assert from "node:assert/strict";
import { poll } from "../src/lib/poll.ts";
const originalSet = globalThis.setTimeout, originalClear = globalThis.clearTimeout;
let serial = 0;
const timers = new Map<number, () => void>();
globalThis.setTimeout = ((fn: () => void) => { const id = ++serial; timers.set(id, fn); return id; }) as any;
globalThis.clearTimeout = ((id: number) => timers.delete(id)) as any;
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const fire = async () => { const [id, fn] = timers.entries().next().value!; timers.delete(id); fn(); await flush(); };
const errors: unknown[] = [];
try {
  let release!: () => void;
  const gate = new Promise<void>(r => release = r);
  let calls = 0, writes = 0;
  const stop = poll(100, async live => { calls++; await gate; if (live()) writes++; return true; }, e => errors.push(e));
  assert.equal(calls, 0);
  await fire();
  assert.equal(calls, 1);
  assert.equal(timers.size, 0, "在途期间没有下一次 tick，慢请求不叠加");
  stop(); release(); await flush();
  assert.equal(writes, 0, "关闭期间返回的结果不写回");
  assert.equal(timers.size, 0, "在途请求返回后不复活计时器");

  calls = 0;
  poll(80, async () => ++calls < 2, e => errors.push(e), true);
  await flush();
  assert.equal(calls, 1);
  await fire();
  assert.equal(calls, 2);
  assert.equal(timers.size, 0, "完成后不再安排下一轮");

  poll(80, async () => { throw new Error("IPC failed"); }, e => errors.push(e));
  await fire();
  assert.equal(errors.length, 1);
  assert.equal(timers.size, 0, "失败仅报告一次，并停止");

  const stopBeforeStart = poll(80, async () => { throw new Error("不该运行"); }, e => errors.push(e));
  stopBeforeStart();
  assert.equal(timers.size, 0);
  assert.equal(errors.length, 1);
  console.log("✅ 异步采样：慢请求、关闭、完成、失败、首次延迟通过");
} finally {
  globalThis.setTimeout = originalSet;
  globalThis.clearTimeout = originalClear;
}
