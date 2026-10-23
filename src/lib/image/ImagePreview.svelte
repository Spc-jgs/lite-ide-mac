<script lang="ts">
  /**
   * 图片预览（docs/TAKEOVER.md G26，IDEA 的图片编辑器）：PNG / JPEG / GIF / WebP / BMP / ICO / SVG 直接画出来。
   * 原来这些走日志模式 —— 里面有 NUL 字节、被判成「二进制文件」，满屏乱码。
   *
   * 字节走二进制 IPC（`read_image`），在页面里拼成 data URL：CSP 的 `img-src` 只放 `'self' data:`，
   * 用 `blob:` 或 Tauri 的 asset 协议都要放开 CSP。SVG 也是当 `<img>` 画 —— `<img>` 里的 SVG 不跑脚本、不拉外部资源，
   * 别人仓库里一份带脚本的 SVG 打开看一眼是安全的；**不要改成内联 `<svg>` 或 `<object>`**，那两种都会执行它。
   *
   * 底是棋盘格（透明的地方看得出是透明），默认「适应」（大图缩小、小图原大）；＋ − 按档跳、触控板捏合连续缩放、
   * 放大时按像素画（图标要看清每个像素，不要被插值糊掉）。SVG 能切到源码（「编辑源码」），源码那边状态栏「编辑模式 ⇄」切回来。
   *
   * 盘上的图被外部改了不会自己刷新（外部改动那条路只管编辑模式的文本标签）—— 点「重新载入」
   */
  import type { TabState } from "../state/tab";
  import { tabflow } from "../state/tabflow.svelte";
  import { imeKey } from "../shell/ime";
  import Icon from "../shell/Icon.svelte";
  import { bytes, fitScale, kind, openInPreview, percent, pinch, readImage, step } from "./zoom";
  import { notify } from "../state/notify.svelte";

  let { tab }: { tab: TabState } = $props();

  let src = $state("");
  let err = $state("");
  let size = $state(0);
  let nw = $state(0);
  let nh = $state(0);
  let vw = $state(0);
  let vh = $state(0);
  /** null = 适应（跟着岛的大小变）；数字 = 你按过 ＋ − 1:1 之后定下来的比例 */
  let zoom = $state<number | null>(null);
  let tick = $state(0);

  const mime = $derived(tab.reason || "image/png");
  const svg = $derived(mime === "image/svg+xml");
  /**
   * PDF（2026-10-23 定的）：不在页面里画 —— 要放开 CSP 的 object-src / frame-src，那条挡的是「页面里跑别人的东西」，为了预览不开口子。
   * 认出来（不再是日志模式的乱码）、说一句、给一个按钮交给系统的「预览」
   */
  const pdf = $derived(mime === "application/pdf");
  const preview = () => void openInPreview(tab.path).catch((e) => notify.fail(e instanceof Error ? e.message : String(e)));
  const fit = $derived(fitScale(nw, nh, vw, vh));
  const scale = $derived(zoom ?? fit);

  // 换了一张图（同一组里切到另一个图片标签，组件不重建）：缩放回到「适应」、尺寸等新图量
  let shown = "";
  $effect(() => {
    const path = tab.path;
    void tick;
    if (pdf) return;
    let dead = false;
    err = "";
    if (path !== shown) {
      shown = path;
      zoom = null;
      nw = nh = 0;
      src = "";
    }
    void readImage(path)
      .then(
        (buf) =>
          new Promise<string>((ok, bad) => {
            size = buf.byteLength;
            const r = new FileReader();
            r.onload = () => ok(String(r.result));
            r.onerror = () => bad(r.error);
            r.readAsDataURL(new Blob([buf], { type: mime }));
          }),
      )
      .then((url) => {
        if (!dead) src = url;
      })
      .catch((e) => {
        if (!dead) err = e instanceof Error ? e.message : String(e);
      });
    return () => (dead = true);
  });

  function loaded(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    // 只写了 viewBox、没写宽高的 SVG，WebKit 给的天然尺寸可能是 0：按 viewBox 的比例摆一个 300 宽的
    nw = img.naturalWidth || 300;
    nh = img.naturalHeight || 150;
  }

  function onWheel(e: WheelEvent) {
    // 触控板捏合在 WebKit 里是带 ctrlKey 的滚轮；⌘ 滚轮同样当缩放。普通滚动照常滚
    if (!e.ctrlKey && !e.metaKey) return;
    e.preventDefault();
    zoom = pinch(scale, e.deltaY);
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e) || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === "+" || e.key === "=") zoom = step(scale, 1);
    else if (e.key === "-") zoom = step(scale, -1);
    else if (e.key === "0") zoom = 1;
    else if (e.key === "f") zoom = null;
    else return;
    e.preventDefault();
  }
