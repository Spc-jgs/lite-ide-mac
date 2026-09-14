<script lang="ts">
  /**
   * 五个浮层：快捷键速查、文件结构大纲、随处搜索、编码选择器、分支与工作树。
   * 开合状态在 `state/overlay.svelte.ts`（开它们的人散在键盘分派、菜单、导轨、
   * 状态栏、Git 栏、标题栏），这里只管加载和渲染。
   *
   * 从 App.svelte 搬出来（issue #9 第 6 步）。分支浮层属于 Git 那组 `lazyGroup`
   * （App 还有别的组件用它），以组件类型传进来；`actions` 是随处搜索里的
   * 「操作」表，它的 `run` 要调 App 的 `runMenu`，所以在 App 里生成。
   */
  import type BranchPicker from "../git/BranchPicker.svelte";
  import type { Action } from "../search/QuickSearch.svelte";
  import { lazy, lazyGroup } from "../lazy/lazy.svelte";
  import { notify } from "../state/notify.svelte";
  import { tabs } from "../state/tabs.svelte";
  import { docs } from "../state/docs.svelte";
  import { project } from "../state/project.svelte";
  import { git } from "../state/git.svelte";
  import { branches } from "../state/branches.svelte";
  import { nav } from "../state/nav.svelte";
  import { overlay } from "../state/overlay.svelte";
  import { lang } from "../state/lang.svelte";

  let {
    Branch,
    actions,
  }: {
    Branch: typeof BranchPicker | undefined;
    actions: Action[];
  } = $props();

  /**
   * 两个搜索浮层（⌘P 随处搜索、⌘⇧O 文件结构）。
   *
   * 一起拉是因为**它们共用 `fuzzy.ts`** —— 分两次的话那份排序算法要么进公共块、
   * 要么各带一份，而两个浮层本来就是同一类东西（键盘唤出、盖在界面上）。
   *
   * 挪出入口包**实测省 10,183 字节**（挪走前后各量一次，不是按 sourcemap 归因
   * 的估值 —— 归因会高估数据密集的模块，见 .claude/rules/frontend.md）。
   * 判据同 `keysPanel`：问一句「这东西在窗口出现之前有用吗」——
   * 没有，它们都得等一次按键。
   *
   * 但和速查表不同的是，**这两个是天天按的**，不能让第一次 ⌘P 等一次 chunk 往返。
   * 所以首屏画完之后就预拉（见下面那个 setTimeout）：既不占首屏之前那段，
   * 又保证人真按下去的时候它已经在了。
   */
  const overlays = lazyGroup(
    {
      quick: () => import("../search/QuickSearch.svelte"),
      outline: () => import("../search/Outline.svelte"),
    },
    "搜索浮层",
  );

  $effect(() => {
    // 兜底：预拉万一没跑到（或者失败过），真按下去时补一次。
    // `load()` 是幂等的，重复调用会被它自己的状态挡掉
    if (overlay.quickOpen || overlay.outlineOpen) overlays.load();
  });

  $effect(() => {
    /*
     * 首屏之后再拉。
     *
     * 300ms 不是随便取的：它要**明确落在首屏绘制之后**（否则等于没挪出去），
     * 又要远早于人按下第一个 ⌘P。用 `setTimeout` 而不是
     * `requestIdleCallback` —— 后者 Safari 16.4 才有，而构建目标是 safari15。
     */
    const id = setTimeout(() => overlays.load(), 300);
    return () => clearTimeout(id);
  });

  const encPicker = lazy(() => import("../encoding/EncodingPicker.svelte"), "编码选择器");

  /*
   * 速查表是「忘了才看」的东西，一次都不点开也很正常 ——
   * 让它在首屏之前被解析执行不划算。判据同 ARCHITECTURE 那条：
   * 问一句「这东西在窗口出现之前有用吗」。
   */
  const keysPanel = lazy(() => import("../search/Keys.svelte"), "快捷键速查");

  $effect(() => {
    if (overlay.encOpen) encPicker.load();
  });
  $effect(() => {
    if (overlay.keysOpen) keysPanel.load();
  });
  // 按需加载失败要说出来（App 那张汇总名单的本地版，同 Panel / Content）
  $effect(() => {
    const e = overlays.error || encPicker.error || keysPanel.error;
    if (e) notify.fail(e);
  });

  /** 走 legacy stream parser 的语言没有语法树，界面要明说 */
  const LEZER_LANGS = new Set([
    "java", "javascript", "typescript", "python", "markdown", "json", "rust",
    "yaml", "html", "css", "sass", "less", "xml", "sql", "cpp", "php", "vue", "liquid",
  ]);

  let outlineSupported = $derived(
    tabs.active?.mode === "edit" && !!lang.mod && LEZER_LANGS.has(lang.mod.langOf(tabs.active.path) ?? ""),
  );
</script>

{#if keysPanel.comp}
  <keysPanel.comp bind:open={overlay.keysOpen} />
{/if}

{#if overlays.comps.outline}
  <overlays.comps.outline
    bind:open={overlay.outlineOpen}
    symbols={overlay.symbols}
    fileName={tabs.active?.name ?? ""}
    supported={outlineSupported}
    onPick={(line) => (nav.goto(line))}
  />
{/if}

{#if overlays.comps.quick}
  <overlays.comps.quick
    bind:open={overlay.quickOpen}
    bind:scope={overlay.quickScope}
    seed={overlay.quickSeed}
    root={project.root}
    {actions}
    onOpenFile={(p, l) => nav.openAt(p, l)}
  />
{/if}

{#if encPicker.comp && tabs.active}
  <encPicker.comp
    bind:open={overlay.encOpen}
    current={tabs.active.encoding ?? "UTF-8"}
    bom={!!tabs.active.bom}
    lossy={!!tabs.active.lossy}
    readonly={tabs.active.mode !== "edit"}
    onReopen={(l) => void docs.reopenWith(l)}
    onSaveAs={(...a) => docs.saveAsEncoding(...a)}
  />
{/if}

{#if Branch && git.repo}
  <Branch
    bind:open={overlay.branchOpen}
    anchor={overlay.branchAnchor}
    repo={git.repo}
    ahead={git.status?.ahead ?? 0}
    behind={git.status?.behind ?? 0}
    onSwitch={(n) => branches.switchTo(n)}
    onNewBranch={(n) => branches.switchTo(n, true)}
    onOpenWorktree={(p) => void branches.openWorktree(p)}
    onNewWorktree={(...a) => branches.newWorktree(...a)}
    onRemoveWorktree={(w) => (branches.pendingWtRemove = w)}
  />
{/if}



