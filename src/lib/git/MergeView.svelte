<script lang="ts">
  import { parseConflicts, compose, unresolved, type Block, type ConflictBlock } from "./conflict";

  let {
    text,
    path,
    onResolve,
  }: {
    /** 工作区里那份带冲突标记的原文 */
    text: string;
    path: string;
    /** 写回文件；resolved 为真表示所有冲突都决定完了，可以 git add */
    onResolve: (content: string, resolved: boolean) => void;
  } = $props();

  let blocks = $state<Block[]>([]);
  /** 上下文最多显示几行；中间省略，不然一个小冲突要翻半屏 */
  const CTX = 3;

  // 换文件就重新解析。以工作区文件为准 —— 用户可能已经手改过一部分
  $effect(() => {
    blocks = parseConflicts(text);
  });

  let left = $derived(unresolved(blocks));
  let conflicts = $derived(blocks.filter((b) => b.kind === "conflict") as ConflictBlock[]);
  let oursLabel = $derived(conflicts[0]?.oursLabel || "我的");
  let theirsLabel = $derived(conflicts[0]?.theirsLabel || "他们的");

  function choose(b: ConflictBlock, pick: ConflictBlock["pick"]) {
    // 再点一次同一个选项 = 撤销这次选择，回到未决定
    b.pick = b.pick === pick ? null : pick;
  }

  function all(pick: "ours" | "theirs") {
    for (const b of blocks) if (b.kind === "conflict") b.pick = pick;
  }

  function save() {
    onResolve(compose(blocks), left === 0);
  }

  /** 普通段落只显示首尾各 CTX 行，中间折起来 */
  function trim(lines: string[], head: boolean, tail: boolean) {
    if (!head && !tail) return { top: lines, hidden: 0, bottom: [] as string[] };
    if (lines.length <= CTX * 2) return { top: lines, hidden: 0, bottom: [] as string[] };
    return {
      top: head ? lines.slice(0, CTX) : [],
      hidden: lines.length - (head ? CTX : 0) - (tail ? CTX : 0),
      bottom: tail ? lines.slice(-CTX) : [],
    };
  }

  const isFirst = (i: number) => i === 0;
  const isLast = (i: number) => i === blocks.length - 1;
</script>

