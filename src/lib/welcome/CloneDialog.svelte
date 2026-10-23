<script lang="ts">
  /**
   * 从 Git 克隆（docs/TAKEOVER.md G27，IDEA 的 Get from VCS）：地址、放到哪个目录、目录名，「克隆」。
   * 目录名照 git 的规矩从地址猜（Rust 那一份），你改过就不再跟着地址变；放到哪儿默认是最近那个项目的上一级，上次挑过的记着。
   *
   * 克隆跑起来对话框不关：进度写在按钮旁边，「取消」喊停（git 留下的半截目录进废纸篓，见 `gitsvc::remote::clone`）。
   * 做完关掉、开这个项目（和「打开文件夹」同一条路：没项目的窗口就地变成它）。
   * 认证不过不替你输密码（应用里从来不经手密码）：照拉取那套说一句去终端怎么做，原话能展开
   */
  import { untrack } from "svelte";
  import { Channel } from "@tauri-apps/api/core";
  import type { RemoteErr, RemoteProgress } from "../ipc/commands";
  import { notify } from "../state/notify.svelte";
  import { tabflow } from "../state/tabflow.svelte";
  import { imeKey } from "../shell/ime";
  import { cancelOp, cloneName, cloneProblem, defaultParent, gitClone, pickDir, tildify } from "./welcome";

  let { recent, onclose }: { recent: string[]; onclose: () => void } = $props();

  const KEY = "lite-ide.clone-parent";
  const remembered = (() => {
    try {
      return localStorage.getItem(KEY) ?? "";
    } catch {
      return "";
    }
  })();

  let url = $state("");
  // 只取开的那一刻的名单（对话框开着时名单变了也不改你眼前这个默认值）
  let parent = $state(remembered || untrack(() => defaultParent(recent)));
  let name = $state("");
  /** 你自己改过目录名：之后地址变了也不替你改 */
  let named = false;
  let busy = $state(false);
  let phase = $state("");
  let err = $state<RemoteErr | null>(null);
  let opId = 0;
  let seq = 0;

  const problem = $derived(cloneProblem(url, parent, name));
  const ready = $derived(!!url.trim() && !problem && !busy);

  // 地址一变就猜目录名（Rust 算，挡在最后一次输入上：先发的回包晚到不能盖掉后发的）
  $effect(() => {
    const u = url;
    if (named) return;
    const my = ++seq;
    void cloneName(u).then((n) => {
      if (my === seq && !named) name = n;
    });
  });

  // 关掉的时候还在克隆（Esc、点了外面都会关）：喊停，不让它在后台接着跑
  $effect(() => () => {
    if (busy) void cancelOp(opId);
  });

  async function choose() {
    const d = await pickDir("克隆到哪个目录", parent || null).catch(() => null);
    if (d) parent = d;
  }

  function hint(e: RemoteErr): string {
    // ls-remote 只问一下远程有哪些分支、不往盘上写东西，认证那一步和克隆一样 —— 输过一次钥匙串就记住了
    if (e.kind === "auth-https") return `这个仓库要先认证一次：在终端里跑一遍\n  git ls-remote ${url.trim()}\n输过一次账号密码（钥匙串会记住），这里就能克隆了`;
    if (e.kind === "auth-ssh") return "ssh-agent 里没有能用的钥匙：\n  ssh-add --apple-use-keychain ~/.ssh/id_ed25519";
    return e.message;
  }

  async function start() {
    if (!ready) return;
    busy = true;
    err = null;
    phase = "正在连接…";
    // 和拉取 / 推送共用一张按 (窗口, id) 记的表；它们从 1 往上数，这里从一个它们碰不到的数起
    opId = 1_000_000 + Date.now() % 1_000_000;
    const ch = new Channel<RemoteProgress>();
    ch.onmessage = (p) => (phase = p.percent === null ? p.phase : `${p.phase} ${p.percent}%`);
    try {
      const target = await gitClone(parent, url.trim(), name, opId, ch);
      busy = false;
      try {
        localStorage.setItem(KEY, parent);
      } catch {
        /* 记不住就下次再挑 */
      }
      onclose();
      notify.ok(`已克隆到 ${tildify(target)}`);
      await tabflow.openPath(target);
    } catch (e) {
      busy = false;
      const r = e as RemoteErr;
      if (r?.kind === "cancelled") phase = "";
      else err = r?.kind ? r : { kind: "other", message: String(e), raw: "" };
    }
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      void start();
    }
  }

  function focusIn(el: HTMLInputElement) {
    el.focus({ preventScroll: true });
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog clone" role="dialog" aria-label="从 Git 克隆" tabindex="-1" onkeydown={onKey}>
    <p class="title">从 Git 克隆</p>
    <label class="row">
      <span class="lab">地址</span>
      <input class="field" use:focusIn placeholder="https://github.com/… 或 git@github.com:…" spellcheck="false" disabled={busy} bind:value={url} />
    </label>
    <div class="row">
      <span class="lab">放到</span>
      <span class="dir" class:empty={!parent} title={parent}>{parent ? tildify(parent) : "还没选"}</span>
      <button class="btn sm" disabled={busy} onclick={() => void choose()}>选择…</button>
    </div>
    <label class="row">
      <span class="lab">目录名</span>
      <input class="field" spellcheck="false" disabled={busy} bind:value={() => name, (v) => ((name = v), (named = true))} />
    </label>
    {#if err}
      <div class="err">
        <pre>{hint(err)}</pre>
        {#if err.raw && err.raw !== err.message}<details><summary>git 的原话</summary><pre>{err.raw}</pre></details>{/if}
      </div>
    {/if}
    <div class="actions">
      {#if busy}<span class="phase"><span class="spinner sm"></span> {phase}</span>{:else if problem}<span class="why">{problem}</span>{/if}
      <span class="gap"></span>
      {#if busy}
        <button class="btn" onclick={() => void cancelOp(opId)}>取消克隆</button>
      {:else}
        <button class="btn" data-dismiss onclick={onclose}>取消</button>
      {/if}
      <button class="btn primary" data-default disabled={!ready} class:busy onclick={() => void start()}>克隆</button>
    </div>
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .clone { width: min(560px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 10px; }
  .row { display: flex; align-items: center; gap: 10px; }
  .lab { flex: none; width: 48px; font-size: var(--fs-md); color: var(--text-muted); }
  .field { flex: 1; min-width: 0; height: 28px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-size: var(--fs-md); outline: none; }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .dir { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; direction: rtl; text-align: left; font-size: var(--fs-md); color: var(--text); }
  .dir.empty { direction: ltr; color: var(--text-faint); }
  .err { padding: 8px 10px; border-radius: var(--r-sm); background: color-mix(in srgb, var(--lvl-error) 12%, transparent); font-size: var(--fs-sm); color: var(--text); }
  .err pre { margin: 0; white-space: pre-wrap; font-family: var(--code-font); font-size: var(--fs-sm); }
  .err details { margin-top: 6px; color: var(--text-muted); }
  .phase, .why { font-size: var(--fs-sm); color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 300px; }
</style>
