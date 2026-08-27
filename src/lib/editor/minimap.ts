/**
 * 代码缩略图。
 *
 * # 为什么自己画而不是拉个包
 *
 * CM6 官方没有缩略图，社区包（`@replit/codemirror-minimap`）的做法是**把整份
 * 文档再渲染一遍真 DOM 然后 CSS 缩放**。那意味着一份两万行的文件在页面上有两套
 * 两万行的 DOM，滚动时两套都要重排。canvas 版本只画矩形，代价与文档长度无关。
 *
 * # 瓦片缓存：为什么不是每帧重画
 *
 * 第一版是「每次滚动重画当前这一屏」。看着合理，实际很卡 —— 一帧里要
 * 重设 canvas 尺寸（等于重新分配并清空整块位图）、跑一遍 `highlightTree`、
 * 再发几千次 `fillRect`，加起来约 5ms，占掉 60fps 单帧预算的三分之一。
 *
 * 现在改成**瓦片**：一次把「当前位置上下共三屏」的内容渲染进离屏画布，
 * 滚动时只做一次 `drawImage` 把需要的那一段贴过来。滚出瓦片范围才重渲染，
 * 也就是每滚两屏才付一次渲染代价。
 *
 * # 长文档怎么办
 *
 * 短文档整份铺开（真·缩略图）。超过一屏画不下时**滑动**：只画一个窗口，
 * 窗口位置跟着编辑器滚动比例走。硬把十万行压进 800 像素，每行不到 0.01 像素，
 * 画出来是一团糊，不如不画。
 */

import { EditorView, ViewPlugin, type PluginValue, type ViewUpdate } from "@codemirror/view";
import { StateEffect, StateField } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";
import { highlightTree } from "@lezer/highlight";
import { minimapHighlighter, MINIMAP_DEFAULT } from "./theme-idea-dark";

/** 缩略图宽度（CSS px） */
const WIDTH = 88;
/** 改动标记条占的宽度，画在最左边 */
const MARK_W = 3;
/** 墨迹左边距，与改动标记条拉开 */
const INK_X = MARK_W + 3;
/** 每行占的高度 */
const LINE_H = 3;
/** 墨迹高度，比行高小一点才有行的间隙 */
const INK_H = 2;
/** 每个字符占的宽度 */
const CHAR_W = 1;
/** 一行最多画多少字符 */
const MAX_COLS = Math.floor((WIDTH - INK_X - 2) / CHAR_W);
/** 制表符按几个空格算 */
const TAB = 4;
/** 瓦片覆盖几屏：当前屏 + 上下各一屏 */
const TILE_SCREENS = 3;

/** 一行的改动类型，画在缩略图左缘 */
export type MarkKind = "add" | "mod" | "del";

/** 设置改动标记：行号（1-based）→ 类型 */
export const setMinimapMarks = StateEffect.define<Map<number, MarkKind>>();

const marksField = StateField.define<Map<number, MarkKind>>({
  create: () => new Map(),
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setMinimapMarks)) return e.value;
    return value;
  },
});

const MARK_COLOR: Record<MarkKind, string> = {
  add: "#63b76c",
  mod: "#4f9ee3",
  del: "#d1707a",
};

/** 读阶段量到的东西，原样交给写阶段 */
interface Measured {
  h: number;
  offset: number;
  visLines: number;
  docLines: number;
  topLine: number;
  botLine: number;
}

class MinimapPlugin implements PluginValue {
  private view: EditorView;
  private wrap: HTMLDivElement;
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null;
  private slider: HTMLDivElement;

  /** 离屏瓦片 */
  private tile = document.createElement("canvas");
  private tileFirst = 0;
  private tileLines = 0;
  /** 瓦片内容的失效键；对不上就重渲染 */
  private tileKey = "";
  /**
   * 文档版本号。
   *
   * 失效键**不能用 `doc.length`** —— 把一个字符换成另一个字符时长度不变，
   * 缩略图就再也不更新了。这个 bug 只在「改了但没增删字符」时出现，
   * 平时敲字（长度总在变）根本看不出来。
   */
  private docVersion = 0;
  /** 上一次的改动标记对象，按引用比 —— 条数没变但内容变了也要重画 */
  private lastMarks: Map<number, MarkKind> | null = null;

