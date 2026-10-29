<script lang="ts">
  import { cmenu } from "./context-menu.svelte";
  import { imeKey } from "./ime";
  let { label, value, min, max, defaultValue, horizontal = false, reverse = false, onchange, ondrag = (_v: boolean) => {} }: {
    label: string; value: number; min: number; max: number; defaultValue: number;
    horizontal?: boolean; reverse?: boolean; onchange: (value: number) => void; ondrag?: (active: boolean) => void;
  } = $props();
  let node: HTMLDivElement;
  let drag: { id: number; at: number; value: number } | null = null;
  let active = $state(false);
  let menu = $state<{ x: number; y: number } | null>(null);
  const set = (n: number) => onchange(Math.round(Math.max(min, Math.min(max, n))));
  function end() {
    const id = drag?.id;
    drag = null;
    active = false;
    ondrag(false);
    if (id !== undefined && node?.hasPointerCapture(id)) node.releasePointerCapture(id);
  }
  function start(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    node.focus({ preventScroll: true });
    drag = { id: e.pointerId, at: horizontal ? e.clientY : e.clientX, value };
    node.setPointerCapture(e.pointerId);
    active = true;
    ondrag(true);
  }
  function move(e: PointerEvent) {
    if (!drag || drag.id !== e.pointerId) return;
    set(drag.value + ((horizontal ? e.clientY : e.clientX) - drag.at) * (reverse ? -1 : 1));
  }
  function key(e: KeyboardEvent) {
    if (imeKey(e)) return;
    const keys = horizontal ? ["ArrowUp", "ArrowDown"] : ["ArrowLeft", "ArrowRight"];
    const i = keys.indexOf(e.key);
    if (i < 0 && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault();
    e.stopPropagation();
    set(e.key === "Home" ? min : e.key === "End" ? max : value + (i === 0 ? -1 : 1) * (reverse ? -1 : 1) * (e.shiftKey ? 32 : 8));
  }
  function open(e: MouseEvent) {
    e.preventDefault();
    cmenu.load();
    menu = { x: e.clientX, y: e.clientY };
  }
  $effect(() => () => end());
</script>

<svelte:window onblur={end} />
<!-- separator 的可调数值、焦点与方向键是其交互本身。 -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div bind:this={node} class="splitter" class:horizontal class:active role="separator" tabindex="0"
  aria-label={label} aria-orientation={horizontal ? "horizontal" : "vertical"}
  aria-valuemin={min} aria-valuemax={max} aria-valuenow={Math.round(value)} aria-valuetext={`${Math.round(value)} 像素`}
  title={`${label} · 方向键调整 · 双击恢复 · 右键更多操作`}
  onpointerdown={start} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={end}
  onkeydown={key} ondblclick={() => set(defaultValue)} oncontextmenu={open}></div>
{#if menu && cmenu.comp}
  <cmenu.comp x={menu.x} y={menu.y} {label} onclose={() => menu = null} items={[
    { label: "增大 32 像素", disabled: value >= max, run: () => set(value + 32) },
    { label: "减小 32 像素", disabled: value <= min, run: () => set(value - 32) },
    { label: "恢复默认尺寸", sep: true, run: () => set(defaultValue) },
  ]} />
{/if}

<style>
  .splitter { position: relative; flex: none; width: 6px; min-height: 0; cursor: col-resize; touch-action: none; user-select: none; z-index: 1; outline-offset: -1px; }
  .splitter::after { content: ""; position: absolute; inset: 4px 2px; background: transparent; border-radius: 1px; }
  .splitter::before { content: ""; position: absolute; inset: 0 -2px; }
  .splitter.horizontal { width: auto; height: 6px; cursor: row-resize; }
  .horizontal::after { inset: 2px 4px; }
  .horizontal::before { inset: -2px 0; }
  .splitter:hover::after, .splitter:focus-visible::after, .splitter.active::after { background: var(--accent); }
</style>
