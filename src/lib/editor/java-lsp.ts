/**
 * Java 语言服务器的编辑器那一半（docs/IDEA.md 第 2 步）：文档同步、实时报错（波浪线 + 悬停看原因）、跳转问 jdtls。
 * 跟着 Java 语言包懒加载（Editor.svelte 的 applyLang）：不打开 .java 就一个字节都不付。
 *
 * # 文档同步跟着编辑器走，关闭跟着标签走
 *
 * 编辑器被 `{#key}` 包着，切标签就销毁重建（frontend.md）—— 所以「打开」在编辑器建好时发（Rust 那边已经开着就当一次改动，
 * 切回来重发一遍无害），「关闭」不在编辑器销毁时发（那只是切走了），由 `watchClosed` 看标签表：Java 标签没了才 didClose。
 * 改动去抖 250ms 发全文（理由在 java.rs 头上「文档同步用全文」）；问跳转之前先把攒着的那次发出去 —— 不然 jdtls 拿旧文本答新位置。
 *
 * # 跳转：同 java-peek 的「先缓存、回头补下划线」
 *
 * jump.ts 的判定是同步的（⌘hover 鼠标每动一格问一次），而问 jdtls 是一次 IPC + 一次 LSP 往返（热的时候 10ms）。
 * 所以和 java-peek 一样：缓存里有当场答；没有就答「还不知道」、后台去问，回来调 `recheck` 让编辑器在鼠标还停着的地方重问。
 * **还不知道的时候不画下划线** —— 「有下划线 = 这一下我确定」是 jump.ts 唯一的承诺；问回来是空的（关键字、声明本身）就不画，
 * ⌘Click 照旧交回 CM6 加光标。jdtls 没起好（或者不是 Maven 项目）时这一层整个不说话，退回 Lezer 那两层。
 */
