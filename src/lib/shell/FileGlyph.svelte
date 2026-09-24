<script module lang="ts">
  /**
   * 文件 / 目录图标 —— 全应用唯一的出处（文件树、标签栏、随处搜索都用它）。
   *
   * **照 IDEA 按类型上色**（2026-09-24）：JetBrains 新 UI 的官方文件类型图标，文件在
   * `public/icons/file/`（来源和许可见 `public/icons/SOURCES.md`）。
   *
   * 原来是五类单色描边，理由是「颜色这条通道已经被 git 状态占了」。那条理由站不住：
   * IDEA 的图标颜色说类型、**文件名**的颜色说 git 状态，两个通道在两个元素上，互不打架 ——
   * 我们的 git 状态本来也是染在文件名上的。
   *
   * 表里没有的扩展名画纯文本图标。Python、Kotlin 的取自各自插件目录，Rust、Go 上游没有新 UI 版，
   * 用的是 `platform/icons/src/language/` 里那份（已经是 16px 扁平风格）。
   */
  import type { DirKind } from "./tree-rows";

  /** 扩展名 → 图标。只列有图标的；没列的画纯文本 */
  const BY_EXT: Record<string, string> = {
    java: "java",
    py: "python", pyi: "python", pyw: "python",
    kt: "kotlin", kts: "kotlin",
    rs: "rust",
    go: "go",
    js: "javaScript", mjs: "javaScript", cjs: "javaScript", jsx: "javaScript",
    ts: "typeScript", mts: "typeScript", cts: "typeScript", tsx: "typeScript",
    vue: "vue",
    json: "json", jsonc: "json", json5: "json",
    yaml: "yaml", yml: "yaml",
    xml: "xml", xsd: "xml",
    md: "markdown", markdown: "markdown",
    properties: "properties",
    sql: "sql",
    sh: "shell", bash: "shell", zsh: "shell", fish: "shell",
    html: "html", htm: "html",
    css: "css", scss: "css", sass: "css", less: "css",
    csv: "csv", tsv: "csv",
    toml: "toml",
    gradle: "gradle",
    c: "c",
    cpp: "cpp", cc: "cpp", cxx: "cpp", hpp: "cpp",
    h: "h",
    png: "image", jpg: "image", jpeg: "image", gif: "image", svg: "image", webp: "image", ico: "image",
    zip: "archive", jar: "archive", war: "archive", tar: "archive", gz: "archive", tgz: "archive", "7z": "archive",
    ini: "config", conf: "config", cfg: "config", env: "config", plist: "config", lock: "config",
  };
  /** 整个文件名说了算的（没有扩展名、或扩展名不说明问题） */
  const BY_NAME: Record<string, string> = {
    dockerfile: "docker",
    ".gitignore": "gitignore",
    ".gitattributes": "gitignore",
    ".editorconfig": "editorConfig",
    ".env": "config",
    ".npmrc": "config",
    ".nvmrc": "config",
    "build.gradle.kts": "gradle",
    "settings.gradle.kts": "gradle",
  };
  const BY_DIR: Record<DirKind, string> = {
    folder: "folder",
    source: "sourceRoot",
    test: "testRoot",
    resources: "resourcesRoot",
    testResources: "testResourcesRoot",
    package: "package",
    excluded: "excludeRoot",
  };

  export function fileIconOf(name: string, isDir = false, kind: DirKind = "folder"): string {
    if (isDir) return BY_DIR[kind];
    const lower = name.toLowerCase();
    const whole = BY_NAME[lower];
    if (whole) return whole;
    const dot = lower.lastIndexOf(".");
    return (dot > 0 && BY_EXT[lower.slice(dot + 1)]) || "text";
  }
</script>

<script lang="ts">
  let {
    name,
    isDir = false,
    kind = "folder",
    size = 16,
  }: { name: string; isDir?: boolean; kind?: DirKind; size?: number } = $props();
</script>

<!-- 彩色的，画原色：`<img>`（单色的界面图标才走 mask，见 Icon.svelte） -->
<img
  class="glyph"
  src="/icons/file/{fileIconOf(name, isDir, kind)}.svg"
  width={size}
  height={size}
  alt=""
  aria-hidden="true"
  draggable="false"
/>

<style>
  .glyph { flex: none; display: block; }
</style>
