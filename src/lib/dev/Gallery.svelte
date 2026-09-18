<script lang="ts">
  /**
   * 组件画廊：`pnpm dev` 下开 `http://localhost:1420/?gallery`。
   *
   * 把已经定下来的通用控件全部摆在一屏上 —— 文字按钮 `.btn` 的每个变体、图标按钮 `.ibtn`
   * 的每个尺寸、`Icon` 的每一个名字、四个交互态的底色。用途两个：
   *
   * 1. **确认风格**：改了 app.css 里任何一条，来这儿看一眼全家福，而不是在应用里
   *    一个角落一个角落地找。
   * 2. **查漏补缺**：加新功能之前先来这儿看有没有现成的；加了新变体 / 新图标必须在这儿
   *    出现 —— 不在画廊里的控件，就是下一个「各写各的」。
   *
   * 只在 dev 分支上挂（main.ts 里 `import.meta.env.DEV && ?gallery`），生产包里没有它。
   * 不是 storybook：一个文件、零依赖、不带交互，够用就停。
   */
  import Icon, { ICON_NAMES } from "../shell/Icon.svelte";

  const TONES = [
    ["--hover", "悬停"],
    ["--selected", "选中 / 开关态"],
    ["--pressed", "按下 · xs 的悬停"],
    ["--accent", "主动作 / 焦点环"],
    ["--lvl-error", "不可逆"],
    ["--lvl-warn", "警告（受限、草稿存失败）"],
  ] as const;
</script>

<div class="gallery">
  <h1>通用控件 <small>app.css · Icon.svelte —— 改了这两处来这儿看全家福</small></h1>

  <section>
    <h2>文字按钮 <code>.btn</code> <small>尺寸 24 / <code>sm</code> 20；语义 默认 / <code>primary</code> / <code>danger</code> / <code>quiet</code>；组件里只写语义</small></h2>
    <div class="row">
      <button class="btn">默认</button>
      <button class="btn primary">主动作</button>
      <button class="btn danger">不可逆</button>
      <button class="btn quiet">安静</button>
      <button class="btn" disabled>默认 · 禁用</button>
      <button class="btn primary" disabled>主动作 · 禁用</button>
      <span class="btn-split"><button class="btn primary">提交</button><button class="btn primary"><Icon name="chevron-down" size={11} /></button></span>
    </div>
    <div class="row">
      <button class="btn sm">默认 sm</button>
      <button class="btn sm primary">主动作 sm</button>
      <button class="btn sm danger">不可逆 sm</button>
      <button class="btn sm quiet">安静 sm</button>
      <button class="btn sm" disabled>禁用 sm</button>
      <button class="btn sm"><Icon name="swap" size={11} /> 带图标</button>
    </div>
  </section>

  <section>
    <h2>图标按钮 <code>.ibtn</code> <small>24 头上工具 / <code>sm</code> 20 行内 / <code>xs</code> 16 标签上 / <code>lg</code> 26 只有导轨；<code>on</code> 开关态用中性白，accent 留给状态</small></h2>
    <div class="row">
      <span class="lbl">24</span>
      <button class="ibtn" title="默认"><Icon name="refresh" /></button>
      <button class="ibtn on" title="开关态"><Icon name="follow" /></button>
      <button class="ibtn" disabled title="禁用"><Icon name="collapse" /></button>
      <span class="lbl">sm</span>
      <button class="ibtn sm"><Icon name="plus" size={12} /></button>
      <button class="ibtn sm"><Icon name="minus" size={12} /></button>
      <button class="ibtn sm"><Icon name="undo" size={12} /></button>
      <button class="ibtn sm"><Icon name="more-h" size={12} /></button>
      <span class="lbl">xs</span>
      <span class="chip">标签 <button class="ibtn xs"><Icon name="x" size={10} /></button></span>
      <span class="chip on">当前 <button class="ibtn xs"><Icon name="pin" size={11} /></button></span>
      <span class="lbl">lg</span>
      <span class="rail"><button class="ibtn lg"><Icon name="files" /></button><button class="ibtn lg on"><Icon name="git" /></button><button class="ibtn lg"><Icon name="terminal" /></button></span>
    </div>
  </section>

  <section>
    <h2>图标 <code>Icon</code> <small>{ICON_NAMES.length} 个 · 16 网格、字形收在 2–14、描边 1.25、端点 round；默认 14px，行内 11–12，标签上的 ✕ 10</small></h2>
    <div class="icons">
      {#each ICON_NAMES as n (n)}
        <div class="ic"><Icon name={n} /><code>{n}</code></div>
      {/each}
    </div>
  </section>

  <section>
    <h2>交互态的底 <small>全是中性白叠加（`--hover` &lt; `--selected` &lt; `--pressed`），只有主动作和状态才上色</small></h2>
    <div class="row">
      {#each TONES as [v, label] (v)}
        <div class="tone"><span class="sw" style:background="var({v})"></span><code>{v}</code><small>{label}</small></div>
      {/each}
    </div>
  </section>
</div>

<style>
  .gallery {
    height: 100%;
    overflow: auto;
    padding: 24px 28px 48px;
    background: var(--content-bg);
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 12.5px;
  }
  h1 { font-size: 16px; margin: 0 0 18px; }
  h2 { font-size: 12.5px; margin: 0 0 10px; color: var(--text-dim); }
  h1 small, h2 small { font-weight: normal; color: var(--text-faint); margin-left: 8px; font-size: 11px; }
  code { font-family: var(--code-font); font-size: 11px; color: var(--text-dim); }
  section { margin-bottom: 26px; padding: 14px 16px; border: var(--island-border); border-radius: var(--island-radius); }
  .row { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; margin-bottom: 8px; }
  .lbl { color: var(--text-faint); font-family: var(--code-font); font-size: 10.5px; margin-left: 8px; }
  /* 标签块和导轨的仿真底：xs / lg 只在这两种容器里出现，摆出来才看得出 hover 对不对 */
  .chip { display: inline-flex; align-items: center; gap: 4px; height: 28px; padding: 0 6px 0 10px; border-radius: var(--r-sm); }
  .chip.on { background: var(--selected); }
  .rail { display: inline-flex; flex-direction: column; gap: 4px; padding: 4px; width: 34px; align-items: center; border-radius: var(--r-sm); background: rgba(255,255,255,0.03); }
  .icons { display: grid; grid-template-columns: repeat(auto-fill, minmax(112px, 1fr)); gap: 6px; }
  .ic { display: flex; align-items: center; gap: 8px; height: 28px; padding: 0 8px; border-radius: var(--r-sm); color: var(--text-dim); }
  .ic:hover { background: var(--hover); }
  .tone { display: flex; align-items: center; gap: 8px; }
  .sw { width: 28px; height: 20px; border-radius: var(--r-sm); border: 1px solid var(--border); }
  .tone small { color: var(--text-faint); }
</style>
