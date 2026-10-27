<script lang="ts">
  /**
   * 书签工具窗（⌘F3，docs/TAKEOVER.md G18，IDEA 的 Bookmarks）：这个项目里打过的全部书签，按文件分组，带那一行的字；
   * 点一行跳过去，✕ 删掉。编辑器里 F3 打 / 取消（`editor/bookmarks-ext.ts`）。
   * 行号是跟着改动走过的（编辑器写回的）；文件在编辑器外面被改了，书签还停在原来的行号上 —— 照 IDEA，不去猜
   */
  import { allOf, marksOf, onBookmarks, setMarks, type Entry } from "../state/bookmarks";
  import { project } from "../state/project.svelte";
  import { nav } from "../state/nav.svelte";
  import Icon from "./Icon.svelte";

  let tick = $state(0);
  $effect(() => onBookmarks(() => tick++));
  const list = $derived((void tick, allOf(project.root)));
  const groups = $derived.by(() => {
    const m = new Map<string, Entry[]>();
    for (const e of list) m.set(e.path, [...(m.get(e.path) ?? []), e]);
    return [...m];
  });
  const rel = (p: string) => (project.root && p.startsWith(`${project.root}/`) ? p.slice(project.root.length + 1) : p);
  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

  function remove(e: Entry) {
    setMarks(project.root, e.path, marksOf(project.root, e.path).filter((m) => m.line !== e.line));
  }
  function clearAll() {
    for (const [p] of groups) setMarks(project.root, p, []);
  }
</script>

<div class="bms">
  <div class="bar">
    <span class="cnt">{list.length} 个书签 · F3 在光标那行打 / 取消</span>
    <span class="gap"></span>
    <button class="btn sm quiet" disabled={!list.length} onclick={clearAll}>全部删掉</button>
  </div>
  <div class="list" role="tree" aria-label="书签">
    {#each groups as [path, marks] (path)}
      <div class="file" title={path}><span class="fn">{base(path)}</span><span class="dir">{rel(path).slice(0, -base(path).length).replace(/\/$/, "")}</span></div>
      {#each marks as e (e.line)}
        <div class="row" role="treeitem" aria-selected="false" tabindex="-1" onclick={() => void nav.openAt(e.path, e.line, false)} onkeydown={() => {}} title="跳到第 {e.line} 行">
          <span class="mk"><Icon name="bookmark" /></span>
          <span class="ln">{e.line}</span>
          <span class="txt">{e.text}</span>
          <button
            class="ibtn sm x"
            aria-label="删掉第 {e.line} 行的书签"
            title="删除此书签"
            onclick={(ev) => {
              ev.stopPropagation();
              remove(e);
            }}><Icon name="x" /></button>
        </div>
      {/each}
    {:else}
      <div class="empty-msg">此项目尚无书签<span class="sub">在编辑器中按 F3 为光标所在行添加书签，书签将列在此处；单击即可跳转</span></div>
    {/each}
  </div>
</div>

<style>
  .bms { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .bar { flex: none; display: flex; align-items: center; gap: 8px; height: 34px; padding: 0 8px 0 12px; border-bottom: 1px solid var(--border-soft); font-size: var(--fs-sm); color: var(--text-faint); }
  .gap { flex: 1; }
  .list { flex: 1; min-height: 0; overflow: auto; padding: 4px 6px; }
  .file { display: flex; align-items: baseline; gap: 8px; padding: 6px 6px 2px; font-size: var(--fs-md); color: var(--text); }
  .dir { font-size: var(--fs-sm); color: var(--text-faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; align-items: center; gap: 8px; height: 24px; padding: 0 6px 0 14px; border-radius: var(--r-sm); cursor: default; }
  .row:hover { background: var(--hover); }
  .mk { flex: none; display: grid; place-items: center; }
  .ln { flex: none; min-width: 32px; text-align: right; font-size: var(--fs-sm); color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .txt { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text-dim); }
  .x { visibility: hidden; }
  .row:hover .x { visibility: visible; }
</style>
