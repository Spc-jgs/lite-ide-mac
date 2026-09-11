import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import { setInvariantSink } from "./lib/state/invariant";
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

/*
 * **异常同时落盘。**
 *
 * `diag` 默认闭嘴，而且 stderr 在双击启动的 `.app` 里没人接 —— 也就是说
 * 在这之前，一次真实的前端崩溃**不留任何痕迹**，只能等用户再复现一遍。
 * 「默认关掉的可观测性等于没有」：出事的那一次，人不会正好带着
 * `LITE_IDE_DEBUG=1` 在跑。
 *
 * 两条一起发是有意的，不是重复：开发时盯着终端看 `diag`，
 * 事后回头查看 `app.log`。
 */
const oops = (level: "warn" | "error", source: string, msg: string) => {
  diag(`${source}: ${msg}`);
  invoke("app_log", { level, source, msg }).catch(() => {});
};

/*
 * 运行时不变量自检的出口（issue #27）。
 *
 * `state/invariant.ts` 自己不 import 任何东西 —— 上报通道从这里注进去。
 * 这样它在 `tests/` 里能拿裸 node 跑，而**一个自己没被测过的自检器
 * 只会往日志里写噪音**。
 *
 * 走 `app_log` 而不是 `diag`：这类 bug 的特征就是「状态悄悄地不对、
 * 界面照常画」，发现它的时刻往往在事后，而 `diag` 默认闭嘴、
 * `.app` 又没有 stderr。级别用 error —— 不变量不成立就是 bug，
 * 哪怕用户当时没察觉。
 */
setInvariantSink((msg) => {
  invoke("app_log", { level: "error", source: "invariant", msg }).catch(() => {});
});

window.addEventListener("error", (e) =>
  oops("error", "window.error", `${e.message} @ ${e.filename}:${e.lineno}\n${e.error?.stack ?? ""}`),
);
// 模块加载阶段就失败（语法错、chunk 404）时 mount 根本不会被执行，
// 只能靠这个事件把白屏换成一块能读的错误屏
window.addEventListener("error", (e) => {
  if (!document.getElementById("app")?.hasChildNodes()) fatal(e.error ?? e.message, "加载脚本");
});
window.addEventListener("unhandledrejection", (e) =>
  oops("error", "unhandledrejection", `${e.reason}\n${e.reason?.stack ?? ""}`),
);
/*
 * CSP 违规不会触发 window.error —— 被挡掉的资源就那么静静地没加载，
 * 界面上只表现为「某个东西不好使了」，查起来毫无线索。
 * 收紧 CSP 的同时必须给它配一条回传通道，否则下次加个依赖被 CSP 挡了，
 * 得从零开始猜。
 */
document.addEventListener("securitypolicyviolation", (e) =>
  oops("warn", "csp", `挡下 ${e.violatedDirective} ← ${e.blockedURI} @ ${e.sourceFile}:${e.lineNumber}`),
);

/*
 * 内存诊断：把**对象数**报出来，而不是让人去猜进程内存。
 *
 * 起因是 issue #10。`scripts/mem.sh` 量的是四个进程的 Physical footprint，
 * 那个数字的噪声（同样状态多次启动差 40MB）比要测的信号还大，
 * 只能回答「有没有线性泄漏」，回答不了「关掉的标签到底释放了没有」。
 *
 * 而这三个数是**确定的**，没有 GC 时机的干扰：
 *
 *   - editors —— CM6 的根元素个数。关掉全部标签之后必须是 0，
 *     不是 0 就说明 EditorView 没销毁干净，那是真泄漏。
 *   - nodes   —— 整棵 DOM 的元素数，跟着标签开关涨落。
 *   - langs   —— 语言包缓存里装了几个。它只 set 不 evict 是**设计**
 *     （67 封顶），把它报出来是为了让「有没有上限」可以被量。
 *
 * **关着的时候一次都不算。** 判据是 Rust 侧的 `diag_enabled`（即
 * `LITE_IDE_DEBUG=1`），不是 `import.meta.env.DEV` —— 要量的正是
 * `pnpm app:bundle` 出来的 release 包，dev 模式加载的是 localhost 的前端。
 *
 * langs-load 走动态 import：静态引会把那 500 行连同 67 个 import 桩
 * 拉回入口包（见 rules/frontend.md 那条 150KB 红线）。
 */
invoke<boolean>("diag_enabled")
  .then(async (on) => {
    if (!on) return;
    const { langCacheSize } = await import("./lib/editor/langs-load");
    const report = () =>
      diag(
        `mem editors=${document.querySelectorAll(".cm-editor").length}` +
          ` nodes=${document.getElementsByTagName("*").length}` +
          ` langs=${langCacheSize()}`,
      );
    setTimeout(report, 1500); // 等首屏挂完，否则第一条永远是 0
    setInterval(report, 3000);
  })
  .catch(() => {});

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
