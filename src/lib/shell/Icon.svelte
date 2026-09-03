<script module lang="ts">
  export type IconName =
    | "sidebar"
    | "files"
    | "git"
    | "search"
    | "panel"
    | "refresh"
    | "check"
    | "plus"
    | "warn"
    | "chevron-up"
    | "chevron-down";
</script>

<script lang="ts">
  /**
   * 界面图标的唯一出处。
   *
   * 收拢之前它们是 16 个内联 `<svg>` 散在 6 个文件里，而且**不是一套**：
   * `stroke-width` 全仓库有 6 个取值（1.2 / 1.25 / 1.3 / 1.4 / 1.5 / 2），
   * viewBox 有 12 / 16 / 20，渲染尺寸有 10 / 12 / 13 / 14 / 16，
   * 端点处理也各写各的 —— 搜索有 round cap、文件夹有 round join、Git 两个都没有。
   *
   * 单看每一个都挑不出毛病，但它们并排放在一条 34px 宽的导轨上时，
   * 差别是看得见的：文件夹明显比 Git 重，搜索的 1.4 描边比旁边的 1.3 更黑。
   * 这类问题不会被谁报成 bug，只是「看着不太对」。
   *
   * # 这一套的规矩
   *
   * - **一个网格**：`viewBox="0 0 16 16"`，字形都收在 2–14 这个 12×12 的
   *   视觉框里。不是「填满 16」—— 填满的那个（原来的文件夹）会显得比别人大一号。
   * - **一个描边**：1.25。粗细是图标里最容易被看出来的差异，
   *   一套里出现两个值就等于告诉人「这俩不是一家的」。
   * - **端点和拐角一律 round**。只有部分图标 round 的话，
   *   没 round 的那几个线头看着像被切掉了。
   * - **实心块只用来表示「哪一半是主体」**（侧边栏/底部面板那两个开关），
   *   一律 `opacity: 0.28` 的 currentColor —— 不另取颜色，
   *   这样跟着 hover / 选中态一起变。
   *
   * 加新图标先问一句：它能不能收进 2–14 这个框、只用 1.25 的描边画出来。
   * 画不出来的多半是想塞太多细节进 14px。
   */
  let { name, size = 14 }: { name: IconName; size?: number } = $props();
</script>

<svg
  viewBox="0 0 16 16"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width="1.25"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "sidebar"}
    <!-- 侧边栏开关：左侧那一块是实心的，表示"侧边栏在这边" -->
    <rect x="2" y="3" width="12" height="10" rx="2" />
    <path d="M6.4 3 V13" />
    <path d="M4 3 H6.4 V13 H4 A2 2 0 0 1 2 11 V5 A2 2 0 0 1 4 3 Z"
          fill="currentColor" stroke="none" opacity="0.28" />
  {:else if name === "files"}
    <!--
      文件夹。比原来那个小一圈 —— 它是这条导轨上唯一的大块闭合形状，
      按原尺寸画出来比旁边的 Git 重得多。
    -->
    <path d="M2.6 12.4 V4.6 a1.2 1.2 0 0 1 1.2-1.2 h2.3 l1.3 1.6 h4.8
             a1.2 1.2 0 0 1 1.2 1.2 v6.2 a1.2 1.2 0 0 1-1.2 1.2 H3.8
             a1.2 1.2 0 0 1-1.2-1.2 Z" />
  {:else if name === "git"}
    <!--
      分支：一条主干 + 从中段岔出去、拐上去接到第三个结点。
      原来那版用一条 Q 曲线从上面的结点斜拉到下面的结点，
      两头都是斜着扎进圆里的，看着像"连线"而不是"分支"。
    -->
    <circle cx="4.6" cy="3.4" r="1.7" />
    <circle cx="4.6" cy="12.6" r="1.7" />
    <circle cx="11.4" cy="3.4" r="1.7" />
    <path d="M4.6 5.1 V10.9" />
    <path d="M11.4 5.1 V6.6 a2.4 2.4 0 0 1-2.4 2.4 H4.6" />
  {:else if name === "search"}
    <circle cx="7" cy="7" r="4" />
    <path d="M9.95 9.95 L13.2 13.2" />
  {:else if name === "panel"}
    <!-- 底部面板开关：和 sidebar 同一个外框，实心块换到下面 -->
    <rect x="2" y="3" width="12" height="10" rx="2" />
    <path d="M2 9.6 H14" />
    <path d="M2 9.6 H14 V11 A2 2 0 0 1 12 13 H4 A2 2 0 0 1 2 11 Z"
          fill="currentColor" stroke="none" opacity="0.28" />
  {:else if name === "refresh"}
    <path d="M13 8 A5 5 0 1 1 11.4 4.3" />
    <path d="M13 2.6 V5.2 H10.4" />
  {:else if name === "check"}
    <path d="M3.6 8.3 L6.6 11.3 L12.4 5" />
  {:else if name === "plus"}
    <path d="M8 3.6 V12.4 M3.6 8 H12.4" />
  {:else if name === "warn"}
    <!--
      三角感叹号。Crash.svelte 原来自己画了一个 viewBox 20 的版本 ——
      同一个意思在两个网格上画两遍，粗细也不一样。
    -->
    <path d="M8 2.8 L14.4 13.4 H1.6 Z" />
    <path d="M8 6.9 V9.8" />
    <circle cx="8" cy="11.7" r="0.75" fill="currentColor" stroke="none" />
  {:else if name === "chevron-up"}
    <path d="M4.2 9.6 L8 5.8 L11.8 9.6" />
  {:else if name === "chevron-down"}
    <path d="M4.2 6.4 L8 10.2 L11.8 6.4" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    /* 图标只跟着字色走，不自己带颜色 —— hover / 选中态才不用各写一遍 */
    color: inherit;
  }
</style>