  private dragging = false;
  private pending = false;
  /** 可见画布当前的设备像素尺寸，没变就不重设（重设 = 重新分配位图） */
  private cw = 0;
  private ch = 0;

  constructor(view: EditorView) {
    this.view = view;
    this.wrap = document.createElement("div");
    this.wrap.className = "cm-minimap";
    this.canvas = document.createElement("canvas");
    this.ctx = this.canvas.getContext("2d");
    this.slider = document.createElement("div");
    this.slider.className = "cm-minimap-slider";
    this.wrap.append(this.canvas, this.slider);
    // 挂在 .cm-editor 上而不是 .cm-scroller 里：挂进滚动容器会跟着内容一起滚走
    view.dom.appendChild(this.wrap);

    this.wrap.addEventListener("pointerdown", this.onDown);
    /*
     * 直接听 scrollDOM 的 scroll，而不是等 ViewUpdate。
     * 视口内的小幅滚动不一定触发 viewportChanged（CM6 的视口带余量），
     * 而滑块必须跟得上每一次滚动，否则它看着就是「粘住了」。
     */
    view.scrollDOM.addEventListener("scroll", this.onScroll, { passive: true });
    this.schedule();
  }

  update(u: ViewUpdate) {
    if (u.docChanged) this.docVersion++;
    if (
      u.docChanged ||
      u.viewportChanged ||
      u.geometryChanged ||
      u.transactions.some((t) => t.effects.some((e) => e.is(setMinimapMarks)))
    ) {
      this.schedule();
    }
  }

  destroy() {
    this.wrap.removeEventListener("pointerdown", this.onDown);
    this.view.scrollDOM.removeEventListener("scroll", this.onScroll);
    window.removeEventListener("pointermove", this.onMove);
    window.removeEventListener("pointerup", this.onUp);
    this.wrap.remove();
  }

  private onScroll = () => this.schedule();

  /**
   * 排一次重绘。
   *
   * **不能在 update() 里直接读布局** —— CM6 会抛
   * "Reading the editor layout isn't allowed during an update"。
   * 它把一帧切成「读」和「写」两个阶段，正是为了避免读写交替触发的强制重排。
   */
  private schedule() {
    if (this.pending) return;
    this.pending = true;
    this.view.requestMeasure({
      read: () => this.measure(),
      write: (m) => {
        this.pending = false;
        if (m) this.paint(m);
      },
    });
  }

  // ── 交互：点一下跳过去，按住拖动连续滚 ──

  private onDown = (e: PointerEvent) => {
    e.preventDefault();
    this.dragging = true;
    this.wrap.classList.add("dragging");
    this.scrollToPointer(e);
    window.addEventListener("pointermove", this.onMove);
    window.addEventListener("pointerup", this.onUp);
  };
  private onMove = (e: PointerEvent) => {
    if (this.dragging) this.scrollToPointer(e);
  };
  private onUp = () => {
    this.dragging = false;
    this.wrap.classList.remove("dragging");
    window.removeEventListener("pointermove", this.onMove);
    window.removeEventListener("pointerup", this.onUp);
  };

  private scrollToPointer(e: PointerEvent) {
    const rect = this.wrap.getBoundingClientRect();
    const y = e.clientY - rect.top;
    const g = this.geometry(rect.height);
    const doc = this.view.state.doc;
    const line = Math.round(g.offset + y / LINE_H);
    const target = Math.min(Math.max(1, line), doc.lines);
    this.view.dispatch({
      effects: EditorView.scrollIntoView(doc.line(target).from, { y: "center" }),
    });
  }

