import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../scripts/lib/resource-lifetimes.js", import.meta.url), "utf8");
const refs: FakeRef[] = [];
class FakeRef {
  value: object | undefined;
  constructor(value: object) { this.value = value; refs.push(this); }
  deref() { return this.value; }
}
let active: { dom: { isConnected: boolean }; state: { doc: object } } | null = null;
const create = function(this: unknown, tag: string) {
  assert.equal(this, document, "保留 createElement 的接收者");
  return { tagName: tag.toUpperCase(), width: 300, height: 150, isConnected: false };
};
const document = {
  createElement: create,
  querySelector() { return active ? { cmTile: { view: active } } : null; },
};
const window: Record<string, any> = {};
const install = () => vm.runInNewContext(`(function(){${source}\n})()`, { window, document, WeakRef: FakeRef, WeakSet });
const plain = (value: unknown) => JSON.parse(JSON.stringify(value));
install();
const p = window.__resourceLifetimes;
assert.equal(p.record(), false, "没有编辑器时不伪造对象");
assert.deepEqual(plain(p.snapshot().views), { observed: 0, alive: 0, cleared: 0, connected: 0 });
active = { dom: { isConnected: true }, state: { doc: {} } };
assert.equal(p.record(), true);
p.record();
document.createElement("div");
const canvas = document.createElement("canvas");
let s = p.snapshot();
assert.deepEqual(plain(s.views), { observed: 1, alive: 1, cleared: 0, connected: 1 }, "按对象去重");
assert.deepEqual(plain(s.docs), { observed: 1, alive: 1, cleared: 0 });
assert.equal(s.canvases.observed, 1, "只跟踪画布");
assert.equal(s.canvases.nonzero, 1);
assert.equal(s.canvases.pixels, 45_000);
active.dom.isConnected = false;
canvas.width = canvas.height = 0;
s = p.snapshot();
assert.equal(s.views.alive, 1, "从 DOM 移除不伪装成已经 GC");
assert.equal(s.views.connected, 0);
assert.equal(s.canvases.alive, 1);
assert.equal(s.canvases.nonzero, 0, "已清位图和对象回收分别计数");
assert.equal(s.canvases.pixels, 0);
for (const ref of refs) ref.value = undefined;
s = p.snapshot();
assert.deepEqual(plain(s.views), { observed: 1, alive: 0, cleared: 1, connected: 0 });
assert.deepEqual(plain(s.docs), { observed: 1, alive: 0, cleared: 1 });
assert.equal(s.canvases.cleared, 1);
assert.throws(install, /已经安装/);
p.stop();
assert.equal(document.createElement, create, "退出后恢复原来的方法");
assert.equal(window.__resourceLifetimes, undefined);
assert.throws(() => vm.runInNewContext(`(function(){${source}\n})()`, { window, document, WeakRef: undefined }), /不支持 WeakRef/);
install();
for (let i = 0; i < 1024; i++) document.createElement("canvas");
assert.throws(() => document.createElement("canvas"), /超过 1024/, "诊断本身有数量上限");
window.__resourceLifetimes.stop();
console.log("✅ 资源生命周期探针：去重、DOM/位图/回收分离、方法恢复、缺失能力和数量上限通过");
