import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

/*
 * 构建时间戳，注入成编译期常量。
 *
 * 起因是一次真实的误诊：我照着报上来的现象查了半天代码，
 * 最后发现那个 bug 早就修好了 —— 跑的是前一天编出来的 .app。
 * 界面上没有任何地方能回答「我现在跑的是哪个构建」，
 * 于是「重现不了」和「你版本旧了」这两种情况长得一模一样。
 *
 * 标题栏上悬停应用名就能看到它。
 */
const BUILD_TIME = new Date()
  .toLocaleString("sv-SE", { timeZone: "Asia/Shanghai" })
  .slice(0, 16);

export default defineConfig({
  plugins: [svelte()],
  define: {
    __BUILD_TIME__: JSON.stringify(BUILD_TIME),
  },
  // Tauri 期望固定端口，端口被占则直接失败而不是悄悄换一个
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
    /*
     * 浏览器里也用生产那条 CSP。
     *
     * 跟 mock-ipc 是同一个理由：CSP 挡下一个东西不会报错，只表现为
     * 「某处不好使了」。要是只有打包后的壳才带 CSP，这类问题就得等
     * 45 秒的 Tauri 构建之后才撞得上，而且撞上了也不知道是 CSP 干的。
     * 这里跟 src-tauri/tauri.conf.json 里那条保持一致 —— 改一边记得改另一边。
     */
    headers: {
      "Content-Security-Policy":
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; " +
        "img-src 'self' data:; font-src 'self'; connect-src 'self' ws: ipc: http://ipc.localhost; " +
        "object-src 'none'; base-uri 'none'; frame-src 'none'",
    },
  },
  build: {
    target: "safari15",   // macOS WKWebView，不必为老浏览器降级
    minify: "esbuild",
    sourcemap: false,
  },
});
