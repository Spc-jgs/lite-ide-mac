<script lang="ts">
  /**
   * 行详情（#64）：日志视图底下展开的一整行。列表里行高固定、不折行（虚拟滚动的前提），比窗口宽的部分看不到 ——
   * 这里折行显示整行，行里有 JSON 就排好版（接口日志里一行一个响应体是常态）。
   * 超过 64KB 的行列表里本来就截了，这里另外取整行（`log_line_full`），最多 1MB；再长说真数字（「截断了要说出来」）
   */
  import Icon from "../shell/Icon.svelte";
  import { notify } from "../state/notify.svelte";
  import { extractJson, sizeOf } from "./line-detail";

  let {
    line,
    text,
    total = 0,
    shown = 0,
    loading = false,
    onclose,
  }: {
    /** 文件里的行号（1 起，和行号栏一致） */
    line: number;
    text: string;
    /** 这一行的真字节数；0 = 没另外取（没截过，`text` 就是整行） */
    total?: number;
    /** 取回来的字节数（截在 1MB 时比 `total` 小） */
    shown?: number;
    loading?: boolean;
    onclose: () => void;
  } = $props();

  const json = $derived(loading ? null : extractJson(text));
  const pretty = $derived(json ? JSON.stringify(json.value, null, 2) : "");
  const cut = $derived(total > shown);

  async function copy(s: string, what: string) {
    try {
      await navigator.clipboard.writeText(s);
      // 不把内容放进提示：一行可能有 1MB
      notify.ok(cut && what === "整行" ? `已复制这一行的前 ${sizeOf(shown)}（整行 ${sizeOf(total)}，剩下的没取）` : `已复制${what}（${s.length} 个字）`);
    } catch {
      notify.fail("剪贴板不可用，没复制成");
    }
  }
</script>

<section class="detail" aria-label="第 {line} 行的全文">
  <header>
    <span class="t">第 {line} 行</span>
    <span class="meta">
      {#if loading}正在取整行…{:else if cut}整行 {sizeOf(total)}，下面是前 {sizeOf(shown)}{:else}{text.length} 个字{/if}
    </span>
    <span class="gap"></span>
    <span class="keys">↑↓ 上下一行 · Esc 关掉</span>
    {#if json}<button class="btn sm" onclick={() => void copy(pretty, " JSON")}>复制 JSON</button>{/if}
    <button class="btn sm" disabled={loading} onclick={() => void copy(text, "整行")}>复制整行</button>
    <button class="ibtn sm" title="关掉（Esc）" aria-label="关掉行详情" onclick={onclose}><Icon name="x" /></button>
  </header>
  <div class="body">
    {#if loading}
      <span class="spinner sm"></span>
    {:else if json}
      {#if json.before.trim()}<pre class="txt">{json.before}</pre>{/if}
      <pre class="json">{pretty}</pre>
      {#if json.after.trim()}<pre class="txt">{json.after}</pre>{/if}
    {:else}
      <pre class="txt">{text}</pre>
    {/if}
  </div>
</section>

<style>
  .detail {
    flex: none;
    display: flex;
    flex-direction: column;
    max-height: 42%;
    min-height: 96px;
    border-top: 1px solid var(--border);
    background: var(--panel-bg, var(--editor-bg));
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 8px 0 12px;
    border-bottom: 1px solid var(--border-soft);
    font-size: var(--fs-sm);
  }
  .t { font-weight: 600; color: var(--text); }
  .meta { color: var(--text-muted); }
  .gap { flex: 1; }
  .keys { color: var(--text-faint); font-size: var(--fs-xs); }
  .body { flex: 1; min-height: 0; overflow: auto; padding: 8px 12px 12px; }
  pre {
    margin: 0;
    font-family: var(--code-font);
    font-size: var(--fs-md);
    line-height: 1.5;
    /* 折行：这里就是为了看全，横着滚又回到了「看不到后半截」 */
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--text);
    user-select: text;
  }
  .txt { color: var(--text-dim); }
  .json { margin: 4px 0; padding: 6px 8px; border-radius: var(--r-sm); background: var(--hover); }
</style>
