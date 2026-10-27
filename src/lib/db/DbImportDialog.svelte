<script lang="ts">
  /**
   * 从 IDEA 导入数据库连接（docs/DATABASE.md 第 4 节，DB-4）：列出项目 `.idea/dataSources.xml` 里的数据源，勾上的导进来。
   * 认不得的库、同名已有的，列着、灰掉、说清为什么（不悄悄少几个）。密码不带：IDEA 把它存在自己的钥匙串条目里，导入后在「编辑连接」里填一次
   */
  import { untrack } from "svelte";
  import type { DbIdea } from "../ipc/commands";
  import { dbIdeaImport } from "../ipc/db";
  import { notify } from "../state/notify.svelte";

  let { root, found, problems, ondone, onclose }: { root: string; found: DbIdea[]; problems: string[]; ondone: () => void; onclose: () => void } = $props();

  const can = (d: DbIdea) => !!d.kind && !d.exists;
  // 打开时的一份（`untrack`：有意只取初值）：能导的默认都勾上
  let picked = $state(new Set(untrack(() => found.filter(can).map((d) => d.uuid))));
  let busy = $state(false);

  function toggle(uuid: string) {
    const next = new Set(picked);
    if (next.has(uuid)) next.delete(uuid);
    else next.add(uuid);
    picked = next;
  }

  async function go() {
    if (!picked.size || busy) return;
    busy = true;
    try {
      const n = await dbIdeaImport(root, [...picked]);
      notify.ok(`已从 IDEA 导入 ${n} 个连接。密码未导入，首次连接前请在「编辑连接」中填写`, 8000);
      ondone();
      onclose();
    } catch (e) {
      notify.fail(`导入失败：${String(e)}`);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  const why = (d: DbIdea) => (d.exists ? "已存在同名连接" : !d.kind ? (d.note ?? "暂不支持") : d.note);
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog wide dbimport" role="dialog" aria-label="从 IDEA 导入连接" tabindex="-1" onkeydown={onKey}>
    <p class="title">从 IDEA 导入数据库连接</p>
    <p class="body">来自项目的 .idea/dataSources.xml。密码不会导入（IDEA 将密码保存在它自己的钥匙串条目中），首次连接前请在「编辑连接」中填写。</p>
    <div class="list" role="list">
      {#each found as d (d.uuid)}
        <label class="item" class:off={!can(d)} role="listitem">
          <!-- bind:checked 的读 / 写写法，不写 checked={…}：后者会把 Svelte 运行时里处理属性的那段拽进入口包（frontend.md） -->
          <input type="checkbox" disabled={!can(d)} bind:checked={() => picked.has(d.uuid), () => toggle(d.uuid)} aria-label="导入 {d.name}" />
          <span class="main">
            <span class="name">{d.name}{#if d.user}<span class="user">{d.user}</span>{/if}</span>
            <span class="url">{d.url}</span>
            {#if why(d)}<span class="why" class:bad={!can(d)}>{why(d)}</span>{/if}
          </span>
        </label>
      {/each}
    </div>
    {#each problems as p (p)}<p class="why bad">{p}</p>{/each}
    <div class="actions">
      <span class="gap"></span>
      <button class="btn" data-dismiss onclick={onclose}>取消</button>
      <button class="btn primary" data-default class:busy disabled={busy || !picked.size} onclick={() => void go()}>导入 {picked.size} 个连接</button>
    </div>
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .dbimport { width: min(640px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 10px; }
  .body { margin: 0; font-size: var(--fs-sm); color: var(--text-muted); }
  .list { display: flex; flex-direction: column; gap: 2px; max-height: 50vh; overflow: auto; }
  .item { display: flex; align-items: flex-start; gap: 8px; padding: 6px 8px; border-radius: var(--r-sm); }
  .item:hover { background: var(--hover); }
  .item.off { opacity: 0.6; }
  .item input { margin-top: 2px; }
  .main { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .name { font-size: var(--fs-md); color: var(--text); }
  .user { margin-left: 8px; font-size: var(--fs-sm); color: var(--text-faint); }
  .url { font-family: var(--code-font); font-size: var(--fs-xs); color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .why { margin: 0; font-size: var(--fs-xs); color: var(--text-faint); }
  .why.bad { color: var(--lvl-warn); }
</style>
