<script lang="ts">
  /**
   * 欢迎页（docs/TAKEOVER.md G27，IDEA 的 Welcome 窗口）：没开项目、一个标签都没有时内容区里那一块。
   * 原来是一张卡片 + 一个「最近：xxx」链接 —— 最近项目只露一个，别的要去标题栏的项目挂件里翻；没有克隆。
   *
   * 现在：新建草稿 / 打开文件夹 / 从 Git 克隆三个动作，下面是最近项目（名字、路径、现在在哪个分支），
   * 多了就有一个筛选框；↑↓ 挑、↵ 开，悬停出 ✕ 从名单里拿掉。
   *
   * 懒加载（Content 里一个 `lazy()`）：开着项目的窗口永远用不上它，原来那张卡片的模板一直跟着入口包。
   * 分支是 Rust 读 `.git/HEAD`（不起 git、不过信任检查，别人的目录里 git 的配置能执行命令），一个项目一次调用：
   * 网络卷上没挂载的那个卡住，别的照常出来；名字和路径不等它
   */
  import { untrack } from "svelte";
  import { project } from "../state/project.svelte";
  import { tabflow } from "../state/tabflow.svelte";
  import { forgetRecent } from "../ipc/commands";
  import { imeKey } from "../shell/ime";
  import Icon from "../shell/Icon.svelte";
  import { openModal } from "../settings/modal-host";
  import CloneDialog from "./CloneDialog.svelte";
  import { baseName, filterRecent, initial, recentBranch, tildify } from "./welcome";
  import { byId, onOverrides, type KeyDef } from "../state/keymap";

  let query = $state("");
  let sel = $state(0);
  let branches = $state<Record<string, string | null>>({});
  const list = $derived(filterRecent(project.recent, query));

  $effect(() => {
    const paths = project.recent;
    let dead = false;
    for (const p of paths) {
      if (untrack(() => p in branches)) continue;
      void recentBranch(p)
        .catch(() => null)
        .then((b) => {
          if (!dead) branches = { ...branches, [p]: b };
        });
    }
    return () => (dead = true);
  });
  // 筛完了选中项还要在列表里
  $effect(() => {
    if (sel >= list.length) sel = Math.max(0, list.length - 1);
  });

  // 键位从 keymap.ts 来、跟着你改的变（同空态卡片那份，R3）
  let keys = $state(0);
  $effect(() => onOverrides(() => keys++));
  const accel = (id: string) => (void keys, byId(id)?.accel ?? "");
  const HINTS = ["quick-all", "quick-file", "recent-files", "toggle-panel"];
  const hints = $derived((void keys, HINTS.map((id) => byId(id)).filter((k): k is KeyDef => !!k)));

  function open(p: string) {
    void tabflow.openRecent(p);
  }
  async function forget(p: string) {
    project.recent = project.recent.filter((r) => r !== p);
    project.recent = (await forgetRecent(p).catch(() => null))?.projects ?? project.recent;
  }
  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (list.length) sel = (sel + (e.key === "ArrowDown" ? 1 : list.length - 1)) % list.length;
    } else if (e.key === "Enter" && list[sel]) {
      e.preventDefault();
      open(list[sel]);
    } else if (e.key === "Escape" && query) {
      e.preventDefault();
      query = "";
    }
  }
</script>

