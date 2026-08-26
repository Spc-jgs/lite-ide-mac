import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import "./app.css";

// 开发期诊断：任何 JS 错误都要能在 Rust 侧 stderr 看到，
// 否则 release build 没有 devtools 时前端是个黑盒。
const diag = (msg: string) => {
  invoke("diag", { msg }).catch(() => {});
};
window.addEventListener("error", (e) =>
  diag(`window.error: ${e.message} @ ${e.filename}:${e.lineno}`),
);
window.addEventListener("unhandledrejection", (e) =>
  diag(`unhandledrejection: ${e.reason}`),
);

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
