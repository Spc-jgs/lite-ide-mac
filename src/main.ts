import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import "./app.css";
import { installMockIpc } from "./lib/dev/mock-ipc";

// 浏览器里跑 `pnpm dev` 时装 IPC 桩：改 UI 不必等壳重新编译（约 40 秒 → 毫秒）。
//
// 用静态 import + 条件调用而非 `await import()`，纯粹是因为不需要顶层 await 就能
// 做到同样的事 —— 生产构建里 import.meta.env.DEV 为假，if 块被消除，
// installMockIpc 随之无人引用，整个模块被 tree-shake 掉，产物里一个字节都不剩。
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
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
    "font-family:-apple-system,'PingFang SC',system-ui,sans-serif;color:#dfe1e5";
  const inner = document.createElement("div");
  inner.style.cssText =
    "width:min(680px,90vw);background:#2b2d30;border:1px solid #393b40;" +
    "border-radius:8px;padding:18px 20px";
  const h = document.createElement("div");
  h.textContent = "启动失败";
  h.style.cssText = "color:#f75464;font-size:14px;margin-bottom:10px";
  const pre = document.createElement("pre");
  pre.textContent = detail;
  pre.style.cssText =
    "margin:0 0 14px;padding:10px 12px;max-height:300px;overflow:auto;background:#1e1f22;" +
    "border:1px solid #393b40;border-radius:5px;font-family:'SF Mono',Menlo,monospace;" +
    "font-size:11px;line-height:1.65;color:#9da0a8;white-space:pre-wrap;user-select:text";
  const btn = document.createElement("button");
  btn.textContent = "重载窗口";
  btn.style.cssText =
    "padding:4px 12px;background:#3574f0;border:1px solid #3574f0;border-radius:4px;" +
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
