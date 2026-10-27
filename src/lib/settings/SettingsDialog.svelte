<script lang="ts">
  /**
   * 设置（⌘,，docs/SETTINGS.md 第 13 节）：照 IDEA 的 Settings 对话框 —— 左边分类和搜索，右边表单，底下「取消 / 应用 / 确定」。
   * 「此项目」那一类只在打开了项目时有，里面的东西只对这个项目（IDEA 在项目级的设置旁边标一个图标，我们单列一类、写清楚）。
   *
   * 全局的写回 settings.json（Rust 只改那一个值的字符，注释和次序都在）；项目的写进应用自己的目录（不往项目里写）。
   * 改了先攒着，「应用」才交出去：改 JDK、Maven、jdtls 这些要重启语言服务器，一项一生效的话改到一半就重启了。
   *
   * 下拉框不用 `<select>`：Svelte 运行时里处理它的那段会落进入口包（frontend.md，调试面板那次）。JDK 这种几个选项的，摆成一列单选
   */
  import { untrack } from "svelte";
  import Icon from "../shell/Icon.svelte";
  import type { SettingDef, SettingsPage } from "../ipc/commands";
  import { settingsApply, settingsPage } from "../ipc/settings";
  import { CATS, catOf, changes, isSet, matches, parseIntField, shown, type Cat, type Edits } from "./settings-form";
  import { imeKey } from "../shell/ime";
  import KeymapPane from "./KeymapPane.svelte";
  import { staged, type KeyEdits } from "./keymap-form";
  import { conflicts, setOverrides } from "../state/keymap";
  import { keymapOverrides, keymapSet } from "../ipc/keymap";
  // 静态引：notify 本来就在入口包里。写成 `await import()` 的话 Rollup 要在入口里给它造一个模块命名空间对象（+114 B，量过）
  import { notify } from "../state/notify.svelte";

  /** `query`：打开时先填进搜索框（⇧⇧ 里选了一个设置项，docs/TAKEOVER.md I8 —— 直接看到那一项） */
  let { root, query = "", onclose }: { root: string | null; query?: string; onclose: () => void } = $props();

  let page = $state<SettingsPage | null>(null);
  let loadError = $state("");
  let edits = $state<Edits>({});
  /** 输入框里填错的（整数不是整数、越界）：键 → 说明。有它就不让应用 */
  let errs = $state<Record<string, string>>({});
  /** 「此项目」那一栏改了还没应用的；null = 没动 */
  let proj = $state<{ jdk: string; offline: boolean; skipTests: boolean } | null>(null);
  let cat = $state<Cat>("appearance");
  let q = $state(untrack(() => query));
  let busy = $state(false);
  let error = $state("");
  /** 「应用」之后在按钮旁边说一句（按钮原地反馈，ui.md 第十五条） */
  let applied = $state(false);
  let newJdk = $state("");
  /** 键位（docs/TAKEOVER.md R3）：现在的覆盖表、这次改了还没应用的 */
  let keyOver = $state<Record<string, string>>({});
  let keyEdits = $state<KeyEdits>({});

  $effect(() => {
    void settingsPage(root)
      .then((p) => (page = p))
      .catch((e) => (loadError = e instanceof Error ? e.message : String(e)));
    void keymapOverrides()
      .then((o) => (keyOver = o))
      .catch(() => {});
  });

  const cats = $derived(CATS.filter((c) => c.id !== "project" || page?.project));
  const projName = $derived(root ? root.slice(root.lastIndexOf("/") + 1) : "");
  const projNow = $derived(page?.project ? (proj ?? { jdk: page.project.jdk, offline: page.project.offline, skipTests: page.project.skipTests }) : null);
  const global = $derived(page ? changes(page, edits) : {});
  const projDirty = $derived(
    !!(page?.project && proj && (proj.jdk !== page.project.jdk || proj.offline !== page.project.offline || proj.skipTests !== page.project.skipTests)),
  );
  const keyNext = $derived(staged(keyOver, keyEdits));
  const keyDirty = $derived(JSON.stringify(Object.entries(keyNext).sort()) !== JSON.stringify(Object.entries(keyOver).sort()));
  const settingsDirty = $derived(Object.keys(global).length > 0 || projDirty);
  const dirty = $derived(settingsDirty || keyDirty);
  /** 写不了全局设置（settings.json 读不懂）：那几类的输入框灰掉 */
  const locked = $derived(!!page?.broken);
  /** 列出来的项：搜索时跨所有类（「java.jdks」那一项在 JDK 一节里单独画） */
  const fields = $derived(page ? page.defs.filter((d) => d.kind !== "array" && (q.trim() ? matches(d, q) : catOf(d.key) === cat)) : []);
  const jdkList = $derived(page ? ((shown(page, edits, page.defs.find((d) => d.key === "java.jdks")!) as string[]) ?? []) : []);
  /** 改了这几类要重启 jdtls 才生效：应用之后提醒一句 */
  // java.hotswapOnSave 是调试那边每次存盘时现读的，不用重启
  const needsRestart = $derived(Object.keys(global).some((k) => (k.startsWith("java.") && k !== "java.hotswapOnSave") || k.startsWith("maven.")) || (projDirty && proj?.jdk !== page?.project?.jdk));

  function set(key: string, v: unknown) {
    edits = { ...edits, [key]: v };
    applied = false;
  }

  function setInt(d: SettingDef, text: string) {
    const r = parseIntField(d, text);
    const next = { ...errs };
    if ("error" in r) next[d.key] = r.error;
    else {
      delete next[d.key];
      set(d.key, r.value);
    }
    errs = next;
  }

  function reset(key: string) {
    const next = { ...errs };
    delete next[key];
    errs = next;
    set(key, null);
  }

  function setProj(p: Partial<{ jdk: string; offline: boolean; skipTests: boolean }>) {
    if (!projNow) return;
    proj = { ...projNow, ...p };
    applied = false;
  }

  function addJdk() {
    const d = newJdk.trim();
    if (!d || jdkList.includes(d)) return;
    set("java.jdks", [...jdkList, d]);
    newJdk = "";
  }

  async function apply(thenClose: boolean) {
    if (!page) return;
    if (Object.keys(errs).length) {
      error = "存在无效的设置项，请修正后再应用";
      return;
    }
    if (!dirty) {
      if (thenClose) onclose();
      return;
    }
    // 键位撞了不让应用：菜单的键会先被吃掉、编辑器里两个只有一个能用 —— 应用出去就是「按了没反应」，不报错
    const clash = Object.keys(conflicts(keyNext));
    if (keyDirty && clash.length) {
      error = `快捷键 ${clash.join(" ")} 冲突，请修改其中一个后再应用`;
      cat = "keymap";
      q = "";
      return;
    }
    busy = true;
    error = "";
    const restart = needsRestart;
    try {
      if (settingsDirty) page = await settingsApply(root, global, projDirty && projNow && page.project ? { root: page.project.root, ...projNow } : null);
      edits = {};
      proj = null;
      if (keyDirty) {
        await keymapSet(keyNext);
        // 广播也会来（keymap-changed），这里先换上：这个窗口里不用等那一趟
        setOverrides(keyNext);
        keyOver = keyNext;
        keyEdits = {};
      }
      if (thenClose) {
        onclose();
        notify.ok(restart ? "设置已保存。Java 语言服务器将在下次启动时应用新设置（关闭此项目的所有 Java 文件，稍后重新打开即可）" : "设置已保存");
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

  async function openFile() {
    onclose();
    (await import("../shell/menu-actions")).openSettingsFile();
  }

  /** 对话框里的键不往外冒（App 的全局按键收不到）；Esc = 取消、⌘↵ = 确定 */
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

  const autofocus = (n: HTMLElement) => {
    queueMicrotask(() => n.focus({ preventScroll: true }));
  };
</script>

{#snippet field(d: SettingDef)}
  {@const v = page ? shown(page, edits, d) : null}
  <div class="row" class:set={page && isSet(page, edits, d.key)}>
    <div class="lab">
      <span class="name">{d.label}</span>
      <span class="key">{d.key}</span>
      {#if page && isSet(page, edits, d.key) && !locked}
        <button class="btn quiet sm reset" onclick={() => reset(d.key)} title="从 settings.json 中删除此项，恢复默认值">恢复默认</button>
      {/if}
    </div>
    {#if d.kind === "boolean"}
      <label class="check"><input type="checkbox" bind:checked={() => v === true, (c) => set(d.key, c)} disabled={locked} /> {v === true ? "开启" : "关闭"}</label>
    {:else if d.kind === "integer"}
      <input class="field num" bind:value={() => String(v ?? ""), (t) => setInt(d, t)} disabled={locked} aria-label={d.label} />
    {:else}
      <input class="field" bind:value={() => String(v ?? ""), (t) => set(d.key, t)} disabled={locked} spellcheck="false" aria-label={d.label} />
    {/if}
    {#if errs[d.key]}<p class="ferr">{errs[d.key]}{d.min !== null ? `（${d.min}–${d.max}）` : ""}</p>{/if}
    <p class="doc">{d.doc}</p>
  </div>
{/snippet}

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog settings" role="dialog" aria-label="设置" tabindex="-1" onkeydown={onKey}>
    <div class="main">
      <nav class="cats" aria-label="设置的分类">
        <input class="field search" placeholder="搜索设置" spellcheck="false" bind:value={q} use:autofocus aria-label="搜索设置" />
        {#each cats as c (c.id)}
          <button class="cat" class:on={cat === c.id && !q.trim()} onclick={() => ((cat = c.id), (q = ""))}>
            {c.label}{#if c.id === "project"}<span class="proj">{projName}</span>{/if}
          </button>
        {/each}
      </nav>
      <section class="pane">
        {#if loadError}
          <div class="empty-msg bad">{loadError}</div>
        {:else if !page}
          <div class="empty-msg wait"><span class="spinner sm"></span> 正在加载设置…</div>
        {:else}
          {#if page.broken && cat !== "project" && cat !== "keymap"}
            <div class="banner">
              <Icon name="warn" />
              <span>settings.json {page.broken}。请先修复该文件，才能在此修改全局设置</span>
              <button class="btn sm" onclick={openFile}>打开 settings.json</button>
            </div>
          {/if}
          {#if q.trim()}
            {#each fields as d (d.key)}{@render field(d)}{:else}<div class="empty-msg">没有名称或说明包含「{q.trim()}」的设置</div>{/each}
          {:else if cat === "keymap"}
            <KeymapPane
              over={keyOver}
              edits={keyEdits}
              onedit={(id, a) => ((keyEdits = { ...keyEdits, [id]: a }), (applied = false), (error = ""))}
              onresetall={() => (keyEdits = Object.fromEntries(Object.keys(keyNext).map((id) => [id, null])))}
            />
          {:else if cat === "project" && page.project && projNow}
            <h3>项目 JDK <span class="scope">仅对 {projName}</span></h3>
            <p class="doc">运行、构建和测试使用的 JDK。可在运行配置中为单个服务另行指定</p>
            <div class="choices" role="radiogroup" aria-label="项目 JDK">
              <button class="choice" role="radio" aria-checked={projNow.jdk === ""} onclick={() => setProj({ jdk: "" })}>
                <span class="dot" class:on={projNow.jdk === ""}></span><span class="what">根据 pom 自动选择</span><span class="where">{page.project.autoJdk}</span>
              </button>
              {#each page.jdks as j (j.home)}
                <button class="choice" role="radio" aria-checked={projNow.jdk === j.home} onclick={() => setProj({ jdk: j.home })}>
                  <span class="dot" class:on={projNow.jdk === j.home}></span><span class="what">Java {j.version}</span><span class="where">{j.home}</span>
                </button>
              {/each}
              {#if projNow.jdk && !page.jdks.some((j) => j.home === projNow.jdk)}
                <div class="choice gone"><span class="dot on"></span><span class="what">未找到</span><span class="where">{projNow.jdk}（运行时将根据 pom 自动选择）</span></div>
              {/if}
            </div>
            <h3>Maven <span class="scope">仅对 {projName}</span></h3>
            <label class="check"><input type="checkbox" bind:checked={() => projNow!.offline, (c) => setProj({ offline: c })} /> 离线（-o）</label>
            <label class="check"><input type="checkbox" bind:checked={() => projNow!.skipTests, (c) => setProj({ skipTests: c })} /> 跳过测试（执行 Maven 目标时添加 -DskipTests）</label>
            <p class="doc">
              {#if page.project.wrapper}项目根目录包含 mvnw：{page.values["maven.useWrapper"] ? "已使用（版本由 .mvn/wrapper/ 指定）" : "未使用（已在「Maven」类别中关闭）"}{:else}项目根目录没有 mvnw：使用「Maven」类别中设置的 Maven{/if}。
              勾哪些 profile 在 Maven 工具窗里；每个服务的 VM 参数、Spring profile、环境变量在「运行 › 编辑配置…」
            </p>
            {@const cs = page.project.codeStyle}
            <h3>Java 代码风格 <span class="scope">与 IDEA 一致，{cs.origin}</span></h3>
            <dl class="style">
              <dt>缩进 / 续行</dt><dd>{cs.indent} / {cs.cont} 个空格</dd>
              <dt>行宽</dt><dd>{cs.margin}</dd>
              <dt>import 顺序</dt><dd>{cs.imports}</dd>
              <dt>合并为 .*</dt><dd>同一包中 {cs.star} 个类、static {cs.staticStar} 个成员</dd>
            </dl>
            <p class="doc">
              ⌥⌘L 格式化、⌃⌥O 优化 import、补全时自动加的 import 都照它。要改就在 IDEA 里改（项目的方案提交在 .idea/codeStyles/ 里，团队共用），
              改完在状态栏「重起」Java 智能。缩进跟着编辑器对每个文件的判断走{#each cs.notes as n}；{n}{/each}
            </p>
          {:else}
            {#each fields as d (d.key)}{@render field(d)}{/each}
            {#if cat === "java"}
              <h3>JDK <span class="scope">本机</span></h3>
              <p class="doc">自动检测：系统、~/Library/Java、Homebrew、sdkman、asdf、jenv、mise。未检测到的可在下方手动添加（JAVA_HOME 目录）</p>
              <div class="jdks">
                {#each page.jdks as j (j.home)}
                  <div class="jdk"><span class="ver">Java {j.version}</span><span class="where">{j.home}</span>{#if j.manual}<span class="tag">手动添加</span>{/if}</div>
                {/each}
                {#each jdkList as d (d)}
                  {#if !page.jdks.some((j) => j.home === d || j.home.startsWith(d.replace(/\/$/, "") + "/"))}
                    <div class="jdk pending" class:bad={page.badJdks.includes(d)}>
                      <span class="ver">{page.badJdks.includes(d) ? "无法识别" : "待应用"}</span><span class="where">{d}</span>
                    </div>
                  {/if}
                {/each}
              </div>
              <div class="add">
                <input class="field" placeholder="/opt/jdk-21" spellcheck="false" bind:value={newJdk} disabled={locked}
                  onkeydown={(e) => { if (imeKey(e)) return; if (e.key === "Enter") { e.preventDefault(); addJdk(); } }}
                  aria-label="JDK 目录" />
                <button class="btn" disabled={locked || !newJdk.trim()} onclick={addJdk}>添加</button>
              </div>
              {#if jdkList.length}
                <div class="manual">
                  {#each jdkList as d (d)}
                    <span class="chip">{d}<button class="ibtn xs" title="移除" aria-label="移除 {d}" disabled={locked} onclick={() => set("java.jdks", jdkList.filter((x) => x !== d))}><Icon name="x" /></button></span>
                  {/each}
                </div>
              {/if}
            {/if}
          {/if}
        {/if}
      </section>
    </div>
    <div class="actions">
      <button class="btn quiet" onclick={openFile} title={page?.path}>打开 settings.json</button>
      <span class="gap"></span>
      {#if error}<span class="err">{error}</span>{:else if applied}<span class="ok">已应用</span>{/if}
      <button class="btn" data-dismiss onclick={onclose}>取消</button>
      <button class="btn" disabled={!dirty || busy} class:busy onclick={() => void apply(false)}>应用</button>
      <button class="btn primary" data-default disabled={busy} onclick={() => void apply(true)}>确定</button>
    </div>
  </div>
</div>

<style>
  /* 盖整个窗口（不只编辑岛）：设置不是「编辑岛里的事」。点暗处不关 —— 同 Confirms 的对话框，改了一半的东西不该因为手滑没了 */
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
  .settings {
    width: min(880px, calc(100% - 48px));
    height: min(620px, calc(100vh - 96px));
    padding: 0;
    gap: 0;
    overflow: hidden;
  }
  .main { flex: 1; min-height: 0; display: flex; }
  .cats {
    flex: none;
    width: 200px;
    padding: 12px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-right: 1px solid var(--border-soft);
  }
  .search { margin-bottom: 8px; }
  .cat {
    display: flex;
    align-items: baseline;
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
  }
  .cat:hover { background: var(--hover); }
  .cat.on { background: var(--selected); color: var(--text); }
  .proj { color: var(--text-faint); font-size: var(--fs-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .pane { flex: 1; min-width: 0; overflow: auto; padding: 16px 20px; }
  .row { position: relative; padding: 8px 0 10px 12px; }
  /* 改过的（settings.json 里写了、或这次改了）：左边一条强调色（VS Code 设置编辑器的做法） */
  .row.set::before { content: ""; position: absolute; left: 0; top: 10px; bottom: 12px; width: 2px; border-radius: 99px; background: var(--accent); }
  .lab { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  .name { font-size: var(--fs-md); color: var(--text); font-weight: 600; }
  .key { font-size: var(--fs-xs); color: var(--text-faint); font-family: var(--code-font); }
  .reset { margin-left: auto; }
  .field {
    width: 100%;
    max-width: 520px;
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
  .field:disabled { opacity: 0.5; }
  .field.num { width: 120px; }
  .cats .field { max-width: none; }
  .doc { margin: 6px 0 0; font-size: var(--fs-sm); line-height: 1.5; color: var(--text-muted); max-width: 620px; }
  .ferr { margin: 4px 0 0; font-size: var(--fs-sm); color: var(--lvl-error); }
  .check { display: flex; align-items: center; gap: 6px; font-size: var(--fs-md); color: var(--text-dim); margin: 6px 0; }
  h3 { margin: 18px 0 4px; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  h3:first-child { margin-top: 0; }
  .scope { margin-left: 6px; font-weight: normal; font-size: var(--fs-xs); color: var(--text-faint); }
  .choices { display: flex; flex-direction: column; gap: 2px; margin-top: 8px; }
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
  .choice.gone { cursor: default; color: var(--lvl-warn); }
  .dot { flex: none; width: 12px; height: 12px; border-radius: 99px; border: 1.5px solid var(--text-faint); }
  .dot.on { border-color: var(--accent); background: radial-gradient(var(--accent) 0 3px, transparent 3.5px); }
  .what { flex: none; color: var(--text); }
  .where { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-faint); font-size: var(--fs-sm); }
  .jdks { display: flex; flex-direction: column; gap: 2px; margin: 8px 0; }
  .jdk { display: flex; align-items: center; gap: 8px; min-height: 24px; font-size: var(--fs-md); }
  .jdk .ver { flex: none; width: 64px; color: var(--text); }
  .jdk.pending .ver { color: var(--text-faint); }
  .jdk.bad .ver, .jdk.bad .where { color: var(--lvl-error); }
  .tag { flex: none; font-size: var(--fs-xs); color: var(--text-faint); }
  .add { display: flex; gap: 8px; align-items: center; max-width: 520px; }
  .manual { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .style { display: grid; grid-template-columns: max-content 1fr; gap: 4px 16px; margin: 8px 0 0; font-size: var(--fs-md); }
  .style dt { color: var(--text-muted); }
  .style dd { margin: 0; color: var(--text); }
  .chip { display: inline-flex; align-items: center; gap: 4px; padding: 2px 4px 2px 8px; border-radius: var(--r-xs); background: var(--hover); font-size: var(--fs-sm); color: var(--text-dim); }
  .banner { display: flex; align-items: center; gap: 8px; padding: 8px 10px; margin-bottom: 12px; border-radius: var(--r-sm); background: color-mix(in srgb, var(--lvl-error) 14%, transparent); font-size: var(--fs-sm); color: var(--text); }
  .banner span { flex: 1; }
  .actions { flex: none; margin: 0; padding: 10px 16px; border-top: 1px solid var(--border-soft); }
  .err { font-size: var(--fs-sm); color: var(--lvl-error); max-width: 360px; }
  .ok { font-size: var(--fs-sm); color: var(--accent); }
</style>
