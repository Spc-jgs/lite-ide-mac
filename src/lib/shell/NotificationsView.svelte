<script lang="ts">
  /**
   * 通知记录（docs/TAKEOVER.md I11，IDEA 的 Notifications 工具窗）：这次会话里胶囊和卡片说过的每一句，新的在上。
   *
   * 胶囊几秒就走，被没读完的失败压下去的那条（「已保存」「已推送」）根本没露面 —— 不打断的代价是「错过了」，这里补看。
   * 开着的时候来了新的，导轨上铃的角标不涨（看着呢）；字能选、能复制（报错的原话常要贴给别人）。
   * 只在内存里、按窗口各一份：重启之后的「上次出过什么错」去应用日志里看（帮助菜单）
   */
  import { untrack } from "svelte";
  import { notify, type LogItem } from "../state/notify.svelte";
  import Icon from "./Icon.svelte";

  const items = $derived([...notify.log].reverse());

  // 开着就算看过了：新来的坏消息也不攒角标
  $effect(() => {
    void notify.log.length;
    untrack(() => (notify.unseen = 0));
  });

  const hhmmss = (t: number) => new Date(t).toTimeString().slice(0, 8);
  const ICON: Record<LogItem["kind"], "check" | "x" | null> = { ok: "check", fail: "x", block: "x", hint: null };
</script>

<div class="notes">
  <div class="bar">
    <span class="cnt">{items.length} 条 · 这次打开以来，新的在上</span>
    <span class="gap"></span>
    <button class="btn sm quiet" disabled={!items.length} onclick={() => (notify.log = [])}>清空</button>
  </div>
  <div class="list">
    {#each items as n (n.id)}
      {@const nl = n.kind === "block" ? n.text.indexOf("\n") : -1}
      <div class="row {n.kind}">
        <span class="at">{hhmmss(n.at)}</span>
        <span class="mk">{#if ICON[n.kind]}<Icon name={ICON[n.kind]!} />{/if}</span>
        <span class="txt">{#if nl >= 0}<b>{n.text.slice(0, nl)}</b>{"\n" + n.text.slice(nl + 1)}{:else}{n.text}{/if}</span>
      </div>
    {:else}
      <div class="empty-msg">自本次启动以来没有消息<span class="sub">操作成功、失败及无变化的提示都会记录在此，提示消失后仍可查看</span></div>
    {/each}
  </div>
</div>

<style>
  .notes { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .bar { flex: none; display: flex; align-items: center; gap: 8px; height: 34px; padding: 0 8px 0 12px; border-bottom: 1px solid var(--border-soft); font-size: var(--fs-sm); color: var(--text-faint); }
  .gap { flex: 1; }
  .list { flex: 1; min-height: 0; overflow: auto; padding: 4px 6px; }
  .row { display: flex; align-items: flex-start; gap: 8px; padding: 3px 6px; border-radius: var(--r-sm); font-size: var(--fs-md); line-height: 1.45; }
  .row:hover { background: var(--hover); }
  .at { flex: none; font-size: var(--fs-sm); color: var(--text-faint); font-variant-numeric: tabular-nums; padding-top: 1px; }
  .mk { flex: none; width: 16px; height: 18px; display: grid; place-items: center; }
  .ok .mk { color: var(--accent); }
  .fail .mk, .block .mk { color: var(--lvl-error); }
  .txt { flex: 1; min-width: 0; white-space: pre-wrap; word-break: break-word; color: var(--text); user-select: text; }
  .hint .txt { color: var(--text-muted); }
  .txt b { font-weight: 600; }
</style>
