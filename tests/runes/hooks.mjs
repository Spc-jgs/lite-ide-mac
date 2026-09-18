/**
 * 让 `.svelte.ts`（runes 状态层）能在裸 node 里跑 —— 模块 loader 钩子。
 *
 * 状态层（tabflow / docs / persist / git…）一直是零自动化测试：历史上三次丢数据的 bug
 * 全出在这层，靠间歇红的 smoke 兜。原因是 `$state` / `$derived` 不是 JS，node 认不得；
 * 而拉 vitest + vite 进来是为了一个 transform 装一整套。
 *
 * 其实 svelte 自己就带编译器：`compileModule` 把一个 `.svelte.ts` 编成引用
 * `svelte/internal/client` 的普通 JS —— 那正是浏览器里跑的东西，只是不经 vite。
 * 这里做两件事：
 *
 * 1. resolve：源码里 `import "./tabs.svelte"` / `import "../ipc/commands"` 没带扩展名
 *    （vite 补的），node 的 ESM 不补 —— 给相对路径试一遍 `.ts`。
 * 2. load：`.svelte.ts` 先用 node 自带的 `stripTypeScriptTypes` 剥类型，再 `compileModule`。
 *    别的 `.ts` 交回 node 原生剥类型。
 *
 * svelte 的 client 运行时不碰 DOM 就能跑：`$state` 是信号，`$derived` 懒算，
 * `$effect` 需要一个 effect 根 —— 测试里用 `$effect.root`（见 harness）。
 */
import { readFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import { existsSync } from "node:fs";
import { compileModule } from "svelte/compiler";

export async function resolve(specifier, context, next) {
  if ((specifier.startsWith("./") || specifier.startsWith("../")) && context.parentURL) {
    const base = new URL(specifier, context.parentURL);
    const p = fileURLToPath(base);
    if (!existsSync(p) && existsSync(`${p}.ts`)) {
      return next(pathToFileURL(`${p}.ts`).href, context);
    }
  }
  return next(specifier, context);
}

export async function load(url, context, next) {
  if (url.endsWith(".svelte.ts")) {
    const src = await readFile(fileURLToPath(url), "utf8");
    const js = stripTypeScriptTypes(src, { mode: "strip" });
    const out = compileModule(js, { filename: fileURLToPath(url), generate: "client", dev: true });
    return { format: "module", source: out.js.code, shortCircuit: true };
  }
  return next(url, context);
}