<div class="merge">
  <div class="bar">
    <span class="path" title={path}>{path}</span>
    <span class="gap"></span>
    {#if left > 0}
      <span class="left">还有 {left} 处未决定</span>
    {:else}
      <span class="done">全部已决定</span>
    {/if}
    <button onclick={() => all("ours")}>全取{oursLabel}</button>
    <button onclick={() => all("theirs")}>全取{theirsLabel}</button>
    <button class="primary" onclick={save}>
      {left === 0 ? "保存并标记已解决" : "保存进度"}
    </button>
  </div>

  <div class="body">
    {#each blocks as b, i (i)}
      {#if b.kind === "plain"}
        {@const t = trim(b.lines, !isFirst(i), !isLast(i))}
        <div class="ctx">
          {#each t.top as l}<div class="cl">{l || " "}</div>{/each}
          {#if t.hidden > 0}
            <div class="fold">⋯ 省略 {t.hidden} 行未冲突内容 ⋯</div>
          {/if}
          {#each t.bottom as l}<div class="cl">{l || " "}</div>{/each}
        </div>
      {:else}
        <div class="cf" class:picked={b.pick !== null}>
          <div class="cfhead">
            <span class="tag">冲突</span>
            {#if b.pick}
              <span class="chose">
                已取{b.pick === "ours" ? oursLabel : b.pick === "theirs" ? theirsLabel : b.pick === "both" ? "两边" : "共同祖先"}
              </span>
            {/if}
            <span class="gap"></span>
            {#if b.base}
              <button class:on={b.pick === "base"} onclick={() => choose(b, "base")}>共同祖先</button>
            {/if}
            <button class:on={b.pick === "both"} onclick={() => choose(b, "both")}>两边都要</button>
          </div>
          <div class="sides">
            <div class="side ours" class:dim={b.pick !== null && b.pick !== "ours" && b.pick !== "both"}>
              <button class="pick" class:on={b.pick === "ours"} onclick={() => choose(b, "ours")}>
                <span class="nm">{oursLabel}</span>
                <span class="hint">{b.pick === "ours" ? "✓ 已取" : "取这边"}</span>
              </button>
              {#each b.ours as l}<div class="cl">{l || " "}</div>{/each}
              {#if b.ours.length === 0}<div class="cl empty">（空）</div>{/if}
            </div>
            <div class="side theirs" class:dim={b.pick !== null && b.pick !== "theirs" && b.pick !== "both"}>
              <button class="pick" class:on={b.pick === "theirs"} onclick={() => choose(b, "theirs")}>
                <span class="nm">{theirsLabel}</span>
                <span class="hint">{b.pick === "theirs" ? "✓ 已取" : "取这边"}</span>
              </button>
              {#each b.theirs as l}<div class="cl">{l || " "}</div>{/each}
              {#if b.theirs.length === 0}<div class="cl empty">（空）</div>{/if}
            </div>
          </div>
        </div>
      {/if}
    {/each}
    {#if conflicts.length === 0}
      <div class="none">这个文件里没有冲突标记了 —— 可能已经解决过。直接保存即可标记为已解决。</div>
    {/if}
  </div>
</div>

<style>
  .merge { display: flex; flex-direction: column; height: 100%; overflow: hidden; background: var(--editor-bg); }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 10px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    font-size: 11.5px;
    color: var(--text-dim);
    user-select: none;
  }
  .bar .path { font-family: var(--code-font); color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar .gap { flex: 1; }
  .bar .left { color: var(--lvl-warn); }
  .bar .done { color: var(--diff-add-fg); }
  .bar button {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-dim);
    font-size: 11px;
    padding: 2px 9px;
    cursor: default;
  }
  .bar button:hover { background: var(--panel-bg-2); color: var(--text); }
  .bar button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }

  .body { flex: 1; overflow: auto; font-family: var(--code-font); font-size: 12.5px; line-height: 19px; }
  .cl { white-space: pre; padding: 0 12px; color: var(--text-dim); }
  .cl.empty { color: var(--text-faint); font-style: italic; font-family: var(--ui-font); }
  .ctx .cl { color: var(--text-faint); }
  .fold {
    padding: 2px 12px;
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 11px;
    background: var(--panel-bg);
    border-top: 1px solid var(--border-soft);
    border-bottom: 1px solid var(--border-soft);
    user-select: none;
  }

  .cf {
    margin: 6px 0;
    border: 1px solid var(--lvl-warn);
    border-radius: 4px;
    overflow: hidden;
  }
  /* 决定完的块褪成中性色：眼睛该被还没处理的那些吸引过去 */
  .cf.picked { border-color: var(--border); }
  .cfhead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 8px;
    background: var(--panel-bg);
    font-family: var(--ui-font);
    font-size: 11px;
    color: var(--text-faint);
    user-select: none;
  }
  .cfhead .gap { flex: 1; }
  .cfhead .tag { color: var(--lvl-warn); }
  .cf.picked .cfhead .tag { color: var(--text-faint); }
  .cfhead .chose { color: var(--diff-add-fg); }
  .cfhead button {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-faint);
    font-size: 10.5px;
    padding: 1px 7px;
    cursor: default;
  }
  .cfhead button:hover, .cfhead button.on { background: var(--accent-sel); color: var(--text); }

  .sides { display: grid; grid-template-columns: 1fr 1fr; }
  .side { min-width: 0; overflow-x: auto; }
  .side.ours { background: var(--diff-del-bg); border-right: 1px solid var(--border); }
  .side.theirs { background: var(--diff-add-bg); }
  /* 没选中的那边淡下去，但不隐藏 —— 还要能对照着看 */
  .side.dim { opacity: 0.42; }
  .pick {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 2px 12px;
    background: var(--panel-bg);
    border: none;
    border-bottom: 1px solid var(--border-soft);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 10.5px;
    cursor: default;
  }
  .pick .nm { font-family: var(--code-font); }
  /* 「取这边」平时不出现，鼠标到了才浮出来 —— 标题栏保持安静，
     但一靠近就明确告诉你这一整条是可以点的 */
  .pick .hint { margin-left: auto; opacity: 0; color: var(--accent); }
  .pick:hover { background: var(--panel-bg-2); color: var(--text); }
  .pick:hover .hint { opacity: 1; }
  .pick.on { color: var(--text); background: var(--accent-sel); }
  .pick.on .hint { opacity: 1; color: var(--diff-add-fg); }
  .none { padding: 24px; text-align: center; color: var(--text-faint); font-family: var(--ui-font); font-size: 12.5px; }
</style>
