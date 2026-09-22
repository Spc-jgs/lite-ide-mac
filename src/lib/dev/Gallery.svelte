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
    <h2>等待 <code>.spinner</code> / <code>.busy</code> <small>同一个环；150ms 之内不出现（这一页是常亮的，所以你看到的是 150ms 之后的样子）；busy 的按钮宽高不变、和 disabled 一起写</small></h2>
    <div class="row">
      <span class="lbl">spinner</span>
      <span class="spinner"></span>
      <span class="spinner sm"></span>
      <span class="loading"><span class="spinner"></span>正在载入终端…</span>
      <span class="lbl">btn.busy</span>
      <button class="btn busy" disabled>默认</button>
      <button class="btn primary busy" disabled>主动作</button>
      <button class="btn danger busy" disabled>不可逆</button>
      <button class="btn sm primary busy" disabled>主动作 sm</button>
      <span class="btn-split"><button class="btn primary busy" disabled>提交 (3)</button><button class="btn primary" disabled><Icon name="chevron-down" size={11} /></button></span>
      <span class="lbl">ibtn.busy</span>
      <button class="ibtn busy" disabled title="刷新中"><Icon name="refresh" /></button>
    </div>
  </section>

  <section>
    <h2>进度条 <code>.pbar</code> <small>3px 圆角细条，轨道 --hover、填充 accent；<code>indet</code> 来回跑（不知道多久，不能画成 0%）。后台任务全在状态栏一格里：文字 · 条 · 百分比 · 取消</small></h2>
    <div class="row">
      <span class="lbl">62%</span><span class="pbar w"><span class="pfill" style:width="62%"></span></span>
      <span class="lbl">indet</span><span class="pbar w indet"></span>
      <span class="lbl">状态栏</span>
      <span class="sbdemo">
        <span class="task"><span class="tlabel">推送 · Writing objects</span><span class="pbar"><span class="pfill" style:width="62%"></span></span><span class="tpct">62%</span><button class="ibtn xs"><Icon name="x" size={10} /></button></span>
        <span class="vsep"></span>
        <span class="task"><span class="tlabel">正在提交…</span><span class="pbar indet"></span></span>
      </span>
    </div>
  </section>

  <section>
    <h2>确认卡片 <code>.confirm</code> <small>无色 = 选择题；<code>warn</code> = 要你决定；<code>bad</code> = 不可逆 / 没做成。主动作最右，取消紧挨其左，越不可逆越靠左</small></h2>
    <div class="cards">
      <div class="confirm"><span><b>README.md</b> 有未保存的改动</span><span class="gap"></span><button class="btn">丢弃改动</button><button class="btn">取消</button><button class="btn primary">保存并关闭</button></div>
      <div class="confirm warn"><span><b>README.md</b> 在编辑器外被改过，而你这边也有未保存的改动</span><span class="gap"></span><button class="btn">用磁盘上的</button><button class="btn primary">保留我的</button></div>
      <div class="confirm bad"><span>要丢弃 <b>src/App.svelte</b> 的改动吗？<b>这一步不可撤销</b></span><span class="gap"></span><button class="btn">取消</button><button class="btn danger">丢弃</button></div>
    </div>
  </section>

  <section>
    <h2>浮层的面 <code>.popup</code> / <code>.scrim</code> <small>底、5% 边、r-md、投影 + 内高光、90ms 淡入；位置和尺寸各组件自己管。右键菜单用同一个面但不淡入</small></h2>
    <div class="row">
      <div class="popup demo">浮层 / 菜单 / 弹窗</div>
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
  .loading { display: inline-flex; align-items: center; gap: 8px; color: var(--text-faint); font-size: 12px; }
  .cards { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; }
  .pbar.w { width: 160px; }
  /* 状态栏那格的仿真：照 StatusBar.svelte 的 .task 抄尺寸，改那边记得改这边 */
  .sbdemo { display: inline-flex; align-items: center; gap: 16px; height: 24px; padding: 0 12px; background: var(--chrome-scrim); border-radius: var(--r-sm); font-size: 11.5px; }
  .sbdemo .task { display: inline-flex; align-items: center; gap: 8px; color: var(--text-dim); }
  .sbdemo .pbar { width: 72px; }
  .sbdemo .tpct { font-size: 10.5px; color: var(--text-faint); font-family: var(--code-font); }
  .sbdemo .vsep { width: 1px; height: 11px; background: var(--border); }
  .popup.demo { width: 160px; padding: 14px; font-size: 12px; color: var(--text-dim); }
  .tone { display: flex; align-items: center; gap: 8px; }
  .sw { width: 28px; height: 20px; border-radius: var(--r-sm); border: 1px solid var(--border); }
  .tone small { color: var(--text-faint); }
</style>
