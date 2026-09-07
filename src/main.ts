import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import "./app.css";

/*
 * 窗口后面有没有材质层，这一行说了算。
 *
 * 透明窗口是没有退路的：`transparent: true` 之后窗口自己不画底，
 * 全靠 Rust 侧挂的那块 NSVisualEffectView。而浏览器里跑 `pnpm dev`
 * 根本没有那块 view —— 外壳层要是照样留空，透出来的就是浏览器的白底。
 *
 * 所以判据不能是「深色还是浅色」，得是「这个壳到底有没有材质层」。
 * 用 `__TAURI_INTERNALS__` 是因为它**同步、零成本、不用等 IPC** ——
 * 这行代码在 mount 之前跑，晚一帧就是一帧的白闪。
 * （挂载失败时 Rust 侧会 eval 把它打回 `web`，见 lib.rs 的 apply_window_material。）
 */
document.documentElement.dataset.shell =
  "__TAURI_INTERNALS__" in window ? "tauri" : "web";

/*
 * 浏览器里跑 `pnpm dev` 时装 IPC 桩：改 UI 不必等壳重新编译（约 40 秒 → 毫秒）。
 *
 * **必须是 `await import()`，不能是静态 import + 条件调用。**
 *
 * 原来就是后者，注释还写着「整个模块被 tree-shake 掉，产物里一个字节都不剩」——
 * 那是错的。2026-09-07 按 sourcemap 归因，`mock-ipc.ts` 在生产入口包里
 * 占着 1,195 字节：桩里那张提交图是模块级的 `[...].map(...)`，
 * **`.map()` 是方法调用，打包器证明不了它没有副作用**，于是连同整个数据字面量
 * 一起留下了。产物里能直接 grep 到 `m13/git` 这种只有桩里才有的字符串。
 *
 * 换成动态 import 之后，这条保证就不再依赖打包器的 tree-shaking 能做到哪一步 ——
 * `import.meta.env.DEV` 在生产构建里是常量假，整个 if 块（连同那句 import）
 * 在语法层面就被消除了。桩里之后再写什么都泄不出来。
 *
 * 代价是一个顶层 await。它只在 dev 分支上存在，而且必须 await ——
 * 桩得在 `mount(App)` 之前装好，否则首屏那几个 invoke 会打空。
 */
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installMockIpc } = await import("./lib/dev/mock-ipc");
  installMockIpc();
}

// 开发期诊断：release 没有 devtools，前端出了错在 WebView 里是黑盒，
// 只能靠这条通道回传到 Rust 侧 stderr（需 LITE_IDE_DEBUG=1）。
const diag = (msg: string) => {
  invoke("diag", { msg }).catch(() => {});
};
window.addEventListener("error", (e) =>
  diag(`window.error: ${e.message} @ ${e.filename}:${e.lineno}`),
);
// 模块加载阶段就失败（语法错、chunk 404）时 mount 根本不会被执行，
// 只能靠这个事件把白屏换成一块能读的错误屏
window.addEventListener("error", (e) => {
  if (!document.getElementById("app")?.hasChildNodes()) fatal(e.error ?? e.message, "加载脚本");
});
window.addEventListener("unhandledrejection", (e) => diag(`unhandledrejection: ${e.reason}`));
/*
 * CSP 违规不会触发 window.error —— 被挡掉的资源就那么静静地没加载，
 * 界面上只表现为「某个东西不好使了」，查起来毫无线索。
 * 收紧 CSP 的同时必须给它配一条回传通道，否则下次加个依赖被 CSP 挡了，
 * 得从零开始猜。
 */
document.addEventListener("securitypolicyviolation", (e) =>
  diag(`CSP 挡下: ${e.violatedDirective} ← ${e.blockedURI} @ ${e.sourceFile}:${e.lineNumber}`),
);

/**
 * 挂载失败时的最后一道兜底。
 *
 * 组件内部出错有 <svelte:boundary> 接着，但 mount() 本身炸了的话 Svelte 都没起来 ——
 * 只能用原生 DOM 画一块屏。没有这块屏，用户看到的就是纯白窗口：
 * 既不知道出了什么事，也不知道该把什么信息发给我。
 */
function fatal(e: unknown, phase: string) {
  const err = e as { message?: string; stack?: string } | null;
  const detail = [
    `位置：${phase}`,
    `构建：${__BUILD_TIME__}`,
    `消息：${err?.message ?? String(e)}`,
    "",
    err?.stack ?? "",
  ].join("\n");
  diag(`fatal [${phase}] ${detail}`);

  const root = document.getElementById("app");
  if (!root) return;
  // 这里刻意不用任何框架、不引任何模块 —— 走到这一步说明它们已经不可信了
  root.innerHTML = "";
  const box = document.createElement("div");
  box.style.cssText =
    "height:100%;display:grid;place-content:center;padding:24px;" +
    "font-family:-apple-system,'PingFang SC',system-ui,sans-serif;color:#cdcdcd";
  const inner = document.createElement("div");
  inner.style.cssText =
    "width:min(680px,90vw);background:#232326;border:1px solid rgba(255,255,255,.13);" +
    "border-radius:14px;padding:18px 20px";
  const h = document.createElement("div");
  h.textContent = "启动失败";
  h.style.cssText = "color:#f75464;font-size:14px;margin-bottom:10px";
  const pre = document.createElement("pre");
  pre.textContent = detail;
  pre.style.cssText =
    "margin:0 0 14px;padding:10px 12px;max-height:300px;overflow:auto;background:#1e1f22;" +
    "border:1px solid rgba(255,255,255,.09);border-radius:6px;" +
    "font-family:'SF Mono',Menlo,monospace;" +
    "font-size:11px;line-height:1.65;color:#8a8a8a;white-space:pre-wrap;user-select:text";
  const btn = document.createElement("button");
  btn.textContent = "重载窗口";
  btn.style.cssText =
    "padding:4px 12px;background:#5b8def;border:1px solid #5b8def;border-radius:6px;" +
    "color:#fff;font-size:12px";
  btn.onclick = () => location.reload();
  inner.append(h, pre, btn);
  box.append(inner);
  root.append(box);
}

diag("main.ts 开始执行");
let app;
try {
  app = mount(App, { target: document.getElementById("app")! });
  diag("App 已挂载");
} catch (e) {
  fatal(e, "挂载 App");
}
export default app;
