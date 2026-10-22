<script lang="ts">
  /**
   * 运行 / 调试配置（「运行 › 编辑配置…」、Services 里服务的右键「编辑配置…」）：照 IDEA 的 Run/Debug Configurations。
   * 左边按来源分组，右边是选中那份的表单（docs/SETTINGS.md 13.6、docs/TAKEOVER.md R1）：
   *
   * - **Spring Boot**（认出来的）：profile、VM 参数、程序参数、环境变量、JDK；IDEA 里配过的上面一行「从 IDEA 导入」
   * - **你建的**：应用（一个 main 方法）、Maven 目标、Shell 命令、组合（几份一起起）—— 「＋」新建、可以删、可以改名
   * - **package.json / tasks.json**：只列出来（命令是那两个文件里写的），组合配置里选得到
   *
   * 都存在应用自己的目录里，只有你自己的（IDEA 的运行配置默认也是：`.idea/workspace.xml`）。命令都由 Rust 用现成的拼法拼 ——
   * 应用配置和认出来的 Spring 服务同一种，调试、热替换照常。改的是下次起它时用的：正在跑的要重跑才生效
   */
  import Icon from "../shell/Icon.svelte";
  import type { EnvVar, IdeaRun, RunConfig, RunConfigs, UserConfig, UserKind } from "../ipc/commands";
  import { runConfigs, runConfigSet, runConfigUserDelete, runConfigUserSet } from "../ipc/settings";
  import { imeKey } from "../shell/ime";
  // 静态引：notify 本来就在入口包里。写成 `await import()` 的话 Rollup 要在入口里给它造一个模块命名空间对象（+114 B，量过）
  import { notify } from "../state/notify.svelte";

  let { root, pick = "", onclose }: { root: string; pick?: string; onclose: () => void } = $props();

  const KIND_LABEL: Record<UserKind, string> = { application: "应用", maven: "Maven", shell: "Shell", compound: "组合", remote: "远程 JVM 调试" };
  const KIND_DOC: Record<UserKind, string> = {
    application: "跑一个 main 方法（不是 Spring Boot 的也行）：先编译它在的模块，再 java 起它；调试、热替换照常",
    maven: "在一个模块上跑几个 Maven 目标（clean package、spring-boot:build-image …），带上 Maven 工具窗里勾的 profile 和开关",
    shell: "一行命令，交给你的登录 shell（和在终端里敲一样，和 tasks.json 同一种）",
    compound: "几份一起起（前后端、几个微服务）：⌃R 重跑也是一起",
    remote: "挂到别处起的 JVM 上调试（测试环境、容器里、自己在终端起的）：▶ 就是挂上去，⌘F2 断开 —— 只断开，不停那个 JVM",
  };

  let data = $state<RunConfigs | null>(null);
  let loadError = $state("");
  /** 认出来的 Spring 服务里改了还没应用的：服务名 → 整份 */
  let edits = $state<Record<string, RunConfig>>({});
  /** 你建的那几份的工作副本：`orig` 是存着的名字（新建的是空串），`gone` = 点了删除、还没应用 */
  let ulist = $state<{ key: number; orig: string; cfg: UserConfig; gone: boolean }[]>([]);
  let nextKey = 1;
  /** 选中的那一份：`svc:名字` / `user:key` / `other:名字` */
  let sel = $state("");
  let busy = $state(false);
  let error = $state("");
  let applied = $state(false);
  let adding = $state(false);
  /** 刚从 IDEA 导入的：哪个服务、从哪份、带不过来的几处 */
  let took = $state<{ svc: string; from: string; notes: string[] } | null>(null);

  function load(d: RunConfigs, want = "") {
    data = d;
    ulist = d.user.map((cfg) => ({ key: nextKey++, orig: cfg.name, cfg: structuredClone($state.snapshot(cfg)), gone: false }));
    const u = ulist.find((x) => x.orig === want);
    sel = d.services.some((s) => s.name === want)
      ? `svc:${want}`
      : u
        ? `user:${u.key}`
        : d.services[0]
          ? `svc:${d.services[0].name}`
          : ulist[0]
            ? `user:${ulist[0].key}`
            : d.others[0]
              ? `other:${d.others[0].name}`
              : "";
  }

  $effect(() => {
    void runConfigs(root)
      .then((d) => load(d, pick))
      .catch((e) => (loadError = e instanceof Error ? e.message : String(e)));
  });

  const selKind = $derived(sel.slice(0, sel.indexOf(":")));
  const selId = $derived(sel.slice(sel.indexOf(":") + 1));
  const cur = $derived(data && selKind === "svc" ? (edits[selId] ?? data.services.find((s) => s.name === selId) ?? null) : null);
  const ucur = $derived(selKind === "user" ? (ulist.find((x) => String(x.key) === selId && !x.gone) ?? null) : null);
  const ocur = $derived(data && selKind === "other" ? (data.others.find((o) => o.name === selId) ?? null) : null);

  const svcDirty = $derived(
    data ? Object.keys(edits).filter((n) => JSON.stringify(edits[n]) !== JSON.stringify(data!.services.find((s) => s.name === n))) : [],
  );
  const userDirty = (x: (typeof ulist)[number]) =>
    x.gone ? x.orig !== "" : x.orig === "" || JSON.stringify(x.cfg) !== JSON.stringify(data?.user.find((u) => u.name === x.orig));
  const dirtyCount = $derived(svcDirty.length + ulist.filter(userDirty).length);
  const envOf = $derived(cur ? cur.env : ucur ? ucur.cfg.env : []);
  const badEnv = $derived(envOf.find((e) => e.name.trim() && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(e.name.trim())));

  function patch(p: Partial<RunConfig>) {
    if (!cur) return;
    edits = { ...edits, [cur.name]: { ...cur, ...p } };
    applied = false;
  }

  function upatch(p: Partial<UserConfig>) {
    if (!ucur) return;
    Object.assign(ucur.cfg, p);
    applied = false;
  }

  /** 环境变量 / JDK 两个小部件两种表单共用：改的是当前那份 */
  const setEnv = (env: EnvVar[]) => (cur ? patch({ env }) : upatch({ env }));
  const setJdk = (jdk: string) => (cur ? patch({ jdk }) : upatch({ jdk }));

  /** 整份换成 IDEA 那份（不是合并：IDEA 里没配的就是空的） */
  function takeIdea(r: IdeaRun) {
    if (!cur) return;
    patch({ profiles: r.profiles, vmOptions: r.vmOptions, programArgs: r.programArgs, env: r.env.map((e) => ({ ...e })), jdk: r.jdk });
    took = { svc: cur.name, from: r.name, notes: r.notes };
  }

  /** 新建：名字先给一个不撞的「未命名的应用」，人自己改 */
  function add(kind: UserKind) {
    adding = false;
    if (!data) return;
    const taken = new Set([...data.services.map((s) => s.name), ...data.others.map((o) => o.name), ...ulist.filter((x) => !x.gone).map((x) => x.cfg.name)]);
    const base = `未命名的${KIND_LABEL[kind]}`;
    let name = base;
    for (let i = 2; taken.has(name); i++) name = `${base} ${i}`;
    const cfg: UserConfig = {
      name, kind, mainClass: "", module: data.modules[0] ?? "", goals: "", command: "", cwd: "", items: [],
      profiles: "", vmOptions: "", programArgs: "", env: [], jdk: "", host: "localhost", port: 5005,
    };
    const key = nextKey++;
    ulist = [...ulist, { key, orig: "", cfg, gone: false }];
    sel = `user:${key}`;
    applied = false;
  }

  function remove() {
    if (!ucur) return;
    const key = ucur.key;
    if (ucur.orig === "") ulist = ulist.filter((x) => x.key !== key);
    else ucur.gone = true;
    sel = data?.services[0] ? `svc:${data.services[0].name}` : (ulist.find((x) => !x.gone) ? `user:${ulist.find((x) => !x.gone)!.key}` : "");
    applied = false;
  }

  /** 组合配置里选得到的：别的组合不行（套组合会转圈，Rust 那边也挡） */
  const runnable = $derived(
    data
      ? [
          ...data.services.map((s) => s.name),
          ...ulist.filter((x) => !x.gone && x.cfg.kind !== "compound" && x !== ucur).map((x) => x.cfg.name),
          ...data.others.map((o) => o.name),
        ]
      : [],
  );

  async function apply(thenClose: boolean) {
    if (!data) return;
    error = "";
    busy = true;
    let at = "";
    try {
      for (const n of svcDirty) {
        at = `svc:${n}`;
        const c = edits[n];
        // 名字空着的那几行（加了一行没填）不交：真实现会把它当成写错的变量名
        const env = c.env.map((e) => ({ name: e.name.trim(), value: e.value })).filter((e) => e.name);
        await runConfigSet(root, n, { profiles: c.profiles, vmOptions: c.vmOptions, programArgs: c.programArgs, env, jdk: c.jdk });
      }
      // 先删再存：删掉「甲」、把「乙」改名成「甲」这种，反过来存会说名字撞了
      for (const x of ulist.filter((x) => x.gone && x.orig)) await runConfigUserDelete(root, x.orig);
      for (const x of ulist.filter((x) => !x.gone && userDirty(x))) {
        at = `user:${x.key}`;
        await runConfigUserSet(root, x.orig, { ...$state.snapshot(x.cfg), env: x.cfg.env.filter((e) => e.name.trim()) });
        x.orig = x.cfg.name;
      }
      const changed = dirtyCount;
      const keep = selKind === "user" ? (ucur?.cfg.name ?? "") : selId;
      load(await runConfigs(root), keep);
      edits = {};
      took = null;
      if (thenClose) {
        onclose();
        if (changed) notify.ok(`运行配置已保存，下次启动时生效（正在跑的要重跑）`);
      } else {
        applied = true;
        setTimeout(() => (applied = false), 1400);
      }
    } catch (e) {
      // 哪一份没存成：选中它、说清为什么（Rust 那边一句人话）
      const name = at.startsWith("user:") ? ulist.find((x) => `user:${x.key}` === at)?.cfg.name : at.slice(4);
      error = `「${name}」：${e instanceof Error ? e.message : String(e)}`;
      if (at) sel = at;
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      if (adding) adding = false;
      else onclose();
    } else if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      void apply(true);
    }
  }
