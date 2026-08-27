/**
 * 代码缩略图。
 *
 * # 为什么自己画而不是拉个包
 *
 * CM6 官方没有缩略图，社区包（`@replit/codemirror-minimap`）的做法是**把整份
 * 文档再渲染一遍真 DOM 然后 CSS 缩放**。那意味着一份两万行的文件在页面上有两套
 * 两万行的 DOM，滚动时两套都要重排。canvas 版本只画当前窗口里那几百行的矩形，
 * 一次重绘是几百次 `fillRect`，代价与文档长度无关。
 *
 * 这跟日志引擎里「虚拟滚动 + 只渲染视口」是同一条思路。
 *
 * # 长文档怎么办
 *
 * 短文档整份铺开（真·缩略图）。超过一屏画不下时**滑动**：只画一个窗口，
 * 窗口位置跟着编辑器滚动比例走。VSCode 也是这么做的 —— 硬把十万行压进
 * 800 像素，每行不到 0.01 像素，画出来是一团糊，不如不画。
 */

import { EditorView, ViewPlugin, type PluginValue, type ViewUpdate } from "@codemirror/view";
import { StateEffect, StateField } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";
import { highlightTree } from "@lezer/highlight";
import { minimapHighlighter, MINIMAP_DEFAULT } from "./theme-idea-dark";

/** 缩略图宽度（CSS px）。约等于 84 个字符，够看出代码的形状 */
const WIDTH = 84;
/** 每行占的高度 */
const LINE_H = 3;
/** 墨迹高度，比行高小一点才有行的间隙 */
const INK_H = 2;
/** 每个字符占的宽度 */
const CHAR_W = 1;
/** 一行最多画多少字符 —— 再长也超出画布了 */
const MAX_COLS = Math.floor(WIDTH / CHAR_W);
/** 制表符按几个空格算 */
const TAB = 4;

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
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null;
  private wrap: HTMLDivElement;
  private slider: HTMLDivElement;
  private view: EditorView;
  private dragging = false;
  /** 上一帧的关键参数，没变就不重画 */
  private lastKey = "";
  /** 已经排了一次测量，别重复排 */
  private pending = false;

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
    this.schedule();
  }

  update(u: ViewUpdate) {
    if (u.docChanged || u.viewportChanged || u.geometryChanged || u.transactions.length) {
      this.schedule();
    }
  }

  /**
   * 排一次重绘。
   *
   * **不能在 update() 里直接读布局** —— CM6 会抛
   * "Reading the editor layout isn't allowed during an update"。
   * 它把一帧切成「读」和「写」两个阶段，正是为了避免读写交替触发的强制重排。
   * 我们要的 scrollTop / clientHeight / lineBlockAtHeight 全是读，
   * 必须排进 read 阶段；画 canvas 是写，排进 write 阶段。
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

  destroy() {
    this.wrap.removeEventListener("pointerdown", this.onDown);
    window.removeEventListener("pointermove", this.onMove);
    window.removeEventListener("pointerup", this.onUp);
    this.wrap.remove();
  }

  // ── 交互：点一下跳过去，按住拖动连续滚 ──

  private onDown = (e: PointerEvent) => {
    e.preventDefault();
    this.dragging = true;
    this.scrollToPointer(e);
    window.addEventListener("pointermove", this.onMove);
    window.addEventListener("pointerup", this.onUp);
  };
  private onMove = (e: PointerEvent) => {
    if (this.dragging) this.scrollToPointer(e);
  };
  private onUp = () => {
    this.dragging = false;
    window.removeEventListener("pointermove", this.onMove);
    window.removeEventListener("pointerup", this.onUp);
  };

  private scrollToPointer(e: PointerEvent) {
    const rect = this.wrap.getBoundingClientRect();
    const y = e.clientY - rect.top;
    const g = this.geometry(rect.height);
    const line = Math.round(g.offset + y / LINE_H);
    const doc = this.view.state.doc;
    const target = Math.min(Math.max(1, line), doc.lines);
    const pos = doc.line(target).from;
    this.view.dispatch({
      effects: EditorView.scrollIntoView(pos, { y: "center" }),
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
    return {
      offset: Math.round((docLines - visLines) * ratio),
      visLines,
      docLines,
    };
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
    const h = m.h;
    const state = this.view.state;
    const doc = state.doc;
    const g = { offset: m.offset, visLines: m.visLines, docLines: m.docLines };
    const dpr = window.devicePixelRatio || 1;

    this.slider.style.top = `${Math.max(0, (m.topLine - 1 - g.offset) * LINE_H)}px`;
    this.slider.style.height = `${Math.max(6, (m.botLine - m.topLine + 1) * LINE_H)}px`;

    // 参数没变就不重画。滚动时 update 触发得很密，重画是这里最贵的一步
    const key = `${doc.length}|${g.offset}|${g.visLines}|${h}|${dpr}|${state.field(marksField).size}`;
    if (key === this.lastKey) return;
    this.lastKey = key;

    this.canvas.width = WIDTH * dpr;
    this.canvas.height = h * dpr;
    this.canvas.style.width = `${WIDTH}px`;
    this.canvas.style.height = `${h}px`;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, WIDTH, h);

    const first = g.offset + 1;
    const last = Math.min(g.docLines, g.offset + g.visLines);
    if (first > last) return;
    const from = doc.line(first).from;
    const to = doc.line(last).to;

    // 先把这一窗的颜色索引铺出来，再按行画 —— 比每个字符去树里查快得多
    const palette: string[] = [MINIMAP_DEFAULT];
    const paletteIdx = new Map<string, number>([[MINIMAP_DEFAULT, 0]]);
    const colorAt = new Uint8Array(to - from);
    try {
      highlightTree(
        syntaxTree(state),
        minimapHighlighter,
        (f, t, cls) => {
          let i = paletteIdx.get(cls);
          if (i === undefined) {
            i = palette.length;
            // Uint8Array 装不下超过 255 种颜色；实际上一套主题也就十几种
            if (i > 255) return;
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

    for (let n = first; n <= last; n++) {
      const line = doc.line(n);
      const y = (n - first) * LINE_H;
      const text = line.text;

      // 改动标记画在最左边两像素，与代码墨迹分开
      const mk = marks.get(n);
      if (mk) {
        ctx.fillStyle = MARK_COLOR[mk];
        ctx.fillRect(0, y, 2, INK_H);
      }

      let col = 0;
      let runStart = -1;
      let runColor = 0;
      const flush = (endCol: number) => {
        if (runStart < 0) return;
        ctx.fillStyle = palette[runColor];
        ctx.globalAlpha = 0.75;
        ctx.fillRect(3 + runStart * CHAR_W, y, (endCol - runStart) * CHAR_W, INK_H);
        ctx.globalAlpha = 1;
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
    // 必须是不透明的：半透明时长行会从底下透出来，糊成一片
    background: "#1e1f22",
  },
  ".cm-minimap canvas": { display: "block" },
  ".cm-minimap-slider": {
    position: "absolute",
    left: 0,
    right: 0,
    background: "rgba(255,255,255,.075)",
    borderTop: "1px solid rgba(255,255,255,.10)",
    borderBottom: "1px solid rgba(255,255,255,.10)",
    pointerEvents: "none",
  },
  ".cm-minimap:hover .cm-minimap-slider": { background: "rgba(255,255,255,.12)" },
  // 给缩略图让出位置，否则长行会跑到它底下
  ".cm-scroller": { paddingRight: `${WIDTH}px` },
});

export function minimap() {
  return [marksField, ViewPlugin.fromClass(MinimapPlugin), minimapTheme];
}
