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
  import Toast from "../shell/Toast.svelte";
  import { notify } from "../state/notify.svelte";

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
    <h2>文字按钮 <code>.btn</code> <small>尺寸 28 / <code>sm</code> 24，字号 13 / 12；语义 默认（实心浅灰）/ <code>primary</code> / <code>danger</code>（你要做的就是删）/ <code>destructive</code>（红字：会丢东西、但不是你点的那个动作）/ <code>quiet</code>；组件里只写语义</small></h2>
    <div class="row">
      <button class="btn">默认</button>
      <button class="btn primary">主动作</button>
      <button class="btn danger">不可逆</button>
      <button class="btn destructive">不保存</button>
      <button class="btn quiet">安静</button>
      <button class="btn" disabled>默认 · 禁用</button>
      <button class="btn primary" disabled>主动作 · 禁用</button>
      <span class="btn-split"><button class="btn primary">提交</button><button class="btn primary"><Icon name="chevron-down" /></button></span>
      <span class="lbl">segs</span>
      <span class="segs"><button class="btn sm on">双栏</button><button class="btn sm">统一</button></span>
    </div>
    <div class="row">
      <button class="btn sm">默认 sm</button>
      <button class="btn sm primary">主动作 sm</button>
      <button class="btn sm danger">不可逆 sm</button>
      <button class="btn sm quiet">安静 sm</button>
      <button class="btn sm" disabled>禁用 sm</button>
      <button class="btn sm"><Icon name="swap" /> 带图标</button>
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
      <button class="ibtn sm"><Icon name="plus" /></button>
      <button class="ibtn sm"><Icon name="minus" /></button>
      <button class="ibtn sm"><Icon name="undo" /></button>
      <button class="ibtn sm"><Icon name="more-h" /></button>
      <span class="lbl">xs</span>
      <span class="chip">标签 <button class="ibtn xs"><Icon name="x" /></button></span>
      <span class="chip on">当前 <button class="ibtn xs"><Icon name="pin" /></button></span>
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
      <span class="btn-split"><button class="btn primary busy" disabled>提交 (3)</button><button class="btn primary" disabled><Icon name="chevron-down" /></button></span>
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
        <span class="task"><span class="tlabel">推送 · Writing objects</span><span class="pbar"><span class="pfill" style:width="62%"></span></span><span class="tpct">62%</span><button class="ibtn xs"><Icon name="x" /></button></span>
        <span class="vsep"></span>
        <span class="task"><span class="tlabel">正在提交…</span><span class="pbar indet"></span></span>
      </span>
    </div>
  </section>

  <section>
    <h2>对话框 <code>.dialog</code> <small>你刚做的那个动作引起的选择题：编辑岛压暗、标题是问句、一句后果；会丢东西的（不是你点的那个）最左红字，取消挨着主按钮，主按钮最右。焦点给 <code>data-default</code></small></h2>
    <div class="cards">
      <div class="dialog">
        <p class="title">要保存对 README.md 的改动吗？</p>
        <p class="body">不保存的话，这次的改动会丢掉。</p>
        <div class="actions"><button class="btn destructive">不保存</button><span class="gap"></span><button class="btn">取消</button><button class="btn primary">保存</button></div>
      </div>
      <div class="dialog">
        <p class="title">丢弃 src/App.svelte 的改动？</p>
        <p class="body">改过的文件回到上次提交的样子，<b>这一步不可撤销</b>。</p>
        <div class="actions"><span class="gap"></span><button class="btn">取消</button><button class="btn danger">丢弃</button></div>
      </div>
    </div>
  </section>

  <section>
    <h2>卡片 <code>.confirm</code> <small>告诉你一件事、或启动时冒出来的（不压暗）。无色 = 普通；<code>warn</code> = 要你决定；<code>bad</code> = 出错 / 没做成</small></h2>
    <div class="cards">
      <div class="confirm"><span>已替换 12 个文件 48 处</span><span class="gap"></span><button class="btn">知道了</button><button class="btn primary">撤销</button></div>
      <div class="confirm warn"><span><b>README.md</b> 在编辑器外被改过，而你这边也有未保存的改动</span><span class="gap"></span><button class="btn">用磁盘上的</button><button class="btn primary">保留我的</button></div>
      <div class="confirm bad"><span><b>推送被拒了：远程有你本地没有的提交</b></span><span class="gap"></span><button class="btn">知道了</button><button class="btn primary">先拉取</button></div>
    </div>
  </section>

  <section>
    <h2>提示胶囊 <code>Toast</code> / <code>notify</code> <small>编辑区底部居中，一次一条（ui.md 第十五条）：<code>ok</code> 主色 ✓ = 做成了 / <code>fail</code> 红 ✕ = 没做成（不被随后的 ok 顶掉）/ <code>hint</code> 灰、无图标 = 什么都没发生。停多久按字数，鼠标停上去不走，点一下关</small></h2>
    <div class="row">
      <button class="btn sm" onclick={() => notify.ok("已复制路径：/proj/pom.xml")}>ok</button>
      <button class="btn sm" onclick={() => notify.fail("推送失败：远程拒绝了这次推送")}>fail</button>
      <button class="btn sm" onclick={() => notify.hint("剪贴板里没有文字")}>hint</button>
    </div>
    <div class="toastbox"><Toast /></div>
  </section>

  <section>
    <h2>空态 <code>.empty-msg</code> / 刚做完 <code>.btn.done</code> <small>为什么空 + 下一步；<code>.sub</code> 补充、<code>.bad</code> 拿不到。按钮原地说「✓ 已提交」1.4 秒，主色不灰、宽度不变</small></h2>
    <div class="row" style="align-items: stretch">
      <div class="emptybox"><div class="empty-msg">没有找到 —— 换个词，或者看看上面三个开关</div></div>
      <div class="emptybox"><div class="empty-msg">这个目录还不是 Git 仓库<br /><span class="sub">在终端里（⌘J）<code>git init</code>，这里会自己出现</span></div></div>
      <div class="emptybox"><div class="empty-msg bad">fatal: not a git repository</div></div>
    </div>
    <div class="row">
      <button class="btn primary" disabled>提交 (0)</button>
      <button class="btn primary done" disabled><Icon name="check" /> 已提交</button>
    </div>
  </section>

  <section>
    <h2>浮层的面 <code>.popup</code> / <code>.scrim</code> <small>比内容亮一档的实心底（--elevated）、r-md 12、投影 + <code>--pop-ring</code>（0.5px 外描边 + 顶边高光）、90ms 淡入；居中的位置和宽度是 <code>.palette s/m/l</code>（上沿一条线、440 / 640 / 820），挂在控件下的组件自己按锚点放。右键菜单用同一个面但不淡入，选中项强调色白字、快捷键右对齐</small></h2>
    <div class="row">
      <div class="popup demo">浮层 / 菜单 / 弹窗</div>
    </div>
  </section>

  <section>
    <h2>图标 <code>Icon</code> <small>{ICON_NAMES.length} 个 · JetBrains 新 UI 官方图标（来源见 public/icons/SOURCES.md）；一律 16，组件里不传尺寸；单色走 mask 跟着 color 变，彩色的画原色</small></h2>
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
  /* 胶囊的定位父级：照编辑岛（position: relative），胶囊贴它的底边 */
  .toastbox { position: relative; height: 72px; border-radius: var(--r-sm); background: var(--content-bg); }
  .emptybox { flex: 1; min-width: 200px; border-radius: var(--r-sm); background: var(--elevated); }
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