  /**
   * 算出这一帧要画哪一段。
   *
   * `offset` 是窗口第一行的下标（0-based）。文档短到能整份画下时恒为 0；
   * 画不下时按编辑器的滚动比例走，让缩略图和真实内容对得上。
   */
  private geometry(h: number) {
    const docLines = this.view.state.doc.lines;
    const visLines = Math.max(1, Math.floor(h / LINE_H));
    if (docLines <= visLines) return { offset: 0, visLines, docLines };

    const sc = this.view.scrollDOM;
    const max = sc.scrollHeight - sc.clientHeight;
    const ratio = max > 0 ? Math.min(1, Math.max(0, sc.scrollTop / max)) : 0;
    return { offset: Math.round((docLines - visLines) * ratio), visLines, docLines };
  }

  /** 读阶段：只读布局，不碰 DOM */
  private measure(): Measured | null {
    const h = this.view.dom.clientHeight;
    if (h <= 0) return null;
    const doc = this.view.state.doc;
    const g = this.geometry(h);
    const sc = this.view.scrollDOM;
    const topLine = doc.lineAt(this.view.lineBlockAtHeight(sc.scrollTop).from).number;
    const botLine = doc.lineAt(
      this.view.lineBlockAtHeight(sc.scrollTop + sc.clientHeight).from,
    ).number;
    return { h, topLine, botLine, ...g };
  }

  /** 写阶段：只写 DOM 与 canvas，不读布局 */
  private paint(m: Measured) {
    const ctx = this.ctx;
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const cw = Math.round(WIDTH * dpr);
    const ch = Math.round(m.h * dpr);

    // 尺寸没变就别动 canvas.width —— 写它会重新分配位图并清空，很贵
    if (cw !== this.cw || ch !== this.ch) {
      this.canvas.width = cw;
      this.canvas.height = ch;
      this.canvas.style.width = `${WIDTH}px`;
      this.canvas.style.height = `${m.h}px`;
      this.cw = cw;
      this.ch = ch;
    }

    // 滑块：编辑器里当前看得见的是哪几行
    const top = Math.max(0, (m.topLine - 1 - m.offset) * LINE_H);
    this.slider.style.transform = `translateY(${top}px)`;
    this.slider.style.height = `${Math.max(8, (m.botLine - m.topLine + 1) * LINE_H)}px`;

    this.ensureTile(m, dpr);

    // 贴图：整帧的开销就这一次 drawImage
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, cw, ch);
    const sy = Math.round((m.offset - this.tileFirst) * LINE_H * dpr);
    const sh = Math.min(ch, this.tile.height - sy);
    if (sh > 0) ctx.drawImage(this.tile, 0, sy, cw, sh, 0, 0, cw, sh);
  }

  /** 需要的那一段不在瓦片里（或内容过期）就重渲染瓦片 */
  private ensureTile(m: Measured, dpr: number) {
    const state = this.view.state;
    const marks = state.field(marksField);
    const want = Math.min(m.docLines, m.visLines * TILE_SCREENS);
    // 让当前窗口大致落在瓦片中间，上下各留一屏的余量
    let first = Math.max(0, m.offset - m.visLines);
    first = Math.min(first, Math.max(0, m.docLines - want));

    const inRange =
      m.offset >= this.tileFirst && m.offset + m.visLines <= this.tileFirst + this.tileLines;
    const key = `${this.docVersion}|${want}|${dpr}`;
    if (inRange && key === this.tileKey && marks === this.lastMarks) return;

    this.tileFirst = first;
    this.tileLines = want;
    this.tileKey = key;
    this.lastMarks = marks;
    this.renderTile(dpr);
  }

  private renderTile(dpr: number) {
    const state = this.view.state;
    const doc = state.doc;
    const tile = this.tile;
    const h = this.tileLines * LINE_H;
    const w = Math.round(WIDTH * dpr);
    const th = Math.round(h * dpr);
    if (tile.width !== w || tile.height !== th) {
      tile.width = w;
      tile.height = th;
    }
    const ctx = tile.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, WIDTH, h);

    const first = this.tileFirst + 1;
    const last = Math.min(doc.lines, this.tileFirst + this.tileLines);
    if (first > last) return;
    const from = doc.line(first).from;
    const to = doc.line(last).to;

    // 先把这一段的颜色索引铺出来，再按行画 —— 比每个字符去树里查快得多
    const palette: string[] = [MINIMAP_DEFAULT];
    const paletteIdx = new Map<string, number>([[MINIMAP_DEFAULT, 0]]);
    const colorAt = new Uint8Array(Math.max(0, to - from));
    try {
      highlightTree(
        syntaxTree(state),
        minimapHighlighter,
        (f, t, cls) => {
          let i = paletteIdx.get(cls);
          if (i === undefined) {
            // Uint8Array 装不下超过 255 种颜色；一套主题也就十几种
            if (palette.length > 255) return;
            i = palette.length;
            palette.push(cls);
            paletteIdx.set(cls, i);
          }
          colorAt.fill(i, Math.max(0, f - from), Math.max(0, Math.min(to, t) - from));
        },
        from,
        to,
      );
    } catch {
      // 语法树还没解析到 / 没有语言扩展：全用兜底色，不影响其余部分
    }

    const marks = state.field(marksField);
    // 一次设好，别在内层循环里反复改画笔状态
    ctx.globalAlpha = 0.92;

    for (let n = first; n <= last; n++) {
      const line = doc.line(n);
      const y = (n - first) * LINE_H;
      const text = line.text;

      const mk = marks.get(n);
      if (mk) {
        ctx.fillStyle = MARK_COLOR[mk];
        ctx.fillRect(0, y, MARK_W, INK_H);
      }

      let col = 0;
      let runStart = -1;
      let runColor = 0;
      const flush = (endCol: number) => {
        if (runStart < 0) return;
        ctx.fillStyle = palette[runColor];
        ctx.fillRect(INK_X + runStart * CHAR_W, y, (endCol - runStart) * CHAR_W, INK_H);
        runStart = -1;
      };

      for (let i = 0; i < text.length && col < MAX_COLS; i++) {
        const ch = text[i];
        if (ch === "\t") {
          flush(col);
          col += TAB - (col % TAB);
          continue;
        }
        if (ch === " ") {
          flush(col);
          col++;
          continue;
        }
        const c = colorAt[line.from + i - from] ?? 0;
        if (runStart < 0) {
          runStart = col;
          runColor = c;
        } else if (c !== runColor) {
          flush(col);
          runStart = col;
          runColor = c;
        }
        col++;
      }
      flush(Math.min(col, MAX_COLS));
    }
    ctx.globalAlpha = 1;
  }
}

