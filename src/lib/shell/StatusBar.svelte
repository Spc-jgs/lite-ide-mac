<script lang="ts">
  /**
   * 状态栏：左边「我在哪个文件」（面包屑，或顶掉它的一条提示消息），
   * 右边「这个文件什么状态」（模式 / 语言 / 编码 / 脏 / git），每一格点了都能改
   * （ui.md 第十一条）。
   *
   * 消息直接读 `notify`；面包屑在这里算（`state/crumbs.ts`，纯函数）。
   * 剩下的都是当前标签的事实，由 App 传进来 —— 标签表还在 App 里（#9 第 4 步）。
   */
  import Icon from "./Icon.svelte";
  import type { MenuItem } from "./ContextMenu.svelte";
  import { cmenu } from "./context-menu.svelte";
  import { notify } from "../state/notify.svelte";
  import { progress } from "../state/progress.svelte";
  import { crumbsOf, projectName } from "../state/crumbs";
  import { isLogName } from "../logview/is-log-name";
  import { lang } from "../state/lang.svelte";
  import { nav } from "../state/nav.svelte";
  import { overlay } from "../state/overlay.svelte";
  import type { TabState } from "../state/tab";
  import { scratchSaveState, SCRATCH_SAVE_LABEL } from "../state/autosave";
  import type { GitEntry } from "../ipc/commands";

  let {
    active,
    activeScratch = false,
    activeEntry,
    root,
    logStatus,
    onReveal,
    onSwitchMode,
    onOpenEncoding,
    onOpenDiff,
    onSetIndent,
    onSetEol,
  }: {
    active: TabState | null;
    /** 当前标签是草稿：脏那一格说「自动保存…／已自动保存／失败 ⌘S 重试」而不是「已修改」 */
    activeScratch?: boolean;
    /** 当前文件在 git 里的改动条目，没有就是 null */
    activeEntry: GitEntry | null;
    root: string | null;
    /** 日志模式那一页报上来的一句状态 */
    logStatus: string;
    /** 点面包屑里的目录：在文件树里定位 */
    onReveal: (path: string) => void;
    onSwitchMode: () => void;
    /** 带上格子的矩形：浮层挂在它上面 */
    onOpenEncoding: (rect: DOMRect) => void;
    onOpenDiff: () => void;
    /** 「缩进 · 换行符」那格的菜单选了一项（issue #37）。都是这个文件的属性，写回标签 */
    onSetIndent: (ind: "tab" | number) => void;
    onSetEol: (eol: "LF" | "CRLF") => void;
  } = $props();

  let crumbs = $derived(active ? crumbsOf(root, active.path, active.name) : []);

  /**
   * 缩进 · 换行符（issue #33 ③ 显示，#37 可改）。缩进按盘上那份内容猜（`editor/indent.ts`），
   * 手动改过就用改过的（`tab.indent`）；换行符是 Rust 读文件时探的（`fsservice::eol`）。
   * 点了从这格底下（往上）掉一个菜单，选了写回标签 —— 都是文件的属性，换文件就变回去。
   */
  // `lang.mod` 没到手时空着 —— 和旁边语言名那格同一条规矩（issue #32）
  let indentNow = $derived<"tab" | number | null>(
    active?.mode === "edit" && lang.mod ? (active.indent ?? lang.mod.detectIndent(active.content ?? "")) : null,
  );
  let indent = $derived(lang.mod && active?.mode === "edit" ? lang.mod.indentLabel(indentNow) : "");
  const EOL_LABEL: Record<string, string> = { LF: "LF", CRLF: "CRLF", CR: "CR", mixed: "换行混用" };
  let eol = $derived(active?.mode === "edit" ? (EOL_LABEL[active.eol ?? "LF"] ?? active.eol ?? "LF") : "");

  let fmtMenu = $state<{ x: number; y: number } | null>(null);
  $effect(() => {
    if (fmtMenu) cmenu.load();
  });
  let fmtBtn = $state<HTMLButtonElement | null>(null);
  /**
   * 菜单项：三档常用缩进 + LF / CRLF。文件猜出来的是 3 空格这种不在档里的，把它也列进去
   * 打上勾 —— 不然菜单里一个勾都没有，人会以为「现在」是什么都没设。
   * CR（老 Mac）同理不给选项：没人往这个方向改，但当前是 CR 时勾就空着，格子里的字说明了。
   */
  let fmtItems = $derived.by<MenuItem[]>(() => {
    if (!lang.mod) return [];
    const m = lang.mod;
    const spaces = [2, 4];
    if (typeof indentNow === "number" && !spaces.includes(indentNow)) spaces.push(indentNow);
    spaces.sort((a, b) => a - b);
    const opts: ("tab" | number)[] = [...spaces, "tab"];
    const items: MenuItem[] = opts.map((o) => ({
      label: m.indentLabel(o),
      checked: o === indentNow,
      run: () => onSetIndent(o),
    }));
    for (const [i, e] of (["LF", "CRLF"] as const).entries()) {
      items.push({ label: e, checked: active?.eol === e, sep: i === 0, run: () => onSetEol(e) });
    }
    return items;
  });
  function openFmt() {
    if (!fmtBtn || !lang.mod) return;
    const r = fmtBtn.getBoundingClientRect();
    fmtMenu = { x: r.left, y: r.top - 4 };
  }
