<script lang="ts">
  /**
   * 数据库连接的新建 / 编辑（docs/DATABASE.md DB-1）：类型、主机、端口、库、用户、密码，H2 填完整的连接串；「测试连接」当场连一次、结果说在对话框里。
   * 密码只往钥匙串里存，不回到这个页面：存过的显示「已保存在钥匙串中」，不填就是不改，填了就是换掉
   */
  import { untrack } from "svelte";
  import type { DbConn } from "../ipc/commands";
  import { dbDelete, dbSave, dbTest } from "../ipc/db";
  import { imeKey } from "../shell/ime";

  let { root, conn, onsaved, onclose }: { root: string; conn: DbConn | null; onsaved: () => void; onclose: () => void } = $props();

  const KINDS = [
    { id: "mysql", label: "MySQL", port: 3306 },
    { id: "postgresql", label: "PostgreSQL", port: 5432 },
    { id: "h2", label: "H2", port: 0 },
  ] as const;

  // 编辑的是打开那一刻的一份拷贝：取消就是什么都不动（`untrack`：有意只取初值）
  const init = untrack(() => conn);
  let c = $state<DbConn>(init ? { ...init } : { id: "", name: "", kind: "mysql", host: "localhost", port: 3306, database: "", user: "root", url: "", hasPassword: false });
  let password = $state("");
  let busy = $state(false);
  let result = $state<{ ok: boolean; text: string } | null>(null);

  const isH2 = $derived(c.kind === "h2");
  const problem = $derived(!c.name.trim() ? "请填写名称" : isH2 && !c.url.trim() ? "H2 需要填写完整的 JDBC URL" : !isH2 && !c.url.trim() && !c.host.trim() ? "请填写主机" : "");

  function kind(k: DbConn["kind"]) {
    const was = KINDS.find((x) => x.id === c.kind);
    c.kind = k;
    // 端口还是上一种库的默认值就跟着换（自己改过的不动）
    if (!c.port || c.port === was?.port) c.port = KINDS.find((x) => x.id === k)!.port;
  }

  /** 交给 Rust 的密码：填了就用填的；没填、存过就让 Rust 用钥匙串里的（null） */
  const pw = () => (password ? password : c.hasPassword ? null : "");

  async function test() {
    busy = true;
    result = null;
    try {
      const i = await dbTest($state.snapshot(c), pw());
      result = { ok: true, text: `连接成功：${i.product} ${i.version}` };
    } catch (e) {
      result = { ok: false, text: `连接失败：${String(e)}` };
    } finally {
      busy = false;
    }
  }

  async function save() {
    if (problem || busy) return;
    busy = true;
    try {
      await dbSave(root, $state.snapshot(c), password ? password : null);
      onsaved();
      onclose();
    } catch (e) {
      result = { ok: false, text: String(e) };
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!conn) return;
    await dbDelete(root, conn.id).catch(() => {});
    onsaved();
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      void save();
    }
  }

  function focusIn(el: HTMLInputElement) {
    el.focus({ preventScroll: true });
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog dbconn" role="dialog" aria-label={conn ? "编辑数据库连接" : "新建数据库连接"} tabindex="-1" onkeydown={onKey}>
    <p class="title">{conn ? "编辑数据库连接" : "新建数据库连接"}</p>
    <label class="row">
      <span class="lab">名称</span>
      <input class="field" use:focusIn spellcheck="false" placeholder="例如：订单库（本地）" bind:value={c.name} />
    </label>
    <div class="row">
      <span class="lab">类型</span>
      <span class="segs" role="radiogroup" aria-label="数据库类型">
        {#each KINDS as k (k.id)}
          <button class="btn sm" class:on={c.kind === k.id} role="radio" aria-checked={c.kind === k.id} onclick={() => kind(k.id)}>{k.label}</button>
        {/each}
      </span>
    </div>
    {#if !isH2}
      <div class="row">
        <span class="lab">主机</span>
        <input class="field" spellcheck="false" bind:value={c.host} aria-label="主机" />
        <span class="lab short">端口</span>
        <input class="field port" spellcheck="false" inputmode="numeric" bind:value={() => String(c.port || ""), (v) => (c.port = Number(v.replace(/\D/g, "")) || 0)} aria-label="端口" />
      </div>
      <label class="row">
        <span class="lab">数据库</span>
        <input class="field" spellcheck="false" placeholder="可留空" bind:value={c.database} />
      </label>
    {/if}
    <label class="row">
      <span class="lab">用户</span>
      <input class="field" spellcheck="false" bind:value={c.user} />
    </label>
    <label class="row">
      <span class="lab">密码</span>
      <input class="field" type="password" placeholder={c.hasPassword ? "已保存在钥匙串中（留空则不修改）" : "保存在 macOS 钥匙串中"} bind:value={password} />
    </label>
    <label class="row">
      <span class="lab">URL</span>
      <input class="field mono" spellcheck="false" placeholder={isH2 ? "jdbc:h2:~/data/app" : "可选：自定义 JDBC URL（填写后忽略主机和端口）"} bind:value={c.url} />
    </label>
    {#if result}<p class="result" class:bad={!result.ok}>{result.text}</p>{/if}
    <p class="note">首次连接某种数据库时，将从 Maven Central 下载对应驱动并校验后使用。连接配置保存在本机，不写入项目。</p>
    <div class="actions">
      {#if conn}<button class="btn destructive" disabled={busy} onclick={() => void remove()}>删除</button>{/if}
      <span class="gap"></span>
      {#if problem}<span class="why">{problem}</span>{/if}
      <button class="btn" disabled={busy || !!problem} class:busy onclick={() => void test()}>测试连接</button>
      <button class="btn" data-dismiss onclick={onclose}>取消</button>
      <button class="btn primary" data-default disabled={busy || !!problem} onclick={() => void save()}>保存</button>
    </div>
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .dbconn { width: min(560px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 10px; }
  .row { display: flex; align-items: center; gap: 10px; }
  .lab { flex: none; width: 48px; font-size: var(--fs-md); color: var(--text-muted); }
  .lab.short { width: auto; }
  .field { flex: 1; min-width: 0; height: 28px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-size: var(--fs-md); outline: none; }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .field.port { flex: none; width: 72px; font-variant-numeric: tabular-nums; }
  .mono { font-family: var(--code-font); font-size: var(--fs-sm); }
  .result { margin: 0; font-size: var(--fs-sm); color: var(--text); white-space: pre-wrap; }
  .result.bad { color: var(--lvl-error); }
  .note { margin: 0; font-size: var(--fs-sm); color: var(--text-faint); }
  .why { font-size: var(--fs-sm); color: var(--text-muted); white-space: nowrap; }
</style>