const minimapTheme = EditorView.theme({
  ".cm-minimap": {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
    width: `${WIDTH}px`,
    zIndex: "3",
    cursor: "pointer",
    borderLeft: "1px solid rgba(255,255,255,.06)",
    // 必须不透明：半透明时长行会从底下透出来，糊成一片
    background: "#1e1f22",
    overflow: "hidden",
  },
  ".cm-minimap canvas": { display: "block" },
  ".cm-minimap-slider": {
    position: "absolute",
    left: 0,
    right: 0,
    top: 0,
    background: "rgba(255,255,255,.06)",
    borderTop: "1px solid rgba(255,255,255,.09)",
    borderBottom: "1px solid rgba(255,255,255,.09)",
    pointerEvents: "none",
    // 用 transform 移动而不是改 top：避免每次滚动都触发布局
    willChange: "transform",
  },
  ".cm-minimap:hover .cm-minimap-slider": { background: "rgba(255,255,255,.11)" },
  ".cm-minimap.dragging .cm-minimap-slider": { background: "rgba(53,116,240,.22)" },
  // 给缩略图让出位置，否则长行会跑到它底下
  ".cm-scroller": { paddingRight: `${WIDTH}px` },
});

export function minimap() {
  return [marksField, ViewPlugin.fromClass(MinimapPlugin), minimapTheme];
}