</script>

<!--
  状态栏 = IDEA 的 status bar，**左右两半各管一件事**：

  - 左：我在哪个文件（导航栏 / 面包屑）。IDEA 里不用导航栏时这块显示最近的
    事件消息 —— 这里照抄：`notify` 一来就顶掉路径。以前提示消息挤在挂件中间，
    窗口一窄它先被挤掉，而它恰恰是最该让人看见的。
  - 右：这个文件什么状态，而且**点了都能改**（模式 / 编码 / 差异）。

  这里以前还挂着「搜索 ⇧⇧」「终端 ⌘J」「改动 N」「历史」四个 —— 全是**打开某个
  工具窗**，而那四件事导轨上一个不落地都有（搜索还有双击 ⇧）。同一件事在一屏里
  说两遍，正是上一轮「工具窗切换只能有一处」那条判据本身。
  「改动 N」的计数没丢，挪到导轨 Git 图标的角标上了。
-->
<footer class="statusbar">
  <!--
    左槽。「正在做」原来排在这儿最前面（issue #15：它是唯一一条「事情还没完」的消息，
    不能被「已保存」顶掉）—— 2026-09-22 起它和拉取推送、日志索引一起归右边的进度格
    （`progress.svelte.ts`），左槽只剩做完的消息和面包屑，两边不再互相顶。
  -->
  {#if notify.info}
    <span class="cell ok navslot">{notify.info}</span>
  {:else if notify.error}
    <span class="cell err navslot">{notify.error}</span>
  {:else if crumbs.length > 0}
    <nav class="crumbs navslot" aria-label="当前文件路径">
      {#each crumbs as c, i (c.path)}
        {#if i > 0}<span class="sep" aria-hidden="true">›</span>{/if}
        {#if c.dir}
          <button class="crumb" onclick={() => onReveal(c.path)} title="在文件树中显示 {c.path}">{c.name}</button>
        {:else}
          <span class="crumb here" title={c.path}>{c.name}</span>
        {/if}
      {/each}
    </nav>
  {:else}
    <!-- 没项目不是「在等」—— 无项目是一等状态（#40 第三层），这格空着就好 -->
    <span class="cell dim navslot">{root ? projectName(root) : ""}</span>
  {/if}
  <span class="spacer"></span>

  <!--
    后台任务的进度格（照 IDEA 右下角）：文字 · 3px 条 · 百分比 · 取消。只画最新的一个，
    多于一个时加 +N。**文字放在一个表达式里**：smoke.sh 按「正在提交」整句找，
    拆成三个文本节点在辅助功能树里拼不回来（以前左槽那条的教训）。
    `{#key}`：任务换了整格重来一遍淡入，一个任务内部只更新条和数字。
  -->
  {#if progress.current}
    {@const t = progress.current}
    {@const more = progress.all.length - 1}
    {#key t.id}
      <span class="cell task" title={t.label}>
        <span class="tlabel">{more > 0 ? `${t.label}  +${more}` : t.label}</span>
        <span class="pbar" class:indet={t.percent === null}>
          {#if t.percent !== null}<span class="pfill" style:width="{t.percent}%"></span>{/if}
        </span>
        {#if t.percent !== null}<span class="tpct">{t.percent}%</span>{/if}
        {#if t.cancel}
          <button class="ibtn xs" onclick={t.cancel} title="取消" aria-label="取消 {t.label}"><Icon name="x" size={10} /></button>
        {/if}
      </span>
    {/key}
    <span class="vsep" aria-hidden="true"></span>
  {/if}

  <!-- 右槽 -->
  {#if active?.mode === "merge"}
    <span class="cell warn">冲突合并</span>
  {:else if active?.mode === "diff"}
    <span class="cell dim">
      {active.diffSha
        ? active.diffToLocal ? `${active.diffShort} → 本地` : `提交 ${active.diffShort}`
        : `差异 · ${active.diffStaged ? "已暂存" : "未暂存"}`}
    </span>
  {:else if active}
    <!--
      行:列。IDEA 和 VS Code 状态栏都有，两家都是点了就跳行。
      只在有编辑器时出现（`nav.caret` 在别的视图下是 null），格式照 IDEA `12:34`。
      排在最左：它是这一排里唯一会随光标跳动的，放最左不会让右边那几格跟着抖。
    -->
    {#if nav.caret}
      <button
        class="cell cbtn pos"
        class:on={overlay.gotoOpen && overlay.gotoAnchor !== null}
        onclick={(e) => overlay.openGoto(e.currentTarget.getBoundingClientRect())}
        title="跳到行 ⌘L"
      >{nav.caret.line}:{nav.caret.col}</button>
      <span class="vsep" aria-hidden="true"></span>
    {/if}
    <!--
      **这个按钮只在日志场景出现。**

      它原来对每一个打开的文件都在，而绝大多数文件根本不存在「切到日志模式」
      这个需求 —— 一个 `.ts` 切过去只会得到一份没高亮、不能编辑的文本。
      一个永远在、九成场合按下去只有坏处的按钮，等于白占了状态栏一格。

      两个条件：**已经在日志模式**（那必须留着回去的路，否则单向门），
      或者**文件名看着像日志**（判据在 `is-log-name.ts`，纯按名字，不看内容）。

      藏起来不等于做不了 —— 菜单里的「切换编辑 / 日志模式」对任何文件都还在。
    -->
    {#if active.mode === "log" || isLogName(active.path)}
      <button
        class="cell cbtn mode"
        onclick={onSwitchMode}
        title={active.mode === "log" ? "切换到编辑模式" : "切换到日志模式（只读，带级别过滤与 tail）"}
      >
        {active.mode === "log" ? "日志模式" : "编辑模式"} <Icon name="swap" size={11} />
      </button>
      <!--
        **竖线跟着它后面那格一起退场。**

        窄窗口下 `drop-2` 会藏掉语言、只读原因、保存状态，而竖线原来是
        独立的、不带 drop 类的 —— 于是 640px 宽时状态栏上出现两条挨着的竖线，
        末尾还吊着一条后面什么都没有的。分隔线分的是「区」，区没了线也该没。

        它也在 `{#if}` 里面：按钮不在时这条线就成了开头那条，
        左边什么都没有 —— 同一个毛病，换了个位置。
      -->
      <span class="vsep drop-2" aria-hidden="true"></span>
    {/if}
    {#if active.mode === "log"}
      <!-- 「为什么是只读」原来在标题栏。它说的是当前文件的状态，该和别的状态挂件在一起 -->
      <span
        class="cell dim drop-2"
        title={active.forced ? "你手动切到了日志模式" : "自动判定的原因"}
      >只读 · {active.forced ? "手动切换" : active.reason || "自动判定"}</span>
    {:else}
      <span class="cell dim drop-2">{lang.mod ? lang.mod.langLabel(lang.mod.langOf(active.path)) : ""}</span>
    {/if}
    {#if active.mode === "edit"}
      <!-- 缩进 · 换行符。混用的换行符标黄：那是文件坏了，保存时会统一成 LF；点了能改（issue #37） -->
      <span class="vsep drop-2" aria-hidden="true"></span>
      <button
        class="cell cbtn fmt drop-2"
        class:warn={active.eol === "mixed"}
        class:on={fmtMenu !== null}
        bind:this={fmtBtn}
        onclick={openFmt}
        title={active.eol === "mixed"
          ? "文件里 LF 和 CRLF 混用 —— 点这里选一种，保存时统一"
          : "缩进（按文件内容判断）· 换行符（保存时原样写回）—— 点击可改"}
      >{indent} · {eol}</button>
    {/if}
    <span class="vsep" aria-hidden="true"></span>
    <button
      class="cell cbtn enc"
      class:bad={active.lossy}
      class:on={overlay.encOpen}
      onclick={(e) => onOpenEncoding(e.currentTarget.getBoundingClientRect())}
      title={active.lossy
        ? "有解不出的字节，点这里换个编码重新打开"
        : "文件编码 —— 点击可换编码重新打开或另存"}
    >
      {active.encoding ?? "UTF-8"}{active.bom ? " ·BOM" : ""}{active.lossy ? " ⚠" : ""}
    </button>
    {#if active.mode === "log"}
      <span class="vsep" aria-hidden="true"></span>
      <span class="cell">{logStatus}</span>
    {:else}
      <span class="vsep drop-2" aria-hidden="true"></span>
      {@const st = scratchSaveState({ scratch: activeScratch, dirty: active.dirty, failed: !!active.saveFailed })}
      {#if st === null}
        <span class="cell drop-2" class:accent={active.dirty}>
          {active.dirty ? "已修改" : "无改动"}
        </span>
      {:else}
        <!--
          草稿：这格回答的是「存了没存」，不是「改没改」。改了半秒内就落盘，
          「已修改」会让人去找 ⌘S。失败是唯一要人管的，红字常驻到写成功为止
          （autosaveSweep 那条红色通知只弹一次，这里是留着的那份）
        -->
        <span
          class="cell drop-2"
          class:bad={st === "failed"}
          class:dim={st === "pending"}
          title={st === "failed" ? "改动还在编辑器里，⌘S 重试" : "草稿停止输入半秒后自动落盘，关闭前也会存"}
        >
          {SCRATCH_SAVE_LABEL[st]}
        </span>
      {/if}
    {/if}
    {#if activeEntry}
      <span class="vsep" aria-hidden="true"></span>
      <button
        class="cell cbtn git"
        onclick={onOpenDiff}
        title="查看这个文件的改动"
      >
        <!--
          这里说的是「相对 git 有没有未提交的改动」，跟左边那格的
          「无改动 / 已修改」（缓冲区有没有未保存的编辑）是两件事。
          原本写「有改动」，于是状态栏上会并排出现「无改动」和「有改动」，
          读起来自相矛盾。改成「未提交」，两格就能同时成立且不打架。
        -->
        {activeEntry.untracked ? "未跟踪" : "未提交"}
      </button>
    {/if}
  {/if}
</footer>

{#if fmtMenu && cmenu.comp}
  <cmenu.comp
    x={fmtMenu.x}
    y={fmtMenu.y}
    up
    label="缩进与换行符"
    items={fmtItems}
    onclose={(refocus) => {
      fmtMenu = null;
      if (refocus) fmtBtn?.focus();
    }}
  />
{/if}

<style>
  /*
   * 面包屑。2026-09-06 从标题栏搬到状态栏左边 —— IDEA 的导航栏就在那儿，
   * 而且「我在哪个文件」和右边那排「这个文件什么状态」是同一组信息。
   */
  .statusbar .crumbs {
    display: flex;
    align-items: center;
    gap: 3px;
    /*
     * `.statusbar > * { flex: none }` 会让这一条**不收缩**，窄窗口下
     * 一条长路径能把右边的状态挂件整个顶出屏幕。必须在这里覆回来。
     */
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    /* 状态栏整体是 code-font，而路径是可读文本不是标识符 */
    font-family: var(--ui-font);
  }
  .statusbar .crumbs .sep { flex: none; color: var(--text-faint); font-size: 10px; }
  .crumb {
    flex: none;
    max-width: 160px;
    height: 17px;
    padding: 0 4px;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 11.5px;
    line-height: 17px;
    cursor: default;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* 只有目录段可点（点了在文件树里定位），文件段是 span，不该有 hover 反馈 */
  button.crumb:hover { background: var(--hover); color: var(--text); }
  .crumb.here { color: var(--text-dim); flex: 0 1 auto; min-width: 40px; }
  /* 左槽整块：路径、项目名、以及顶掉它们的那条提示消息 */
  .statusbar .navslot { min-width: 0; overflow: hidden; text-overflow: ellipsis; }

  .statusbar {
    display: flex;
    align-items: center;
    flex-wrap: nowrap;
    /* 窗口窄的时候宁可把右边挤掉，也不能换行 —— 换行会把状态栏撑成两行，
       把编辑区顶掉一截 */
    overflow: hidden;
    gap: 16px;
    padding: 0 12px;
    /* 同标题栏：贴着窗口下边，需要一层 scrim 兜住 11.5px 的小字 */
    background: var(--chrome-scrim);
    /* 不画上边线（M8）：理由同标题栏，scrim 就是分区 */
    font-size: 11.5px;
    color: var(--text-dim);
    font-family: var(--code-font);
    user-select: none;
  }
  .statusbar .spacer { flex: 1; min-width: 0; }
  .statusbar > * { flex: none; white-space: nowrap; }
  /* 窄窗口下先让「知道了也不改变下一步」的那几格退场：语言、只读原因、保存状态 */
  @media (max-width: 740px) {
    .statusbar .drop-2 { display: none; }
  }
  .statusbar .dim { color: var(--text-faint); }
  .statusbar .ok { color: var(--accent); }
  .statusbar .err { color: var(--lvl-error); }
  .statusbar .warn { color: var(--lvl-warn); }
  /* 进度格：文字最多 220px 省略；条 72px；百分比等宽字不抖 */
  .statusbar .task { display: inline-flex; align-items: center; gap: 8px; color: var(--text-dim); animation: fade-in 90ms ease-out; }
  .statusbar .task .tlabel { max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--ui-font); }
  .statusbar .task .pbar { width: 72px; flex: none; }
  .statusbar .task .tpct { flex: none; min-width: 3ch; text-align: right; font-size: 10.5px; color: var(--text-faint); }
  .statusbar .task .ibtn { margin-left: -2px; }
  .statusbar .cbtn.enc { font-size: 11px; }
  /* 解码有损是必须让人看见的事，不能只做成一个安静的标签 */
  .statusbar .cbtn.enc.bad { color: var(--lvl-error); }
  /*
   * 状态栏的格子（2026-09-22 之前叫 `.btn`）：撞上了 app.css 的全局 `.btn`，靠这里覆盖着
   * 才没长成 24px 描边按钮 —— 全局那套一改（加个 transition、改个圆角）这里就跟着变。
   * 改名之后原来从全局白捡的几条（inline-flex、行高、不换行）在这儿自己写。
   */
  .statusbar .cbtn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: 11.5px;
    line-height: 1;
    padding: 0 6px;
    border-radius: var(--r-sm);
    white-space: nowrap;
    cursor: default;
  }
  .statusbar .cbtn:hover { background: var(--hover); color: var(--text); }
  .statusbar .cbtn.mode { color: var(--text-dim); }
  /* 菜单开着时这格保持点亮（ui.md 第十二条），不然那块浮层像凭空冒出来的 */
  .statusbar .cbtn.on { background: var(--selected); color: var(--text); }
  .statusbar .cbtn.fmt.warn { color: var(--lvl-warn); }
  .statusbar .cbtn.mode:hover { color: var(--accent); }
  .statusbar .cbtn.git { color: var(--git-modified); }
  /* 行:列是等宽数字，给个最小宽度，光标从 9 行跳到 10 行时右边那几格不动 */
  .statusbar .cbtn.pos { min-width: 44px; text-align: center; font-variant-numeric: tabular-nums; }
  /*
   * 挂件之间的竖线。**这是分区不是分项** —— 模式、语言/只读原因、编码、
   * 保存状态、git 状态，五组各说一件事，同字号同颜色排在一起时得有个断点。
   *
   * （它原来的理由是「左边一组是文档事实、右边一组是动作」，而右边那组
   * 打开工具窗的按钮 2026-09-06 整组撤了 —— 那些事导轨上都有。）
   */
  .statusbar .vsep {
    flex: none;
    width: 1px;
    height: 11px;
    background: var(--border);
  }
  /* 「已修改」是唯一会改变你下一步动作的那一项，值得提到 accent */
  .statusbar .accent { color: var(--accent); }
  .statusbar .cell.bad { color: var(--lvl-error); }
</style>
