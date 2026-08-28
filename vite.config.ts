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
  },
  build: {
    target: "safari15",   // macOS WKWebView，不必为老浏览器降级
    minify: "esbuild",
    sourcemap: false,
  },
});
