/**
 * 语言识别表（文件名 → 语言 id → 显示名），**只在有标签打开时才拉**。
 *
 * 入口包是**首屏之前必须解析执行完**的那一段，而这张表回答的两个问题
 * （状态栏显示什么语言、⌘⇧O 支不支持这个文件）都要先有一个打开的文件
 * 才成立 —— 一个都没打开时它纯属压秤。实测省 3,419 字节（挪走前后各量一次，
 * 不是按 sourcemap 归因的估值，归因会高估数据密集的模块，见 frontend.md）。
 *
 * 表还没到手时：语言那格空着，`outlineSupported` 是假。两者都只持续到
 * 那个几 KB 的 chunk 回来为止，而它和编辑器（370 KB）是同时开始拉的。
 * 编辑器那边照旧直接 `import` 它 —— 那个 chunk 本来就是懒的，两处引到的是同一份模块。
 *
 * 状态栏、内容区、大纲浮层都要读它，所以是 store（issue #9 第 6 步）；
 * App 的 effect 在 `tabs.active` 一有值时调 `load()`。
 */
class Lang {
  mod = $state<typeof import("../editor/langs") | null>(null);

  load() {
    if (this.mod) return;
    void import("../editor/langs").then((m) => (this.mod = m));
  }
}

export const lang = new Lang();