</script>

{#snippet envEditor(env: EnvVar[])}
  <div class="lab">环境变量</div>
  <div class="env">
    {#each env as v, i (i)}
      <div class="kv">
        <input class="field mono k" placeholder="名字" spellcheck="false" aria-label="环境变量名"
          bind:value={() => v.name, (s) => setEnv(env.map((x, j) => (j === i ? { ...x, name: s } : x)))} />
        <span class="eq">=</span>
        <input class="field mono" placeholder="值" spellcheck="false" aria-label="环境变量的值"
          bind:value={() => v.value, (s) => setEnv(env.map((x, j) => (j === i ? { ...x, value: s } : x)))} />
        <button class="ibtn sm" title="删掉这一个" aria-label="删掉 {v.name || '这一行'}" onclick={() => setEnv(env.filter((_, j) => j !== i))}><Icon name="x" /></button>
      </div>
    {/each}
    <button class="btn sm" onclick={() => setEnv([...env, { name: "", value: "" }])}><Icon name="plus" /> 加一个</button>
  </div>
  {#if badEnv}<p class="ferr">「{badEnv.name}」不能当变量名：要字母或下划线开头，后面只有字母、数字、下划线</p>{/if}
{/snippet}

{#snippet jdkPicker(jdk: string)}
  <div class="lab">JDK</div>
  <div class="choices" role="radiogroup" aria-label="用哪个 JDK">
    <button class="choice" role="radio" aria-checked={jdk === ""} onclick={() => setJdk("")}>
      <span class="dot" class:on={jdk === ""}></span><span class="what">跟项目</span><span class="where">设置 › 此项目 里选的；没选就按 pom 自动挑</span>
    </button>
    {#if jdk && !data!.jdks.some((j) => j.home === jdk)}
      <!-- 选的这个不在列表里（导入的 IDEA 配置指的、或者存过之后挪走了）：照样列出来，别让人以为什么都没选 -->
      <button class="choice" role="radio" aria-checked="true">
        <span class="dot on"></span><span class="what">不在列表里</span><span class="where">{jdk}</span>
      </button>
    {/if}
    {#each data!.jdks as j (j.home)}
      <button class="choice" role="radio" aria-checked={jdk === j.home} onclick={() => setJdk(j.home)}>
        <span class="dot" class:on={jdk === j.home}></span><span class="what">Java {j.version}</span><span class="where">{j.home}</span>
      </button>
    {/each}
  </div>
{/snippet}

{#snippet modulePicker(module: string)}
  <div class="lab">模块</div>
  {#if !data!.modules.length}
    <p class="ferr">这个项目里没找到 pom.xml：应用 / Maven 配置要在一个 Maven 模块上跑</p>
  {:else}
    <div class="choices" role="radiogroup" aria-label="哪个 Maven 模块">
      {#each data!.modules as m (m)}
        <button class="choice" role="radio" aria-checked={module === m} onclick={() => upatch({ module: m })}>
          <span class="dot" class:on={module === m}></span><span class="what">{m || "（项目根）"}</span>
        </button>
      {/each}
    </div>
  {/if}
{/snippet}

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog runcfg" role="dialog" aria-label="运行配置" tabindex="-1" onkeydown={onKey}>
    <div class="main">
      <nav class="list" aria-label="运行配置">
        <div class="tools">
          <button class="ibtn sm" title="新建一份运行配置" aria-label="新建" aria-expanded={adding} onclick={() => (adding = !adding)}><Icon name="plus" /></button>
          <button class="ibtn sm" title="删掉这一份（只有你建的能删）" aria-label="删除" disabled={!ucur} onclick={remove}><Icon name="minus" /></button>
        </div>
        {#if adding}
          <div class="addmenu" role="menu" aria-label="新建哪一种">
            {#each Object.keys(KIND_LABEL) as k (k)}
              <button class="svc" role="menuitem" onclick={() => add(k as UserKind)} title={KIND_DOC[k as UserKind]}>{KIND_LABEL[k as UserKind]}</button>
            {/each}
          </div>
        {/if}
        {#if data?.services.length}
          <div class="head">Spring Boot</div>
          {#each data.services as s (s.name)}
            <button class="svc" class:on={sel === `svc:${s.name}`} onclick={() => (sel = `svc:${s.name}`)} title={s.mainClass}>
              {s.name}{#if svcDirty.includes(s.name)}<span class="mod" title="改了还没应用">•</span>{/if}
            </button>
          {/each}
        {/if}
        {#if ulist.some((x) => !x.gone)}
          <div class="head">你建的</div>
          {#each ulist.filter((x) => !x.gone) as x (x.key)}
            <button class="svc" class:on={sel === `user:${x.key}`} onclick={() => (sel = `user:${x.key}`)} title={KIND_LABEL[x.cfg.kind]}>
              <span class="kind">{KIND_LABEL[x.cfg.kind]}</span>{x.cfg.name || "（没名字）"}{#if userDirty(x)}<span class="mod" title="改了还没应用">•</span>{/if}
            </button>
          {/each}
        {/if}
        {#if data?.others.length}
          <div class="head">package.json · tasks.json</div>
          {#each data.others as o (o.name)}
            <button class="svc dim" class:on={sel === `other:${o.name}`} onclick={() => (sel = `other:${o.name}`)} title={o.command}>{o.name}</button>
          {/each}
        {/if}
      </nav>
      <section class="pane">
        {#if loadError}
          <div class="empty-msg bad">{loadError}</div>
        {:else if !data}
          <div class="empty-msg wait"><span class="spinner sm"></span> 正在列运行配置…</div>
        {:else if cur}
          <h3>{cur.name}</h3>
          <p class="doc">{cur.mainClass}{cur.module ? `（${cur.module}）` : ""} · 认出来的 Spring Boot 应用</p>
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

          {@render envEditor(cur.env)}
          {@render jdkPicker(cur.jdk)}
        {:else if ucur}
          {@const c = ucur.cfg}
          <label class="lab first" for="rc-name">名字</label>
          <input id="rc-name" class="field" bind:value={() => c.name, (v) => upatch({ name: v })} spellcheck="false" />
          <p class="doc">{KIND_LABEL[c.kind]} · {KIND_DOC[c.kind]}</p>

          {#if c.kind === "application"}
            <label class="lab" for="rc-main">主类</label>
            <input id="rc-main" class="field mono" bind:value={() => c.mainClass, (v) => upatch({ mainClass: v })} placeholder="com.demo.Tool" spellcheck="false" />
            <p class="doc">全名，带包名；有 main 方法的那个类</p>
            {@render modulePicker(c.module)}
            <label class="lab" for="rc-uvm">VM 参数</label>
            <input id="rc-uvm" class="field mono" bind:value={() => c.vmOptions, (v) => upatch({ vmOptions: v })} placeholder="-Xmx512m -Dfoo=bar" spellcheck="false" />
            <label class="lab" for="rc-uargs">程序参数</label>
            <input id="rc-uargs" class="field mono" bind:value={() => c.programArgs, (v) => upatch({ programArgs: v })} spellcheck="false" />
            <label class="lab" for="rc-uprof">Spring profile（可选）</label>
            <input id="rc-uprof" class="field" bind:value={() => c.profiles, (v) => upatch({ profiles: v })} placeholder="不是 Spring 应用就空着" spellcheck="false" />
            {@render envEditor(c.env)}
            {@render jdkPicker(c.jdk)}
          {:else if c.kind === "maven"}
            {@render modulePicker(c.module)}
            <label class="lab" for="rc-goals">目标</label>
            <input id="rc-goals" class="field mono" bind:value={() => c.goals, (v) => upatch({ goals: v })} placeholder="clean package" spellcheck="false" />
            <p class="doc">空格隔开；Maven 工具窗里勾的 profile、离线、跳过测试会一起带上</p>
            {@render envEditor(c.env)}
            {@render jdkPicker(c.jdk)}
          {:else if c.kind === "remote"}
            {@const jdwp = `-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:${c.port || 5005}`}
            <label class="lab" for="rc-host">主机</label>
            <input id="rc-host" class="field mono" bind:value={() => c.host, (v) => upatch({ host: v })} placeholder="localhost" spellcheck="false" />
            <label class="lab" for="rc-port">端口</label>
            <input id="rc-port" class="field mono num" bind:value={() => String(c.port || ""), (v) => upatch({ port: /^\d{1,5}$/.test(v.trim()) ? Number(v.trim()) : 0 })} placeholder="5005" inputmode="numeric" spellcheck="false" />
            <div class="lab">那个 JVM 起的时候要带上（IDEA 的 Remote JVM Debug 也是这一句）</div>
            <pre class="cmd">{jdwp}</pre>
            <button class="btn sm" onclick={() => void navigator.clipboard.writeText(jdwp).then(() => notify.ok("已复制"))}>复制</button>
            <p class="doc">JDK 9 之后 address 要写 *:端口 才收得到别的机器的连接（只写端口只听本机）。断点、日志断点、异常断点照常；热替换不行（代理塞不进别人起的 JVM）</p>
          {:else if c.kind === "shell"}
            <label class="lab" for="rc-cmd">命令</label>
            <input id="rc-cmd" class="field mono" bind:value={() => c.command, (v) => upatch({ command: v })} placeholder="pnpm dev" spellcheck="false" />
            <label class="lab" for="rc-cwd">工作目录</label>
            <input id="rc-cwd" class="field mono" bind:value={() => c.cwd, (v) => upatch({ cwd: v })} placeholder="（项目根）" spellcheck="false" />
            <p class="doc">相对项目根</p>
            {@render envEditor(c.env)}
          {:else}
            <div class="lab">一起起这几份</div>
            {#if !runnable.length}
              <p class="ferr">还没有别的运行配置可选</p>
            {:else}
              <div class="choices" role="group" aria-label="一起起这几份">
                {#each runnable as n (n)}
                  {@const on = c.items.includes(n)}
                  <button class="choice" role="checkbox" aria-checked={on} onclick={() => upatch({ items: on ? c.items.filter((x) => x !== n) : [...c.items, n] })}>
                    <span class="box" class:on></span><span class="what">{n}</span>
                  </button>
                {/each}
              </div>
              <p class="doc">按列出的次序起；⌃R 重跑、⌘F2 停也是一起</p>
            {/if}
          {/if}
        {:else if ocur}
          <h3>{ocur.name}</h3>
          <p class="doc">{ocur.source === "package" ? "package.json 里的 scripts，自动列出来的" : "tasks.json 里写的"} —— 改它去改那个文件</p>
          <div class="lab">命令</div>
          <pre class="cmd">{ocur.command}</pre>
          {#if ocur.source === "file"}
            <button class="btn sm" onclick={() => { onclose(); void import("../state/runs-ops").then((m) => m.openTasksFile(root)); }}>打开 tasks.json</button>
          {/if}
        {:else}
          <div class="empty-msg">
            还没有运行配置
            <div class="sub">左上角「＋」新建一份；认出来的 Spring Boot 应用、package.json 的 scripts、tasks.json 里的会自动列在左边</div>
          </div>
        {/if}
      </section>
    </div>
    <div class="actions">
      <span class="note">{data?.problems.length ? data.problems.join("；") : data?.ideaProblems.length ? data.ideaProblems.join("；") : "只存在你这台机器上，不往项目里写"}</span>
      <span class="gap"></span>
      {#if error}<span class="err">{error}</span>{:else if applied}<span class="ok">已应用</span>{/if}
      <button class="btn" data-dismiss onclick={onclose}>取消</button>
      <button class="btn" disabled={!dirtyCount || busy || !!badEnv} onclick={() => void apply(false)}>应用</button>
      <button class="btn primary" data-default disabled={busy || !!badEnv} onclick={() => void apply(true)}>确定</button>
    </div>
  </div>
</div>

<style>
  .field.num { width: 120px; }
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
  .svc.dim { color: var(--text-muted); }
  .kind { flex: none; padding: 0 5px; border-radius: var(--r-sm); background: var(--hover); color: var(--text-faint); font-size: var(--fs-xs); }
  .tools { display: flex; gap: 2px; padding: 0 4px 6px; }
  .addmenu { display: flex; flex-direction: column; gap: 2px; margin: 0 0 6px; padding: 4px; border-radius: var(--r-md, 6px); background: var(--hover); }
  .lab.first { margin-top: 0; }
  .box { flex: none; width: 12px; height: 12px; border-radius: 3px; border: 1.5px solid var(--text-faint); }
  .box.on { border-color: var(--accent); background: var(--accent); }
  .cmd { margin: 0 0 10px; padding: 6px 8px; max-width: 560px; border-radius: var(--r-sm); background: var(--hover); font-family: var(--code-font); font-size: var(--fs-sm); white-space: pre-wrap; overflow-wrap: anywhere; color: var(--text); }
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
