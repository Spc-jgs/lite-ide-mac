import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
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
