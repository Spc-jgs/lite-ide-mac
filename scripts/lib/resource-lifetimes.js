// 由测试通道按需注入；只保存 WeakRef/计数，不长期强持有目标，也不请求 GC。解引用会短暂保活目标。
if (typeof WeakRef !== "function") throw new Error("本 WebKit 不支持 WeakRef，生命周期诊断不可用");
if (window.__resourceLifetimes) throw new Error("生命周期探针已经安装");
const groups = Object.fromEntries(["views", "docs", "canvases"].map(k => [k, { seen: new WeakSet(), refs: [] }]));
const remember = (kind, value) => {
  const group = groups[kind];
  if (group.seen.has(value)) return;
  if (group.refs.length >= 1024) throw new Error("生命周期探针超过 1024 个对象，停止诊断");
  group.seen.add(value);
  group.refs.push(new WeakRef(value));
};
const create = document.createElement;
const wrapped = function (...args) {
  const el = Reflect.apply(create, this, args);
  if (String(args[0]).toLowerCase() === "canvas") remember("canvases", el);
  return el;
};
document.createElement = wrapped;
window.__resourceLifetimes = {
  record() {
    // 和现有 test-hooks 的编辑器入口一致，不改应用的状态/扩展。
    const view = document.querySelector(".cm-content")?.cmTile?.view;
    if (view) {
      remember("views", view);
      remember("docs", view.state.doc);
    }
    return !!view;
  },
  snapshot() {
    const result = { version: "weakref-v1" };
    for (const [kind, { refs }] of Object.entries(groups)) {
      let alive = 0, connected = 0, nonzero = 0, pixels = 0;
      for (const ref of refs) {
        const value = ref.deref();
        if (!value) continue;
        alive++;
        if (kind === "views" && value.dom.isConnected) connected++;
        if (kind === "canvases") {
          if (value.isConnected) connected++;
          if (value.width > 0 && value.height > 0) nonzero++;
          pixels += value.width * value.height;
        }
      }
      result[kind] = { observed: refs.length, alive, cleared: refs.length - alive };
      if (kind !== "docs") result[kind].connected = connected;
      if (kind === "canvases") Object.assign(result[kind], { nonzero, pixels });
    }
    return result;
  },
  stop() {
    if (document.createElement === wrapped) document.createElement = create;
    delete window.__resourceLifetimes;
    return true;
  },
};
return { version: "weakref-v1" };