import { Decoration, EditorView, ViewPlugin, hoverTooltip, keymap, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { Prec, StateEffect, StateField, type Extension, type Text } from "@codemirror/state";
import { indentUnit } from "@codemirror/language";
import { autocompletion, completeAnyWord, snippet, type Completion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { mount, unmount } from "svelte";
import { listenHere, type JavaCompletionItem, type JavaDiagnostic, type JavaDiagnostics, type JavaEdit, type JavaLocation, type JavaResolved, type JavaStatus } from "../ipc/commands";
import { javaApplyAction, javaChange, javaClose, javaCodeActions, javaComplete, javaDefinition, javaFormat, javaOpen, javaPrepareRename, javaReferences, javaRename, javaResolve } from "../ipc/java";
import { boostOf, kindType, lspSnippet, offsetOf } from "./java-complete";
import { nav } from "../state/nav.svelte";
import UsagesPopup from "./UsagesPopup.svelte";
import RenamePopup from "./RenamePopup.svelte";
import QuickFixPopup from "./QuickFixPopup.svelte";
import { notify } from "../state/notify.svelte";
import { progress, type TaskHandle } from "../state/progress.svelte";
import { tabs } from "../state/tabs.svelte";
import type { JumpHit } from "./jump";

const DEBOUNCE = 250;

// ── 状态：每个项目根一份，进状态栏的进度格（ui.md 第十一条：后台在跑什么只有那一格） ──

const statusOf = new Map<string, JavaStatus>();
const tasks = new Map<string, TaskHandle>();
/** 同一句失败只说一次：每次打开一个 Java 文件都会再问一次 Rust，没有 JDK 21 的人不该每开一个文件就被提醒一遍 */
const told = new Set<string>();

/** 「32% Project 'x'」→ 32 */
const percentOf = (m: string) => {
  const g = /^(\d{1,3})%/.exec(m);
  return g ? Math.min(100, Number(g[1])) : null;
};

export function onStatus(s: JavaStatus) {
  statusOf.set(s.root, s);
  const busy = s.phase === "starting" || s.phase === "importing";
  const t = tasks.get(s.root);
  if (busy) {
    const label = s.phase === "starting" ? "Java：正在起语言服务器" : `Java：导入中${s.message ? ` · ${s.message.replace(/^\d{1,3}%\s*/, "")}` : ""}`;
    const percent = s.phase === "importing" ? percentOf(s.message) : null;
    if (t) t.set({ label, percent });
    else tasks.set(s.root, progress.start(label, { percent }));
    return;
  }
  t?.end();
  tasks.delete(s.root);
  if (s.phase === "failed" && !told.has(`${s.root}\n${s.message}`)) {
    told.add(`${s.root}\n${s.message}`);
    notify.fail(`Java 智能没起来：${s.message}`);
  }
}

/** 能问了没有：initialize 回过了（导入中也能问，jdtls 答得出已经导入的那部分） */
const usable = (root: string) => {
  const p = statusOf.get(root)?.phase;
  return p === "importing" || p === "ready";
};

// ── 诊断：按路径存一份，开着这个文件的编辑器各自画 ──

const diagsOf = new Map<string, JavaDiagnostic[]>();
const viewsOf = new Map<string, Set<EditorView>>();

function onDiags(d: JavaDiagnostics) {
  diagsOf.set(d.path, d.items);
  for (const v of viewsOf.get(d.path) ?? []) v.dispatch({ effects: setDiags.of(d.items) });
}

/** 登记过 didOpen 的：路径 → 项目根。标签没了就 didClose */
const opened = new Map<string, string>();

let wired = false;
function wire() {
  if (wired) return;
  wired = true;
  void listenHere<JavaStatus>("java-status", (e) => onStatus(e.payload));
  void listenHere<JavaDiagnostics>("java-diagnostics", (e) => onDiags(e.payload));
  // Java 标签关了才 didClose（编辑器销毁只是切走）。这是普通 .ts，用不了 `$effect`；看的只是「哪些路径还开着」，一秒一次足够
  setInterval(() => {
    if (!opened.size) return;
    const now = new Set(tabs.list.map((t) => t.path));
    for (const [p, root] of opened) {
      if (now.has(p)) continue;
      opened.delete(p);
      diagsOf.delete(p);
      void javaClose(root, p).catch(() => {});
    }
  }, 1000);
}

const setDiags = StateEffect.define<JavaDiagnostic[]>();

interface Mark {
  from: number;
  to: number;
  severity: number;
  message: string;
}

/** LSP 的行列（从 0 数、列按 UTF-16）→ 文档里的位置。越界夹回来：诊断可能是几次改动之前那份文本上算的 */
function offset(doc: Text, line: number, ch: number): number {
  if (line >= doc.lines) return doc.length;
  const l = doc.line(line + 1);
  return Math.min(l.from + ch, l.to);
}

function marksOf(doc: Text, items: JavaDiagnostic[]): Mark[] {
  return items
    .filter((d) => d.severity <= 3)
    .map((d) => {
      let from = offset(doc, d.line, d.character);
      let to = Math.max(from, offset(doc, d.endLine, d.endCharacter));
      // 零宽的（缺分号那种「这里少了东西」）画一个字符宽，不然看不见：往右一格，已经在行尾就往左一格
      if (to === from) {
        const line = doc.lineAt(from);
        if (from < line.to) to = from + 1;
        else if (from > line.from) from -= 1;
      }
      return { from, to, severity: d.severity, message: d.message };
    });
}

const markDeco = (sev: number) => Decoration.mark({ class: sev === 1 ? "cm-diag-err" : sev === 2 ? "cm-diag-warn" : "cm-diag-info" });

function decoOf(marks: Mark[]): DecorationSet {
  const sorted = [...marks].filter((m) => m.to > m.from).sort((a, b) => a.from - b.from || a.to - b.to);
  return Decoration.set(sorted.map((m) => markDeco(m.severity).range(m.from, m.to)));
}

/** 画出来的波浪线 + 每条的原因。改动时一起跟着挪（`map`），下一轮诊断回来整份换掉 */
const diagField = StateField.define<{ deco: DecorationSet; marks: Mark[] }>({
  create: () => ({ deco: Decoration.none, marks: [] }),
  update(v, tr) {
    for (const e of tr.effects) {
      if (e.is(setDiags)) {
        const marks = marksOf(tr.state.doc, e.value);
        return { deco: decoOf(marks), marks };
      }
    }
    if (!tr.docChanged) return v;
    return {
      deco: v.deco.map(tr.changes),
      marks: v.marks.map((m) => ({ ...m, from: tr.changes.mapPos(m.from, 1), to: tr.changes.mapPos(m.to, -1) })),
    };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});

/** 悬停在波浪线上看原因。文字走 textContent（tests/no-html-sinks.test.ts：不插 HTML） */
const diagHover = hoverTooltip((view, pos) => {
  const hits = view.state.field(diagField).marks.filter((m) => pos >= m.from && pos <= m.to);
  if (!hits.length) return null;
  return {
    pos: Math.min(...hits.map((h) => h.from)),
    end: Math.max(...hits.map((h) => h.to)),
    above: true,
    create() {
      const dom = document.createElement("div");
      dom.className = "cm-diag-tip";
      for (const h of hits.sort((a, b) => a.severity - b.severity)) {
        const row = document.createElement("div");
        row.className = h.severity === 1 ? "err" : h.severity === 2 ? "warn" : "info";
        row.textContent = h.message;
        dom.appendChild(row);
      }
      return { dom };
    },
  };
});

// ── 文档同步 ──

const syncs = new WeakMap<EditorView, Sync>();


class Sync {
  #timer: ReturnType<typeof setTimeout> | null = null;
  constructor(
    readonly view: EditorView,
    readonly root: string,
    readonly path: string,
  ) {
    syncs.set(view, this);
    const vs = viewsOf.get(path) ?? new Set();
    vs.add(view);
    viewsOf.set(path, vs);
    const have = diagsOf.get(path);
    // 先画已知的（切走再切回来不用等下一轮），在构造函数里 dispatch 会抛（update 进行中），推到下一拍
    if (have) queueMicrotask(() => syncs.get(view) === this && view.dispatch({ effects: setDiags.of(have) }));
    this.reopen();
  }

  /** 登记这份文档（重登记也一样：Rust 那边已经开着就当一次改动） */
  reopen() {
    const { root, path, view } = this;
    void javaOpen(root, path, view.state.doc.toString())
      .then((s) => {
        if (!s) return; // 不是 Maven / Gradle 项目：不起，也不说
        opened.set(path, root);
        onStatus(s);
      })
      .catch((e) => onStatus({ root, phase: "failed", message: String(e), detail: "" }));
  }

  update(u: ViewUpdate) {
    if (!u.docChanged) return;
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.flush(), DEBOUNCE);
  }

  /** 攒着的改动现在就发。问 jdtls 之前、编辑器销毁之前都要先发 */
  flush() {
    if (!this.#timer) return;
    clearTimeout(this.#timer);
    this.#timer = null;
    void javaChange(this.root, this.path, this.view.state.doc.toString()).catch(() => {});
  }

  destroy() {
    this.flush();
    viewsOf.get(this.path)?.delete(this.view);
    syncs.delete(this.view);
  }
}

// ── 跳转 ──

/** 每份文档的改动代数：缓存的答案只对它被问时的那份文本作数 */
const genField = StateField.define<number>({ create: () => 0, update: (g, tr) => (tr.docChanged ? g + 1 : g) });

interface Asked {
  gen: number;
  from: number;
  to: number;
  /** undefined = 还在问 */
  hit: JumpHit | null | undefined;
  wait: Promise<void>;
}
const asked = new WeakMap<EditorView, Asked>();

/** 光标底下的标识符（Java 的：字母数字、下划线、$）。没有就是 null —— 空白、符号上不问 */
function wordAt(doc: Text, pos: number): { from: number; to: number } | null {
  const line = doc.lineAt(pos);
  const t = line.text;
  const id = /[\p{L}\p{N}_$]/u;
  let a = pos - line.from;
  let b = a;
  while (a > 0 && id.test(t[a - 1])) a--;
  while (b < t.length && id.test(t[b])) b++;
  if (a === b || /\p{N}/u.test(t[a])) return null;
  return { from: line.from + a, to: line.from + b };
}

function toHit(loc: JavaLocation, root: string, self: string, word: { from: number; to: number }, doc: Text): JumpHit | null {
  const text = doc.sliceString(word.from, word.to);
  if (loc.path === null) {
    return { from: word.from, to: word.to, text, target: { rel: "", uri: loc.uri, label: loc.label, line: loc.line + 1, col: loc.character + 1, why: "语言服务器" } };
  }
  if (loc.path === self) {
    const at = offset(doc, loc.line, loc.character);
    // 声明本身：从它跳到它没有意义，画一条点了原地不动的下划线比不画更糟（jump.ts REF_NODES 那条）
    if (at >= word.from && at <= word.to) return null;
    return { from: word.from, to: word.to, text, target: { rel: "", line: loc.line + 1, col: loc.character + 1, why: "语言服务器" } };
  }
  // 项目外的文件（另一个项目里的源码）这一版不跳：nav 只认相对项目根的路径
  if (!loc.path.startsWith(`${root}/`)) return null;
  return { from: word.from, to: word.to, text, target: { rel: loc.path.slice(root.length + 1), line: loc.line + 1, col: loc.character + 1, why: "语言服务器" } };
}

/**
 * 这个位置能不能跳（jdtls 说的）。`undefined` = 还在问（回来调 `recheck`）；`null` = 问过了，不能跳 / 这一层不管。
 * 同一个词、同一份文本只问一次
 */
export function lspResolve(view: EditorView, pos: number, recheck: () => void): JumpHit | null | undefined {
  const s = syncs.get(view);
  if (!s || !usable(s.root)) return null;
  const w = wordAt(view.state.doc, pos);
  if (!w) return null;
  const gen = view.state.field(genField, false) ?? 0;
  const a = asked.get(view);
  if (a && a.gen === gen && a.from === w.from) return a.hit;
  s.flush();
  const line = view.state.doc.lineAt(w.from);
  const entry: Asked = { gen, from: w.from, to: w.to, hit: undefined, wait: Promise.resolve() };
  entry.wait = javaDefinition(s.root, s.path, line.number - 1, w.from - line.from)
    .then((locs) => {
      entry.hit = locs.length && view.state.field(genField, false) === gen ? toHit(locs[0], s.root, s.path, w, view.state.doc) : null;
    })
    .catch(() => {
      entry.hit = null;
    })
    .then(() => {
      if (asked.get(view) === entry) recheck();
    });
  asked.set(view, entry);
  return undefined;
}

/** ⌘B 用：手上在问的那次回来了再说。没有在问的给 null */
export function lspSettle(view: EditorView): Promise<void> | null {
  const a = asked.get(view);
  return a && a.hit === undefined ? a.wait : null;
}

// ── 查找用法（⌥F7 / ⌥⌘F7） ──

/** 一处用法 → 跳转落点。同一个文件里的只跳行；项目外的（另一个项目的源码）这一版不跳 */
function targetOf(loc: JavaLocation, root: string, self: string): JumpHit["target"] | null {
  const at = { line: loc.line + 1, col: loc.character + 1, why: "语言服务器" as const };
  if (loc.path === self) return { rel: "", ...at };
  if (loc.path?.startsWith(`${root}/`)) return { rel: loc.path.slice(root.length + 1), ...at };
  return null;
}

let popup: ReturnType<typeof mount> | null = null;
function closePopup() {
  if (popup) void unmount(popup);
  popup = null;
}

async function findUsages(view: EditorView): Promise<void> {
  const s = syncs.get(view);
  if (!s) return;
  if (!usable(s.root)) {
    notify.hint("Java 语言服务器还没起好。先用「在项目里找这个名字」");
    return;
  }
  const w = wordAt(view.state.doc, view.state.selection.main.head);
  if (!w) {
    notify.hint("光标不在名字上");
    return;
  }
  s.flush();
  const name = view.state.doc.sliceString(w.from, w.to);
  const line = view.state.doc.lineAt(w.from);
  // 第一次查用法要搜全部索引，第 0 步实测最慢 6 秒：进度格得说在干什么（150ms 之内回来的它自己不出现，ui.md 第八条）
  const t = progress.start(`正在查 ${name} 的用法`);
  let r: Awaited<ReturnType<typeof javaReferences>>;
  try {
    r = await javaReferences(s.root, s.path, line.number - 1, w.from - line.from);
  } catch (e) {
    notify.fail(`查不了 ${name} 的用法：${e}`);
    return;
  } finally {
    t.end();
  }
  if (!view.dom.isConnected) return;
  const c = view.coordsAtPos(w.from);
  closePopup();
  popup = mount(UsagesPopup, {
    target: document.body,
    props: {
      name,
      items: r.items,
      total: r.total,
      x: c?.left ?? 200,
      y: c?.bottom ?? 200,
      onclose: () => {
        closePopup();
        view.focus();
      },
      onpick: (loc: JavaLocation) => {
        closePopup();
        const target = targetOf(loc, s.root, s.path);
        if (!target) {
          notify.hint(`${loc.label} 不在这个项目里，这一版还跳不过去`);
          return;
        }
        void nav.jumpTo({ from: w.from, to: w.to, text: name, target });
      },
    },
  });
}

// ── 格式化（⌥⌘L，docs/IDEA.md 第 15 节） ──

/**
 * Java 文件里 ⌥⌘L 交给 jdtls（IDEA 的 Reformat Code）：有选区就只排选中的那几行，没有就整份。缩进照编辑器此刻的设置（状态栏能改）给它 ——
 * 不给的话 jdtls 用 Eclipse 的默认 Tab。回来的是几十处很小的改动，**一笔事务**放进去：⌘Z 一下全撤、光标跟着改动挪。
 * 问的时候带着全文；回来时文件又被改过（人在等的时候接着敲了）就不放，说一句 —— 那些位置是在旧文本上算的
 */
async function formatJava(view: EditorView): Promise<void> {
  const s = syncs.get(view);
  if (!s) return;
  if (!usable(s.root)) {
    notify.hint("Java 语言服务器还没起好，格式化要等它");
    return;
  }
  const st = view.state;
  const unit = st.facet(indentUnit);
  const tabs = unit.includes("\t");
  const sel = st.selection.main;
  let range = null;
  if (!sel.empty) {
    const a = st.doc.lineAt(sel.from);
    const b = st.doc.lineAt(sel.to);
    range = { line: a.number - 1, character: 0, endLine: b.number - 1, endCharacter: b.length };
  }
  const gen = st.field(genField, false) ?? 0;
  let edits: Awaited<ReturnType<typeof javaFormat>>;
  try {
    edits = await javaFormat(s.root, s.path, st.doc.toString(), range, tabs ? st.tabSize : unit.length, !tabs);
  } catch (e) {
    notify.fail(`没格式化：${e instanceof Error ? e.message : String(e)}`);
    return;
  }
  if (!view.dom.isConnected) return;
  if ((view.state.field(genField, false) ?? 0) !== gen) {
    notify.hint("格式化回来时文件又改过了，没动它 —— 再按一次 ⌥⌘L");
    return;
  }
  const doc = view.state.doc;
  const changes = edits
    .map((e) => ({ from: offset(doc, e.line, e.character), to: offset(doc, e.endLine, e.endCharacter), insert: e.text }))
    .filter((c) => c.insert !== doc.sliceString(c.from, c.to))
    .sort((a, b) => a.from - b.from || a.to - b.to);
  if (!changes.length) {
    notify.hint(range ? "选中的这几行已经是这个格式了" : "已经是这个格式了");
    return;
  }
  view.dispatch({ changes, userEvent: "input.format" });
  notify.ok(`已格式化${range ? "选中的几行" : ""}：${changes.length} 处`);
}

// ── 快速修复（⌥↵，docs/IDEA.md 第 16 节） ──

/**
 * 问 jdtls 光标那儿（或选中的那几行）能做什么修复，挂一张列表；选中一条再要它的改动（resolve）。
 * 只改这个文件的放进编辑器、一笔事务（⌘Z 能撤，不写盘）；改到别的文件的照改名那套落盘、带撤销卡片。
 * 问的时候带着全文；改动回来时文件又改过了就不放 —— 位置是在旧文本上算的
 */
async function quickFix(view: EditorView): Promise<void> {
  const s = syncs.get(view);
  if (!s) return;
  if (!usable(s.root)) {
    notify.hint("Java 语言服务器还没起好，快速修复要等它");
    return;
  }
  const st = view.state;
  const sel = st.selection.main;
  const a = st.doc.lineAt(sel.from);
  const b = st.doc.lineAt(sel.to);
  const range = { line: a.number - 1, character: sel.from - a.from, endLine: b.number - 1, endCharacter: sel.to - b.from };
  let items: Awaited<ReturnType<typeof javaCodeActions>>;
  try {
    items = await javaCodeActions(s.root, s.path, st.doc.toString(), range);
  } catch (e) {
    notify.fail(`问不到修复：${e instanceof Error ? e.message : String(e)}`);
    return;
  }
  if (!view.dom.isConnected) return;
  if (!items.length) {
    notify.hint("这里没有可用的修复");
    return;
  }
  const gen = view.state.field(genField, false) ?? 0;
  const c = view.coordsAtPos(sel.head);
  closePopup();
  popup = mount(QuickFixPopup, {
    target: document.body,
    props: {
      items,
      x: c?.left ?? 200,
      y: c?.bottom ?? 200,
      onclose: () => {
        closePopup();
        view.focus();
      },
      onpick: (it: (typeof items)[number]) => {
        closePopup();
        view.focus();
        void applyAction(view, s, it.title, it.raw, gen);
      },
    },
  });
}

async function applyAction(view: EditorView, s: Sync, title: string, raw: string, gen: number) {
  const t = progress.start(`正在做：${title}`);
  try {
    const ops = await import("../state/replace-ops");
    const r = await javaApplyAction(s.root, s.path, raw, ops.openDocs());
    if (r.outcome) {
      await ops.landInTabs(r.outcome);
      const { replace } = await import("../state/replace.svelte");
      const hits = r.outcome.changed.reduce((n, c) => n + c.count, 0);
      const label = `${title}：改了 ${r.outcome.changed.length} 个文件 ${hits} 处`;
      replace.done = r.outcome.undo ? { root: s.root, files: r.outcome.changed.length, hits, label } : null;
      if (!r.outcome.undo) notify.ok(label);
      return;
    }
    if (!view.dom.isConnected) return;
    if ((view.state.field(genField, false) ?? 0) !== gen) {
      notify.hint("修复回来时文件又改过了，没动它 —— 再按一次 ⌥↵");
      return;
    }
    const doc = view.state.doc;
    const changes = r.edits
      .map((e) => ({ from: offset(doc, e.line, e.character), to: offset(doc, e.endLine, e.endCharacter), insert: e.text }))
      .filter((c) => c.insert !== doc.sliceString(c.from, c.to))
      .sort((x, y) => x.from - y.from || x.to - y.to);
    if (!changes.length) {
      notify.hint(`「${title}」什么都没改`);
      return;
    }
    view.dispatch({ changes, userEvent: "input.fix", scrollIntoView: true });
    notify.ok(title);
  } catch (e) {
    notify.fail(`没做成「${title}」：${e instanceof Error ? e.message : String(e)}`, 8000);
  } finally {
    t.end();
  }
}

// ── 改名（⇧F6，docs/IDEA.md 第 14 节） ──

/**
 * 先问 jdtls 光标底下能不能改（关键字、注释、jar 里的类不能），能改才弹框；↵ 之后框立刻关、进度去状态栏。
 * 落盘全在 Rust（`java::apply_rename`，照跨文件替换那套），回来的结果和替换同一个形状 —— 开着的标签走替换那条路跟上（`landInTabs`，
 * 改了名的文件标签挪到新路径），撤销用替换的那张卡片
 */
async function renameAt(view: EditorView): Promise<void> {
  const s = syncs.get(view);
  if (!s) return;
  if (!usable(s.root)) {
    notify.hint("Java 语言服务器还没起好，改名要等它");
    return;
  }
  const head = view.state.selection.main.head;
  const line = view.state.doc.lineAt(head);
  const at = { line: line.number - 1, character: head - line.from };
  let target: Awaited<ReturnType<typeof javaPrepareRename>>;
  try {
    target = await javaPrepareRename(s.root, s.path, view.state.doc.toString(), at.line, at.character);
  } catch (e) {
    notify.fail(`改不了名：${e}`);
    return;
  }
  if (!target) {
    notify.hint("光标底下不是能改名的东西（类、方法、字段、变量才行）");
    return;
  }
  if (!view.dom.isConnected) return;
  const from = offset(view.state.doc, target.line, target.character);
  const name = target.placeholder ?? view.state.doc.sliceString(from, offset(view.state.doc, target.endLine, target.endCharacter));
  const c = view.coordsAtPos(from);
  closePopup();
  popup = mount(RenamePopup, {
    target: document.body,
    props: {
      name,
      x: c?.left ?? 200,
      y: c?.bottom ?? 200,
      onclose: () => {
        closePopup();
        view.focus();
      },
      onrename: (to: string) => {
        closePopup();
        view.focus();
        void doRename(view, s, at, name, to);
      },
    },
  });
}

async function doRename(view: EditorView, s: Sync, at: { line: number; character: number }, name: string, to: string) {
  // 第一次改名要搜全部索引（探针实测 4.3 秒）：进度格得说在干什么（150ms 之内回来的它自己不出现，ui.md 第八条）
  const t = progress.start(`正在把 ${name} 改名为 ${to}`);
  try {
    const ops = await import("../state/replace-ops");
    const o = await javaRename(s.root, s.path, view.state.doc.toString(), at.line, at.character, to, ops.openDocs());
    await ops.landInTabs(o);
    const { replace } = await import("../state/replace.svelte");
    const hits = o.changed.reduce((n, c) => n + c.count, 0);
    const files = o.renamed.length ? `，文件 ${o.renamed.map((r) => `${base(r.from)} → ${base(r.to)}`).join("、")}` : "";
    const label = `已把 ${name} 改名为 ${to}：${o.changed.length} 个文件 ${hits} 处${files}`;
    replace.done = o.undo ? { root: s.root, files: o.changed.length, hits, label } : null;
    if (!o.undo) notify.ok(label);
  } catch (e) {
    notify.fail(`没改名：${e instanceof Error ? e.message : String(e)}`, 8000);
  } finally {
    t.end();
  }
}

const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

// ── 补全（docs/IDEA.md 第 3 步、第 13 节） ──

/**
 * resolve 的结果按这一项的原样记住（同一次补全里上下移动选中项会反复问到同一项）。只留最近这一批：换一次补全列表就清
 */
let resolving = new Map<string, Promise<JavaResolved | null>>();

function resolveOf(root: string, raw: string): Promise<JavaResolved | null> {
  let p = resolving.get(raw);
  if (!p) {
    p = javaResolve(root, raw).catch(() => null);
    resolving.set(raw, p);
  }
  return p;
}

/** 已经回来了的 resolve（同步取）：回车时手上有就和正文一起做，没有就先插正文 */
const settled = new Map<string, JavaResolved | null>();

/**
 * 自动 import（和别的 additionalTextEdits）→ CM6 的改动。**只收补全那一行上面的**：它们是在补全前那份文本上算的行列，
 * 回来之前人可能已经接着敲了 —— 敲在光标处，上面的行没动，行列照样对；落在补全那一行或者下面的就不敢做了（没见 jdtls 给过）
 */
function importChanges(doc: Text, edits: JavaEdit[], line: number) {
  return edits
    .filter((e) => e.endLine < line)
    .map((e) => ({ from: offsetOf(doc, e.line, e.character), to: offsetOf(doc, e.endLine, e.endCharacter), insert: e.text }));
}

type JavaOption = Completion & { description: string };

function toCompletion(root: string, it: JavaCompletionItem, index: number, total: number): JavaOption {
  const text = it.edit?.text ?? it.label;
  return {
    label: it.label,
    detail: it.detail || undefined,
    type: kindType(it.kind),
    boost: boostOf(index, total),
    // 右边那一格（返回类型 / 包名）在 addToOptions 里画
    description: it.description,
    // 说明文字：选中时 CM6 来要，顺手把 resolve 先问了 —— 回车时自动 import 多半已经在手上（resolve 实测 140–250 ms）
    info: async () => {
      const r = await resolveOf(root, it.raw);
      settled.set(it.raw, r);
      if (!r?.doc) return null;
      const dom = document.createElement("div");
      dom.className = "cm-java-doc";
      dom.textContent = r.doc; // 不插 HTML（tests/no-html-sinks.test.ts）；markdown 原文也读得懂
      return dom;
    },
    apply: (view, c, from, to) => {
      const line = view.state.doc.lineAt(from).number - 1;
      const ready = settled.get(it.raw);
      const pending = ready === undefined ? resolveOf(root, it.raw) : null;
      // 手上已有 import：先加 import（在上面），再把补全的范围挪一下、插正文 —— 撤销时两样分两步，但都在这一下里发生
      if (ready?.edits.length) {
        const changes = importChanges(view.state.doc, ready.edits, line);
        if (changes.length) {
          const tr = view.state.update({ changes, userEvent: "input.complete" });
          view.dispatch(tr);
          from = tr.changes.mapPos(from, -1);
          to = tr.changes.mapPos(to, 1);
        }
      }
      if (it.snippet) snippet(lspSnippet(text))(view, c, from, to);
      else view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length }, userEvent: "input.complete", scrollIntoView: true });
      // 还没回来：先插了正文，import 回来再补到上面（第 13 节「两段」）
      void pending?.then((r) => {
        if (!r?.edits.length || !view.dom.isConnected) return;
        const changes = importChanges(view.state.doc, r.edits, view.state.doc.lineAt(view.state.selection.main.head).number - 1);
        if (changes.length) view.dispatch({ changes, userEvent: "input.complete" });
      });
    },
  };
}

/**
 * 补全源。jdtls 没起好、或者不是 Maven / Gradle 项目：退回「文档里出现过的词」（原来的行为），不让补全框在 Java 文件里整个没了。
 * **只在第一个字、`.` 和 `@` 后面问**：之后接着敲在已有的里面筛（`validFor`）—— 除非 jdtls 说它截了（incomplete），那就每个字都问
 */
async function javaSource(ctx: CompletionContext): Promise<CompletionResult | null> {
  const view = ctx.view;
  const s = view && syncs.get(view);
  if (!s || !usable(s.root)) return completeAnyWord(ctx);
  const word = ctx.matchBefore(/[\p{L}\p{N}_$]*/u);
  const from = word?.from ?? ctx.pos;
  const before = ctx.state.sliceDoc(from - 1, from);
  if (!ctx.explicit && from === ctx.pos && before !== "." && before !== "@") return null;
  const doc = ctx.state.doc;
  const line = doc.lineAt(ctx.pos);
  let r: Awaited<ReturnType<typeof javaComplete>>;
  try {
    r = await javaComplete(s.root, s.path, doc.toString(), line.number - 1, ctx.pos - line.from);
  } catch {
    return null;
  }
  if (ctx.aborted || !r.items.length) return null;
  resolving = new Map();
  settled.clear();
  // 替换从哪儿起：jdtls 给的范围的起点（一般就是光标前那个词的开头）；各项不一样的取最靠前的
  const starts = r.items.filter((i) => i.edit).map((i) => offsetOf(doc, i.edit!.line, i.edit!.character));
  const start = starts.length ? Math.min(from, ...starts) : from;
  return {
    from: start,
    options: r.items.map((it, i) => toCompletion(s.root, it, i, r.items.length)),
    validFor: r.incomplete ? undefined : /^[\p{L}\p{N}_$]*$/u,
  };
}

/** 补全框每行右边那一格：返回类型 / 包名（IDEA 的补全也是这么摆的） */
const descriptionColumn = {
  position: 90,
  render(c: Completion) {
    const d = (c as Partial<JavaOption>).description;
    if (!d) return null;
    const el = document.createElement("span");
    el.className = "cm-java-desc";
    el.textContent = d;
    return el;
  },
};

/** 装进 Java 编辑器的那一份。`root` 为空（没项目）或者文件不在项目里：只是空数组 */
export function javaLsp(root: string | null, path: string): Extension {
  if (!root || !path.startsWith(`${root}/`)) return [];
  wire();
  return [
    genField,
    diagField,
    diagHover,
    // 换掉通用的「文档里的词」：Java 文件里由 javaSource 说了算（它自己会在 jdtls 没起好时退回去）。和 Editor 里那份 autocompletion() 的配置合并
    autocompletion({ override: [javaSource], addToOptions: [descriptionColumn] }),
    ViewPlugin.define((view) => new Sync(view, root, path)),
    // IDEA 的键位：⌥F7 查找用法、⌥⌘F7 Show Usages。两个都开同一张列表（UsagesPopup.svelte 头上说了为什么）
    Prec.high(
      keymap.of([
        { key: "Alt-F7", preventDefault: true, run: (v) => (void findUsages(v), true) },
        { key: "Mod-Alt-F7", preventDefault: true, run: (v) => (void findUsages(v), true) },
        // IDEA 的 Refactor → Rename（keymap.ts 登记成 cm6）
        { key: "Shift-F6", preventDefault: true, run: (v) => (void renameAt(v), true) },
        // IDEA 的 Show Context Actions（keymap.ts 登记成 cm6）
        { key: "Alt-Enter", preventDefault: true, run: (v) => (void quickFix(v), true) },
        // IDEA 的 Reformat Code。Prec.high 排在 Editor 里那条「格式化 JSON」前面：Java 文件里先到这儿（keymap.ts 里是同一行，说明写了两种）
        { key: "Alt-Mod-l", preventDefault: true, run: (v) => (void formatJava(v), true) },
      ]),
    ),
    EditorView.theme({
      // 波浪线：只加线不换字色，同 ⌘hover 的下划线那条理由（jump-ext.ts）—— 语法着色说的信息不能被盖掉
      ".cm-diag-err": { textDecoration: "underline wavy var(--lvl-error) 1px", textUnderlineOffset: "3px" },
      ".cm-diag-warn": { textDecoration: "underline wavy var(--lvl-warn) 1px", textUnderlineOffset: "3px" },
      ".cm-diag-info": { textDecoration: "underline dotted var(--lvl-info) 1px", textUnderlineOffset: "3px" },
      ".cm-diag-tip": { padding: "6px 10px", maxWidth: "560px", fontFamily: "var(--ui-font)", fontSize: "var(--fs-sm)", lineHeight: "1.45" },
      ".cm-diag-tip > div + div": { marginTop: "4px" },
      ".cm-java-desc": { marginLeft: "auto", paddingLeft: "16px", color: "var(--text-faint)", fontSize: "var(--fs-xs)" },
      // 选中行是强调色底（theme-idea-dark.ts）：灰字在蓝底上读不清，同 .cm-completionDetail 换成 80% 白
      ".cm-tooltip-autocomplete > ul > li[aria-selected] .cm-java-desc": { color: "rgba(255, 255, 255, 0.8)" },
      ".cm-java-doc": { padding: "6px 10px", maxWidth: "480px", maxHeight: "260px", overflow: "auto", whiteSpace: "pre-wrap", fontFamily: "var(--ui-font)", fontSize: "var(--fs-sm)", lineHeight: "1.45", color: "var(--text-muted)" },
      ".cm-diag-tip > .err::before": { content: '"✕ "', color: "var(--lvl-error)" },
      ".cm-diag-tip > .warn::before": { content: '"! "', color: "var(--lvl-warn)" },
    }),
  ];
}
