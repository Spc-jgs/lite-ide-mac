<script lang="ts">
  /**
   * 运行配置（「运行 › 编辑配置…」、Services 里服务的右键「编辑配置…」，docs/SETTINGS.md 13.6）：照 IDEA 的 Run/Debug Configurations ——
   * 左边认出来的 Spring 服务，右边它的 Spring profile、VM 参数、程序参数、环境变量、JDK。
   *
   * 只往我们拼的命令里加东西（Rust 的 `maven::run_command`），所以调试、热替换照常。存在应用自己的目录里，只有你自己的
   * （IDEA 的运行配置默认也是：`.idea/workspace.xml`）。IDEA 里给这个服务配过的（profile、VM 参数……），上面一行「从 IDEA 导入」
   * 填进表单（`ideacfg.rs` 读、转好），带不过来的几处列在底下 —— 填进来不等于存了，和手改一样要点「确定」
   * 改的是下次起它时用的：正在跑的要重跑才生效，底下说一句
   */
  import Icon from "../shell/Icon.svelte";
  import type { IdeaRun, RunConfig, RunConfigs } from "../ipc/commands";
  import { runConfigs, runConfigSet } from "../ipc/settings";
  import { imeKey } from "../shell/ime";
  // 静态引：notify 本来就在入口包里。写成 `await import()` 的话 Rollup 要在入口里给它造一个模块命名空间对象（+114 B，量过）
  import { notify } from "../state/notify.svelte";

  let { root, pick = "", onclose }: { root: string; pick?: string; onclose: () => void } = $props();

  let data = $state<RunConfigs | null>(null);
  let loadError = $state("");
  /** 改了还没应用的：服务名 → 整份配置 */
  let edits = $state<Record<string, RunConfig>>({});
  let sel = $state("");
  let busy = $state(false);
  let error = $state("");
  let applied = $state(false);
  /** 刚从 IDEA 导入的：哪个服务、从哪份、带不过来的几处 */
  let took = $state<{ svc: string; from: string; notes: string[] } | null>(null);

  $effect(() => {
    void runConfigs(root)
      .then((d) => {
        data = d;
        sel = d.services.some((s) => s.name === pick) ? pick : (d.services[0]?.name ?? "");
      })
      .catch((e) => (loadError = e instanceof Error ? e.message : String(e)));
  });

  const cur = $derived(data ? (edits[sel] ?? data.services.find((s) => s.name === sel) ?? null) : null);
  const dirtyNames = $derived(
    data ? Object.keys(edits).filter((n) => JSON.stringify(edits[n]) !== JSON.stringify(data!.services.find((s) => s.name === n))) : [],
  );
  const badEnv = $derived(cur ? cur.env.find((e) => e.name.trim() && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(e.name.trim())) : undefined);

  function patch(p: Partial<RunConfig>) {
    if (!cur) return;
    edits = { ...edits, [cur.name]: { ...cur, ...p } };
    applied = false;
  }

  /** 整份换成 IDEA 那份（不是合并：IDEA 里没配的就是空的） */
  function takeIdea(r: IdeaRun) {
    if (!cur) return;
    patch({ profiles: r.profiles, vmOptions: r.vmOptions, programArgs: r.programArgs, env: r.env.map((e) => ({ ...e })), jdk: r.jdk });
    took = { svc: cur.name, from: r.name, notes: r.notes };
  }

  async function apply(thenClose: boolean) {
    if (!data) return;
    error = "";
    busy = true;
    try {
      for (const n of dirtyNames) {
        const c = edits[n];
        // 名字空着的那几行（加了一行没填）不交：真实现会把它当成写错的变量名
        const env = c.env.map((e) => ({ name: e.name.trim(), value: e.value })).filter((e) => e.name);
        await runConfigSet(root, n, { profiles: c.profiles, vmOptions: c.vmOptions, programArgs: c.programArgs, env, jdk: c.jdk });
      }
      const changed = dirtyNames.length;
      data = await runConfigs(root);
      edits = {};
      took = null;
      if (thenClose) {
        onclose();
        if (changed) {
            notify.ok(`运行配置已保存，下次启动时生效（正在跑的要重跑）`);
        }
      } else {
        applied = true;
        setTimeout(() => (applied = false), 1400);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      void apply(true);
    }
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog runcfg" role="dialog" aria-label="运行配置" tabindex="-1" onkeydown={onKey}>
    <div class="main">
      <nav class="list" aria-label="Spring Boot 服务">
        <div class="head">Spring Boot</div>
        {#each data?.services ?? [] as s (s.name)}
          <button class="svc" class:on={sel === s.name} onclick={() => (sel = s.name)} title={s.mainClass}>
            {s.name}{#if dirtyNames.includes(s.name)}<span class="mod" title="改了还没应用">•</span>{/if}
          </button>
        {/each}
      </nav>
      <section class="pane">
        {#if loadError}
          <div class="empty-msg bad">{loadError}</div>
        {:else if !data}
          <div class="empty-msg wait"><span class="spinner sm"></span> 正在找 Spring Boot 应用…</div>
        {:else if !cur}
          <div class="empty-msg">
            没认出 Spring Boot 应用
            <div class="sub">各模块 src/main/java 里带 @SpringBootApplication、又有 main 方法的类会出现在这里</div>
          </div>
        {:else}
          <h3>{cur.name}</h3>
          <p class="doc">{cur.mainClass}{cur.module ? `（${cur.module}）` : ""}</p>
          {#if cur.idea.length}
            <div class="idea">
              <span class="idea-lab">IDEA 里配过</span>
              {#each cur.idea as r (r.name)}
                <button class="btn sm" onclick={() => takeIdea(r)} title="把它的 profile、VM 参数、程序参数、环境变量、JDK 换进下面（点「确定」才存）">从「{r.name}」导入</button>
              {/each}
            </div>
            {#if took?.svc === cur.name}
              <div class="took">
                已把「{took.from}」填进下面，点「确定」才存{took.notes.length ? `；有 ${took.notes.length} 处带不过来：` : "。"}
                {#if took.notes.length}<ul>{#each took.notes as n (n)}<li>{n}</li>{/each}</ul>{/if}
              </div>
            {/if}
          {/if}

          <label class="lab" for="rc-profiles">Spring profile</label>
          <input id="rc-profiles" class="field" bind:value={() => cur!.profiles, (v) => patch({ profiles: v })} placeholder="dev, local" spellcheck="false" />
          <p class="doc">逗号或空格隔开，起的时候带 -Dspring.profiles.active=…（IDEA 的 Active profiles）</p>

          <label class="lab" for="rc-vm">VM 参数</label>
          <input id="rc-vm" class="field mono" bind:value={() => cur!.vmOptions, (v) => patch({ vmOptions: v })} placeholder="-Xmx512m -Dfoo=bar" spellcheck="false" />
          <p class="doc">照 shell 的写法原样放在 java 后面，带空格的值自己加引号</p>

          <label class="lab" for="rc-args">程序参数</label>
          <input id="rc-args" class="field mono" bind:value={() => cur!.programArgs, (v) => patch({ programArgs: v })} placeholder="--server.port=9090" spellcheck="false" />
          <p class="doc">放在主类后面，同上</p>

          <div class="lab">环境变量</div>
          <div class="env">
            {#each cur.env as v, i (i)}
              <div class="kv">
                <input class="field mono k" placeholder="名字" spellcheck="false" aria-label="环境变量名"
                  bind:value={() => v.name, (s) => patch({ env: cur!.env.map((x, j) => (j === i ? { ...x, name: s } : x)) })} />
                <span class="eq">=</span>
                <input class="field mono" placeholder="值" spellcheck="false" aria-label="环境变量的值"
                  bind:value={() => v.value, (s) => patch({ env: cur!.env.map((x, j) => (j === i ? { ...x, value: s } : x)) })} />
                <button class="ibtn sm" title="删掉这一个" aria-label="删掉 {v.name || '这一行'}" onclick={() => patch({ env: cur.env.filter((_, j) => j !== i) })}><Icon name="x" /></button>
              </div>
            {/each}
            <button class="btn sm" onclick={() => patch({ env: [...cur.env, { name: "", value: "" }] })}><Icon name="plus" /> 加一个</button>
          </div>
          {#if badEnv}<p class="ferr">「{badEnv.name}」不能当变量名：要字母或下划线开头，后面只有字母、数字、下划线</p>{/if}

          <div class="lab">JDK</div>
          <div class="choices" role="radiogroup" aria-label="这个服务用的 JDK">
            <button class="choice" role="radio" aria-checked={cur.jdk === ""} onclick={() => patch({ jdk: "" })}>
              <span class="dot" class:on={cur.jdk === ""}></span><span class="what">跟项目</span><span class="where">设置 › 此项目 里选的；没选就按 pom 自动挑</span>
            </button>
            {#if cur.jdk && !data.jdks.some((j) => j.home === cur.jdk)}
              <!-- 选的这个不在列表里（导入的 IDEA 配置指的、或者存过之后挪走了）：照样列出来，别让人以为什么都没选 -->
              <button class="choice" role="radio" aria-checked="true">
                <span class="dot on"></span><span class="what">不在列表里</span><span class="where">{cur.jdk}</span>
              </button>
            {/if}
            {#each data.jdks as j (j.home)}
              <button class="choice" role="radio" aria-checked={cur.jdk === j.home} onclick={() => patch({ jdk: j.home })}>
                <span class="dot" class:on={cur.jdk === j.home}></span><span class="what">Java {j.version}</span><span class="where">{j.home}</span>
              </button>
            {/each}
          </div>
        {/if}
      </section>
    </div>
    <div class="actions">
      <span class="note">{data?.ideaProblems.length ? data.ideaProblems.join("；") : "只存在你这台机器上，不往项目里写"}</span>
      <span class="gap"></span>
      {#if error}<span class="err">{error}</span>{:else if applied}<span class="ok">已应用</span>{/if}
      <button class="btn" data-dismiss onclick={onclose}>取消</button>
      <button class="btn" disabled={!dirtyNames.length || busy || !!badEnv} onclick={() => void apply(false)}>应用</button>
      <button class="btn primary" data-default disabled={busy || !!badEnv} onclick={() => void apply(true)}>确定</button>
    </div>
  </div>
</div>

<style>
  .layer {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: min(8vh, 64px);
    background: rgba(0, 0, 0, 0.38);
  }
  .runcfg { width: min(820px, calc(100% - 48px)); height: min(600px, calc(100vh - 96px)); padding: 0; gap: 0; overflow: hidden; }
  .main { flex: 1; min-height: 0; display: flex; }
  .list { flex: none; width: 220px; padding: 12px 8px; overflow: auto; display: flex; flex-direction: column; gap: 2px; border-right: 1px solid var(--border-soft); }
  .head { padding: 0 10px 4px; font-size: var(--fs-xs); letter-spacing: 0.04em; color: var(--text-faint); }
  .svc {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: var(--r-sm);
    background: none;
    color: var(--text-dim);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .svc:hover { background: var(--hover); }
  .svc.on { background: var(--selected); color: var(--text); }
  .mod { color: var(--accent); }
  .pane { flex: 1; min-width: 0; overflow: auto; padding: 16px 20px; }
  h3 { margin: 0; font-size: var(--fs-lg); font-weight: 600; color: var(--text); }
  .lab { display: block; margin: 14px 0 6px; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .field {
    width: 100%;
    max-width: 560px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--field-border);
    border-radius: var(--r-sm);
    background: var(--field-bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    outline: none;
  }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .mono { font-family: var(--code-font); font-size: var(--fs-sm); }
  .doc { margin: 6px 0 0; font-size: var(--fs-sm); line-height: 1.5; color: var(--text-muted); }
  .ferr { margin: 6px 0 0; font-size: var(--fs-sm); color: var(--lvl-error); }
  .idea { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 10px; }
  .idea-lab { font-size: var(--fs-sm); color: var(--text-muted); }
  .took { margin-top: 8px; max-width: 560px; padding: 6px 10px; border-radius: var(--r-sm); background: var(--hover); font-size: var(--fs-sm); line-height: 1.5; color: var(--text-dim); }
  .took ul { margin: 4px 0 0; padding-left: 18px; }
  .env { display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
  .kv { display: flex; align-items: center; gap: 6px; width: 100%; max-width: 560px; }
  .kv .k { flex: 0 0 180px; }
  .eq { color: var(--text-faint); }
  .choices { display: flex; flex-direction: column; gap: 2px; }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    padding: 4px 8px;
    border: none;
    border-radius: var(--r-sm);
    background: none;
    color: var(--text-dim);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .choice:hover { background: var(--hover); }
  .dot { flex: none; width: 12px; height: 12px; border-radius: 99px; border: 1.5px solid var(--text-faint); }
  .dot.on { border-color: var(--accent); background: radial-gradient(var(--accent) 0 3px, transparent 3.5px); }
  .what { flex: none; color: var(--text); }
  .where { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-faint); font-size: var(--fs-sm); }
  .actions { flex: none; margin: 0; padding: 10px 16px; border-top: 1px solid var(--border-soft); }
  .note { font-size: var(--fs-sm); color: var(--text-faint); }
  .err { font-size: var(--fs-sm); color: var(--lvl-error); max-width: 360px; }
  .ok { font-size: var(--fs-sm); color: var(--accent); }
</style>
