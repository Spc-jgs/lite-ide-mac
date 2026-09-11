<script lang="ts">
  /**
   * Git 控制台：把跑过的每一条 git 原样摆出来（issue #29，形状抄 IDEA）。
   *
   * # 它回答的问题
   *
   * 在这之前，git 出了问题**证据说完就没**：失败弹一条横幅，关掉就找不回来；
   * 而且界面上**完全看不到跑的是什么命令** —— 我们给每条 git 都带了一串
   * 加固参数（挡住「仓库自己的 config 让 git 去执行东西」，issue #24），
   * 哪天某个加固参数把一个正常仓库弄坏了，现在的界面给不出任何线索。
   *
   * 所以这里显示的是**完整 argv**，加固参数一个不少。看着长，但那正是要看的。
   *
   * # 不做什么
   *
   * **不能在里面打字执行 git。** 那是终端的事，而终端已经有了 ——
   * 一个半吊子的 git shell 只会让人分不清哪个能干什么。这是只读的一页。
   *
   * # 数据从哪来
   *
   * Rust 侧一个内存环（`gitsvc::console`），关掉应用就没。上限、截断、
   * 凭据打码全在那边 —— 前端只负责显示，一个字都不加工。
   */
  import { onMount } from "svelte";
  import { gitConsole, clearGitConsole, type GitCmd } from "../ipc/commands";

  let rows = $state<GitCmd[]>([]);
  let onlyFailed = $state(false);
  /** 展开了 stderr 的那几条（按时间戳 + 序号认，路径可能重复） */
  let opened = $state<Set<number>>(new Set());

  const failed = (r: GitCmd) => r.code !== 0;

  let shown = $derived(onlyFailed ? rows.filter(failed) : rows);
  let failCount = $derived(rows.filter(failed).length);

  async function refresh() {
    try {
      rows = await gitConsole();
    } catch {
      /* 拉不到就保持上一份 —— 一个诊断页面不该自己再弹一个错 */
    }
  }

  /*
   * 轮询而不是让 Rust 侧推事件。
   *
   * 判据是「只在有人看着的时候才花钱」：这个组件只在控制台那一页
   * **正在显示**时才挂载（面板切走就整个销毁），所以定时器天然跟着可见性走。
   * 换成事件推送的话，要在 Rust 侧为每条 git 多发一次 IPC —— 而状态刷新
   * 每次窗口获得焦点都跑，那就是在给没人看的东西付钱。
   *
   * 1.5 秒：够快到「点完一个 git 动作切过来就看得见」，
   * 又不至于让一页静止的文本每秒重画。
   */
  onMount(() => {
    void refresh();
    const id = setInterval(() => void refresh(), 1500);
    return () => clearInterval(id);
  });

  function toggle(i: number) {
    const next = new Set(opened);
    if (next.has(i)) next.delete(i);
    else next.add(i);
    opened = next;
  }

  async function wipe() {
    try {
      await clearGitConsole();
    } catch {
      /* 清不掉也不值得报错 */
    }
    opened = new Set();
    await refresh();
  }

  /** 本地时区的 `13:42:01.123` —— 和 IDEA 的 Console 一个形状 */
  function clock(ms: number): string {
    const d = new Date(ms);
    const p = (n: number, w = 2) => String(n).padStart(w, "0");
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
  }

  /**
   * argv 拼成一行能直接贴进终端的样子。
   *
   * 带空格或空的参数加引号 —— 不加的话，`-c filter.x.smudge=` 后面跟一个
   * 空值时整行会看着像少了点什么，而**「原样」是这一页唯一的承诺**。
   */
  const line = (argv: string[]) =>
    argv.map((a) => (a === "" || /[\s"']/.test(a) ? JSON.stringify(a) : a)).join(" ");

  async function copy(r: GitCmd) {
    const text = [line(r.argv), r.err].filter(Boolean).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      copied = r.ms;
      setTimeout(() => (copied = copied === r.ms ? 0 : copied), 1200);
    } catch {
      /* 剪贴板被拒（少见）就算了，人还可以自己选中 */
    }
  }
  let copied = $state(0);
</script>

<div class="gc">
  <div class="tools">
    <span>跑过 {rows.length} 条</span>
    {#if failCount > 0}
      <span class="bad">失败 {failCount}</span>
    {/if}
    <label class="chk">
      <input type="checkbox" bind:checked={onlyFailed} />
      只看失败
    </label>
    <span class="gap"></span>
    <button onclick={() => void refresh()} title="刷新">刷新</button>
    <button onclick={() => void wipe()} title="清空（只是内存里那份）">清空</button>
  </div>

  {#if shown.length === 0}
    <div class="empty">
      {#if rows.length === 0}
        还没跑过 git。打开一个仓库、刷新状态或者提交一次，这里就会有东西。
      {:else}
        这一批里没有失败的。
      {/if}
    </div>
  {:else}
    <div class="list">
      {#each shown as r, i (r.ms + ":" + i)}
        <div class="row" class:fail={failed(r)}>
          <button class="head" onclick={() => toggle(i)}>
            <span class="t">{clock(r.ms)}</span>
            <span class="code" class:bad={failed(r)}>
              {r.code === null ? "—" : r.code}
            </span>
            <span class="dur">{r.durMs}ms</span>
            <span class="cmd">{line(r.argv)}</span>
          </button>
          <!--
            用文字不用图标：Icon.svelte 里没有「复制」，而那一套有很严的规矩
            （统一 16 网格、1.25 描边、端点一律 round，见它的文件头）——
            为一个角落里的按钮新增一个图标，不如老实写两个字。
          -->
          <button class="copy" title="复制这条（命令 + 原话）" onclick={() => void copy(r)}>
            {copied === r.ms ? "已复制" : "复制"}
          </button>
        </div>
        {#if opened.has(i)}
          <div class="detail">
            <div class="cwd">在 {r.cwd}</div>
            {#if r.err}
              <pre>{r.err}</pre>
              {#if r.errTruncated}
                <div class="cut">（这段被截断了 —— 后面还有，多半是钩子在刷屏）</div>
              {/if}
            {:else}
              <div class="cut">这条没有输出到 stderr。</div>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .gc {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--editor-bg);
  }
  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    font-size: 11px;
    color: var(--text-dim);
    user-select: none;
  }
  .tools .gap { flex: 1; }
  .tools .bad { color: var(--danger, #e5534b); }
  .tools button {
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-size: 11px;
    padding: 1px 7px;
    cursor: pointer;
  }
  .tools button:hover { color: var(--text); border-color: var(--text-dim); }
  .chk { display: flex; align-items: center; gap: 4px; cursor: pointer; }
  .chk input { margin: 0; }

  .empty {
    padding: 18px;
    color: var(--text-dim);
    font-size: 12px;
    line-height: 1.7;
  }

  .list { flex: 1; overflow: auto; }

  .row { display: flex; align-items: flex-start; }
  .row:hover { background: var(--hover); }
  /* 失败那条左边一道竖线 —— 比整行染色轻，滚动时也更容易扫到 */
  .row.fail { box-shadow: inset 2px 0 0 var(--danger, #e5534b); }

  .head {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
    background: none;
    border: 0;
    padding: 2px 8px;
    text-align: left;
    cursor: pointer;
    font-family: var(--mono-font, ui-monospace, monospace);
    font-size: 11.5px;
    color: var(--text);
  }
  .head .t { flex: none; color: var(--text-dim); }
  .head .code {
    flex: none;
    min-width: 18px;
    text-align: right;
    color: var(--text-dim);
  }
  .head .code.bad { color: var(--danger, #e5534b); font-weight: 600; }
  .head .dur { flex: none; color: var(--text-dim); min-width: 44px; text-align: right; }
  /*
   * 命令这一列**不换行、超出就省略**。
   *
   * 加固参数那一串很长，让它换行的话一条命令能占四五行，
   * 整页就不再是「一眼扫过去哪条红了」——而那是这一页的第一用途。
   * 完整内容点开就有（详情里 cwd 和原话都在），也可以一键复制。
   */
  .head .cmd {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .copy {
    flex: none;
    background: none;
    border: 0;
    padding: 3px 8px;
    color: var(--text-dim);
    cursor: pointer;
    opacity: 0;
    font-size: 11px;
    white-space: nowrap;
  }
  .row:hover .copy { opacity: 1; }
  .copy:hover { color: var(--text); }

  .detail {
    padding: 4px 8px 8px 30px;
    font-size: 11px;
    color: var(--text-dim);
    background: var(--hover);
  }
  .detail .cwd { margin-bottom: 4px; }
  .detail pre {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono-font, ui-monospace, monospace);
    font-size: 11.5px;
    color: var(--text);
  }
  .detail .cut { margin-top: 4px; font-style: italic; }
</style>