<div class="welcome">
  <div class="big">新建草稿，或打开项目</div>
  <p class="sub">草稿无需命名和保存；可将文件或文件夹拖入窗口打开。代码以编辑模式打开，大文件和日志自动以只读的日志模式打开</p>
  <div class="go">
    <button class="btn primary" onclick={() => void tabflow.newScratch()}>新建草稿</button>
    {#if accel("new-scratch")}<kbd>{accel("new-scratch")}</kbd>{/if}
    <button class="btn" onclick={() => void tabflow.openFolder()}>打开文件夹…</button>
    <!-- 没项目时 ⌘O（找类那个键）就是打开文件夹（keymap.ts 里 quick-class 那条）；你在设置里给「打开文件夹」另配了键，就印那个 -->
    {#if accel("open-folder") || accel("quick-class")}<kbd>{accel("open-folder") || accel("quick-class")}</kbd>{/if}
    <button class="btn" onclick={() => openModal(CloneDialog, { recent: project.recent })}>从 Git 克隆…</button>
  </div>

  {#if project.recent.length}
    <div class="sec">
      <span class="label">最近的项目</span>
      {#if project.recent.length > 5}
        <input class="filter" placeholder="筛选…" spellcheck="false" bind:value={query} onkeydown={onKey} aria-label="筛选最近的项目" />
      {/if}
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <ul class="recent" role="listbox" aria-label="最近的项目" tabindex="0" onkeydown={onKey}>
      {#each list as p, i (p)}
        {@const b = branches[p]}
        <li
          class="row"
          class:sel={i === sel}
          role="option"
          aria-selected={i === sel}
          title={p}
          onmouseenter={() => (sel = i)}
          onclick={() => open(p)}
          onkeydown={() => {}}
        >
          <span class="sq" aria-hidden="true">{initial(p)}</span>
          <span class="txt">
            <span class="name">{baseName(p)}{#if b}<span class="br"><Icon name="git" />{b}</span>{/if}</span>
            <span class="path">{tildify(p)}</span>
          </span>
          <button
            class="ibtn sm x"
            aria-label="从最近的项目中移除 {baseName(p)}"
            title="从最近的项目中移除（不影响磁盘上的文件）"
            onclick={(e) => {
              e.stopPropagation();
              void forget(p);
            }}><Icon name="x" /></button>
        </li>
      {:else}
        <li class="none">没有名字或路径里带「{query}」的项目</li>
      {/each}
    </ul>
  {/if}

  <div class="keymap">
    {#each hints as k (k.id)}
      <span><b>{k.gesture ?? k.accel}</b> {k.label}</span>
    {/each}
  </div>
</div>

<style>
  .welcome { width: min(560px, calc(100% - 48px)); margin: min(12vh, 96px) auto 24px; display: flex; flex-direction: column; gap: 10px; }
  .big { font-size: var(--fs-lg); font-weight: 600; color: var(--text); }
  .sub { margin: 0; font-size: var(--fs-md); color: var(--text-muted); line-height: 1.5; }
  .go { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; margin-top: 4px; }
  .go kbd { margin-right: 8px; font-size: var(--fs-sm); color: var(--text-faint); font-family: var(--ui-font); }
  .sec { display: flex; align-items: center; gap: 8px; margin-top: 18px; min-height: 28px; }
  .label { flex: 1; font-size: var(--fs-xs); letter-spacing: 0.04em; color: var(--text-faint); }
  .filter { width: 200px; height: 24px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-size: var(--fs-sm); outline: none; }
  .filter:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .recent { list-style: none; margin: 0 -8px; padding: 0; max-height: min(46vh, 420px); overflow: auto; outline: none; }
  .row { display: flex; align-items: center; gap: 10px; padding: 5px 8px; border-radius: var(--r-sm); cursor: pointer; }
  .row.sel { background: var(--hover); }
  .recent:focus-visible .row.sel { background: var(--selected); }
  .sq { flex: none; display: grid; place-content: center; width: 28px; height: 28px; border-radius: var(--r-xs); background: var(--selected); color: var(--text); font-size: var(--fs-md); font-weight: 600; }
  .txt { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .name { display: flex; align-items: center; gap: 8px; font-size: var(--fs-md); color: var(--text); white-space: nowrap; overflow: hidden; }
  .br { display: inline-flex; align-items: center; gap: 3px; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-size: var(--fs-sm); color: var(--text-faint); }
  .path { font-size: var(--fs-sm); color: var(--text-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .x { visibility: hidden; }
  .row:hover .x, .row.sel .x { visibility: visible; }
  .none { padding: 8px; font-size: var(--fs-sm); color: var(--text-faint); }
  .keymap { display: grid; grid-template-columns: 1fr 1fr; gap: 4px 16px; margin-top: 18px; font-size: var(--fs-sm); color: var(--text-faint); }
  .keymap b { font-weight: normal; color: var(--text-dim); margin-right: 6px; }
</style>