</script>

<div class="image">
  <div class="bar">
    <span class="info">
      {nw ? `${nw} × ${nh} · ` : ""}{bytes(size || tab.size)} · {kind(mime)}
    </span>
    <span class="gap"></span>
    {#if svg}
      <button class="btn sm quiet" title="看 / 改 SVG 的源码（状态栏「编辑模式 ⇄」切回预览）" onclick={() => void tabflow.requestSwitchMode(tab)}>编辑源码</button>
    {/if}
    <button class="btn sm quiet" title="交给系统的「预览」打开" onclick={preview}>在「预览」里打开</button>
    {#if !pdf}<button class="btn sm quiet" title="重新从盘上读一遍" onclick={() => tick++}>重新载入</button>{/if}
    <span class="segs">
      <button class="btn sm" class:on={zoom === null} title="大图缩到放得下，小图原大（F）" onclick={() => (zoom = null)}>适应</button>
      <button class="btn sm" class:on={zoom === 1} title="一个像素对一个像素（0）" onclick={() => (zoom = 1)}>1:1</button>
    </span>
    <button class="ibtn" aria-label="缩小" title="缩小（−）" disabled={scale <= 0.05} onclick={() => (zoom = step(scale, -1))}><Icon name="minus" /></button>
    <span class="pct">{percent(scale)}</span>
    <button class="ibtn" aria-label="放大" title="放大（＋）；触控板捏合也行" disabled={scale >= 32} onclick={() => (zoom = step(scale, 1))}><Icon name="plus" /></button>
  </div>
  <!-- 拿焦点是为了接 ＋ − 0 F 这几个键（点一下图就能按）；它本身不是控件 -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div class="stage" bind:clientWidth={vw} bind:clientHeight={vh} onwheel={onWheel} onkeydown={onKey} tabindex="0" role="img" aria-label={tab.name}>
    {#if pdf}
      <div class="empty-msg">
        PDF 不在这里画
        <div class="sub">要放开页面的安全策略才画得出来，没为它开口子 —— 交给系统的「预览」看</div>
        <button class="btn primary open" onclick={preview}>在「预览」里打开</button>
      </div>
    {:else if err}
      <div class="empty-msg bad">{err}</div>
    {:else if !src}
      <div class="empty-msg wait"><span class="spinner"></span> 正在读图…</div>
    {:else}
      <img
        {src}
        alt={tab.name}
        draggable="false"
        class:pixel={!svg && scale > 1}
        style:width={nw ? `${nw * scale}px` : null}
        style:height={nh ? `${nh * scale}px` : null}
        onload={loaded}
        onerror={() => (err = svg ? "这份 SVG 画不出来 —— 多半是写坏了，点「编辑源码」看看" : "这张图解不出来 —— 文件可能不完整")}
      />
    {/if}
  </div>
</div>

<style>
  .image { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .bar { flex: none; display: flex; align-items: center; gap: 6px; height: 34px; padding: 0 8px 0 12px; border-bottom: 1px solid var(--border-soft); font-size: var(--fs-sm); color: var(--text-dim); user-select: none; }
  .info { font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .gap { flex: 1; }
  .pct { min-width: 44px; text-align: center; font-variant-numeric: tabular-nums; color: var(--text-dim); }
  /* 两层 flex + margin: auto：比容器小时居中，比容器大时从左上角开始滚（justify-content: center 会把左边那截推到滚不到的地方） */
  .stage { flex: 1; min-height: 0; display: flex; overflow: auto; outline: none; }
  .stage > :global(*) { margin: auto; }
  img {
    flex: none;
    display: block;
    /* 棋盘格：透明的地方看得出是透明。两种灰都压得很暗，深色主题里不刺眼 */
    background-color: #2b2b2e;
    background-image: linear-gradient(45deg, #3a3a3e 25%, transparent 25%, transparent 75%, #3a3a3e 75%),
      linear-gradient(45deg, #3a3a3e 25%, transparent 25%, transparent 75%, #3a3a3e 75%);
    background-size: 16px 16px;
    background-position: 0 0, 8px 8px;
    box-shadow: 0 0 0 1px var(--border-soft);
  }
  img.pixel { image-rendering: pixelated; }
  .open { margin-top: 12px; }
</style>
