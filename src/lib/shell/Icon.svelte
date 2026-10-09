<script module lang="ts">
  /**
   * 全部图标名。**类型从这张表派生**（不是反过来）：画廊页（`?gallery`）要把每一个都摆出来，
   * 而 TS 的联合类型在运行时不存在，只有值才能被遍历。
   * 加图标 = 在这儿加一行 + `public/icons/<名字>.svg` 放一个文件 + `SOURCES.md` 记一行来源。
   */
  export const ICON_NAMES = [
    "pull",
    "push",
    "commit",
    "folder",
    "tag",
    "sidebar",
    "files",
    "git",
    "search",
    "terminal",
    "history",
    "note",
    "locate",
    "collapse",
    "follow",
    "compact",
    "pin",
    "more-v",
    "minus",
    "refresh",
    "check",
    "plus",
    "warn",
    "chevron-up",
    "chevron-down",
    "chevron-right",
    "swap",
    "more-h",
    "branch-current",
    "remote",
    "x",
    "undo",
    // ⇧⌘F 的三个开关（#42）：和 IDEA 查找框里那三个是同一份
    "match-case",
    "whole-word",
    "regex",
  ] as const;

  export type IconName = (typeof ICON_NAMES)[number];

  /**
   * 带颜色的那几个：画原色（`<img>`），不跟着 hover / 选中变色。
   *
   * `warn` 是黄的、`note`（草稿）带一个蓝色时钟 —— 颜色本身就是意思。`folder`、`compact` 是**深灰填充 + 浅灰描边**
   * 的双色图标：走 mask 的话只剩透明度，填充和描边糊成一整块实心。其余全是单一灰色，走 mask。
   */
  const COLORED: ReadonlySet<IconName> = new Set<IconName>(["warn", "note", "folder", "compact"]);
</script>

<script lang="ts">
  /**
   * 界面图标的唯一出处 —— **JetBrains 新 UI 的官方图标**（2026-09-24 换的）。
   *
   * 之前是自己手画的 32 个：规矩是齐的（16 网格、1.25 描边、round 端点），但形状是我们画的，
   * 和 IDEA 并排一看就知道不是一家。现在整套取自 intellij-community 的 `platform/icons/src/expui/`
   * （Apache 2.0，许可和逐个来源见 `public/icons/`），深色主题版，文件原样不改。
   * **例外**：`tag` 是自绘的 —— IDEA 新 UI 没有 git 标签的独立图标，见 `SOURCES.md`。
   *
   * # 为什么是 `public/` 里的文件，不是内联 SVG
   *
   * 这个组件在入口包里，而入口包离 160 KB 红线只剩十来 KB —— 六十多个 SVG 内联进来就是 60 多 KB。
   * 放 `public/` 里原样拷进产物，JS 里只剩一个路径。（不走 `import x from "./x.svg"`：
   * Vite 会把小于 4 KB 的内联成 data URL，塞回 JS 里，而且 base64 还更大。）
   *
   * # 单色的走 CSS mask，不是 `<img>`
   *
   * 图标的颜色要跟着状态走（hover 亮、选中白、禁用暗）—— `<img>` 画的是文件里写死的 `#CED0D6`，
   * 改不了。mask 只取 SVG 的**形状**（透明度），颜色是 `background-color: currentColor`，
   * 于是和原来的描边图标一样跟着 `color` 变。顺带文件一个字都不用改（改了按 Apache 2.0 要逐个注明）。
   *
   * CSP 的 `img-src 'self'` 管 mask 图片和 `<img>`，本来就放行。
   */
  /**
   * `size` 默认 16，**组件里别再传** —— JetBrains 的图标是按 16 的网格画的，缩到 10–12 线条落在半像素上。
   * 2026-09-24 之前各处传了 10 / 11 / 12 / 13 / 14 / 16 / 17 七种，一起去掉了。
   */
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

{#if COLORED.has(name)}
  <img class="icon" src="/icons/{name}.svg" width={size} height={size} alt="" aria-hidden="true" draggable="false" />
{:else}
  <span
    class="icon mono"
    style:width="{size}px"
    style:height="{size}px"
    style:--src="url(/icons/{name}.svg)"
    aria-hidden="true"
  ></span>
{/if}

<style>
  .icon {
    display: inline-block;
    flex: none;
    vertical-align: middle;
  }
  .mono {
    background-color: currentColor;
    -webkit-mask: var(--src) center / contain no-repeat;
    mask: var(--src) center / contain no-repeat;
  }
</style>
