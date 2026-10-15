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
import { mount, unmount } from "svelte";
import { listenHere, type JavaDiagnostic, type JavaDiagnostics, type JavaInstall, type JavaLocation, type JavaStatus } from "../ipc/commands";
import { javaChange, javaClose, javaDefinition, javaInstall, javaInstallCancel, javaOpen, javaReferences } from "../ipc/java";
import { declined, javaOffer } from "../state/java-offer.svelte";
import { nav } from "../state/nav.svelte";
import UsagesPopup from "./UsagesPopup.svelte";
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
  // 还没装 jdtls：不报错，问一句要不要下（卡片在 Confirms 里）。点过「以后再说」的这次运行里不再问
  if (s.phase === "missing") {
    if (!declined.has(s.root) && !installing) javaOffer.card = { root: s.root, text: s.message };
    return;
  }
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

// ── 下 jdtls（卡片上点了「下载」） ──

let installing = false;

/** 下 jdtls、装好、起这个项目的 jdtls，再把开着的 Java 文档重新登记一遍（它们手上的才是没保存的那份） */
export async function installJdtls(root: string) {
  if (installing) return;
  installing = true;
  javaOffer.card = null;
  const t = progress.start("正在下载 jdtls", { percent: 0, cancel: () => void javaInstallCancel() });
  const un = await listenHere<JavaInstall>("java-install", (e) => {
    const { received, total } = e.payload;
    t.set({ label: `正在下载 jdtls · ${(received / 1048576).toFixed(0)} / ${(total / 1048576).toFixed(0)} MB`, percent: total ? Math.floor((received / total) * 100) : null });
  });
  try {
    await javaInstall(root);
    notify.ok("jdtls 装好了，Java 智能起来了");
    for (const s of live) if (s.root === root) s.reopen();
  } catch (e) {
    const why = String(e);
    if (why.includes("取消了")) notify.hint("没下 jdtls（取消了）。再打开 Java 文件时会再问");
    else notify.fail(`jdtls 没装上：${why}`);
  } finally {
    un();
    t.end();
    installing = false;
  }
}

// ── 文档同步 ──

const syncs = new WeakMap<EditorView, Sync>();
/** 活着的编辑器：装好 jdtls 之后要逐个重新登记（WeakMap 遍历不了） */
const live = new Set<Sync>();

class Sync {
  #timer: ReturnType<typeof setTimeout> | null = null;
  constructor(
    readonly view: EditorView,
    readonly root: string,
    readonly path: string,
  ) {
    syncs.set(view, this);
    live.add(this);
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
    live.delete(this);
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

/** 装进 Java 编辑器的那一份。`root` 为空（没项目）或者文件不在项目里：只是空数组 */
export function javaLsp(root: string | null, path: string): Extension {
  if (!root || !path.startsWith(`${root}/`)) return [];
  wire();
  return [
    genField,
    diagField,
    diagHover,
    ViewPlugin.define((view) => new Sync(view, root, path)),
    // IDEA 的键位：⌥F7 查找用法、⌥⌘F7 Show Usages。两个都开同一张列表（UsagesPopup.svelte 头上说了为什么）
    Prec.high(
      keymap.of([
        { key: "Alt-F7", preventDefault: true, run: (v) => (void findUsages(v), true) },
        { key: "Mod-Alt-F7", preventDefault: true, run: (v) => (void findUsages(v), true) },
      ]),
    ),
    EditorView.theme({
      // 波浪线：只加线不换字色，同 ⌘hover 的下划线那条理由（jump-ext.ts）—— 语法着色说的信息不能被盖掉
      ".cm-diag-err": { textDecoration: "underline wavy var(--lvl-error) 1px", textUnderlineOffset: "3px" },
      ".cm-diag-warn": { textDecoration: "underline wavy var(--lvl-warn) 1px", textUnderlineOffset: "3px" },
      ".cm-diag-info": { textDecoration: "underline dotted var(--lvl-info) 1px", textUnderlineOffset: "3px" },
      ".cm-diag-tip": { padding: "6px 10px", maxWidth: "560px", fontFamily: "var(--ui-font)", fontSize: "var(--fs-sm)", lineHeight: "1.45" },
      ".cm-diag-tip > div + div": { marginTop: "4px" },
      ".cm-diag-tip > .err::before": { content: '"✕ "', color: "var(--lvl-error)" },
      ".cm-diag-tip > .warn::before": { content: '"! "', color: "var(--lvl-warn)" },
    }),
  ];
}
