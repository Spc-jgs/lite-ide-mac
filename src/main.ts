import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import "./app.css";
import { installMockIpc } from "./lib/dev/mock-ipc";

// 浏览器里跑 `pnpm dev` 时装 IPC 桩：改 UI 不必等壳重新编译（约 40 秒 → 毫秒）。
//
// 刻意用静态 import + 条件调用，而不是 `await import()`：顶层 await 会把整个
// 入口变成异步模块，实测在 Tauri 的 WKWebView 里页面直接不加载（page_load
// 事件都不触发）。生产构建里 import.meta.env.DEV 为假，if 块被消除，
// installMockIpc 随之无人引用，整个模块会被 tree-shake 掉。
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
window.addEventListener("unhandledrejection", (e) => diag(`unhandledrejection: ${e.reason}`));

diag("main.ts 开始执行");
let app;
try {
  app = mount(App, { target: document.getElementById("app")! });
  diag("App 已挂载");
} catch (e) {
  diag(`mount 失败: ${e}`);
  throw e;
}
export default app;
