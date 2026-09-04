<script module lang="ts">
  /**
   * 文件类型字形 —— 全应用唯一的出处。
   *
   * 原来这套分类和五个 `<path>` 内联在 `FileTree.svelte` 里。收出来是因为
   * 标签栏和随处搜索也要用同一套：**同一个文件在三个地方出现，不能长三个样。**
   * （这和「图标收编」是同一条规矩 —— 见 `Icon.svelte` 顶上那段。）
   *
   * **单色描边，不是彩色图标包** —— 颜色这条通道已经被 git 状态占了
   * （改动蓝、新增绿、未跟踪灰绿…），再叠一层彩色图标两边都读不清。
   * 只有配置类给一点 warn 黄，因为改错它的代价最大。
   *
   * 五类就够：目录 / 代码 / 标记文档 / 配置 / 纯文本。分得再细是给自己找活儿。
   */
  export type Glyph = "dir" | "code" | "doc" | "conf" | "text";

  const CONF_EXT = new Set([
    "json", "yaml", "yml", "toml", "ini", "conf", "cfg", "properties", "env",
    "lock", "plist", "xml",
  ]);
  const CONF_NAME = new Set([
    "dockerfile", "makefile", "gemfile", "rakefile", "procfile", "justfile",
    ".gitignore", ".gitattributes", ".editorconfig", ".npmrc", ".nvmrc",
  ]);
  const DOC_EXT = new Set(["md", "markdown", "rst", "adoc", "org"]);
  const TEXT_EXT = new Set(["txt", "log", "csv", "tsv", "out"]);

  export function glyphOf(name: string, isDir = false): Glyph {
    if (isDir) return "dir";
    const lower = name.toLowerCase();
    if (CONF_NAME.has(lower)) return "conf";
    const dot = lower.lastIndexOf(".");
    // 没有扩展名的多半是脚本或 README 之类，按纯文本处理
    if (dot <= 0) return "text";
    const ext = lower.slice(dot + 1);
    if (CONF_EXT.has(ext)) return "conf";
    if (DOC_EXT.has(ext)) return "doc";
    if (TEXT_EXT.has(ext)) return "text";
    return "code";
  }
</script>

<script lang="ts">
  import Icon from "./Icon.svelte";

  let {
    name,
    isDir = false,
    size = 14,
  }: { name: string; isDir?: boolean; size?: number } = $props();

  let gl = $derived(glyphOf(name, isDir));
</script>

<!--
  文件夹**必须**走 Icon 里那一个形状：导轨上的「文件树」按钮画的是同一样东西，
  两处各画一遍，改一处就分叉。其余四类是「按扩展名分色」的字形，
  不是一个家族，不必强行统一。
-->
{#if gl === "dir"}
  <span class="glyph dir"><Icon name="files" {size} /></span>
{:else}
  <svg class="glyph {gl}" viewBox="0 0 16 16" width={size} height={size} aria-hidden="true">
    {#if gl === "code"}
      <path d="M6 3.2 L3 8 L6 12.8" fill="none" stroke="currentColor" stroke-width="1.25"
            stroke-linecap="round" stroke-linejoin="round" />
      <path d="M10 3.2 L13 8 L10 12.8" fill="none" stroke="currentColor" stroke-width="1.25"
            stroke-linecap="round" stroke-linejoin="round" />
    {:else if gl === "doc"}
      <rect x="2.6" y="3.4" width="10.8" height="9.2" rx="1"
            fill="none" stroke="currentColor" stroke-width="1.25" />
      <path d="M4.8 6.6 h3.2 M4.8 9.4 h6" stroke="currentColor" stroke-width="1.25"
            stroke-linecap="round" />
    {:else if gl === "conf"}
      <circle cx="8" cy="8" r="2.1" fill="none" stroke="currentColor" stroke-width="1.25" />
      <path d="M8 1.9 v1.6 M8 12.5 v1.6 M1.9 8 h1.6 M12.5 8 h1.6 M3.7 3.7 l1.1 1.1 M11.2 11.2 l1.1 1.1 M12.3 3.7 l-1.1 1.1 M4.8 11.2 l-1.1 1.1"
            stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
    {:else}
      <path d="M4.2 2.8 h5.2 l3 3 v7.6 a.9 .9 0 0 1-.9 .9 H4.2 a.9 .9 0 0 1-.9-.9 V3.7 a.9 .9 0 0 1 .9-.9 z"
            fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
      <path d="M9.2 2.9 v3.1 h3.1" fill="none" stroke="currentColor" stroke-width="1.25"
            stroke-linecap="round" />
    {/if}
  </svg>
{/if}

<style>
  .glyph { flex: none; display: flex; color: var(--text-faint); }
  /* 配置类给一点 warn 黄：改错它的代价最大。0.75 是为了不跟 git 状态色抢 */
  .glyph.conf { color: var(--lvl-warn); opacity: 0.75; }
</style>
