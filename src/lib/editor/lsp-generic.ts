/**
 * Java 以外的语言服务器的编辑器那一半（docs/LANGPACK.md）：实时报错、补全、F1 文档、⌘B / ⌘Click 跳定义、⌥F7 查用法、
 * ⇧F6 改名、⌥⌘L 格式化、⌥↵ 快速修复。
 * 哪些文件有语言服务器由「语言包」说（内置的 + 你自己的，Rust 侧 `langs.rs`）；跟着语言包懒加载，不打开那种文件一个字节不付。
 *
 * 形状照 `java-lsp.ts`（那边的注释是来龙去脉）：编辑器建好时 didOpen、标签没了才 didClose（编辑器 `{#key}` 重建只是切走）；
 * 改动去抖 250 ms 发全文、问之前先把攒着的发出去；跳转「先缓存、回头补下划线」—— 还不知道就不画。
 * 画法和弹层共用 Java 那边的（波浪线的样式、查用法的列表、文档浮层）。语言服务器没装：只说一次，带怎么装
 */
import { Decoration, EditorView, ViewPlugin, hoverTooltip, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { Prec, StateEffect, StateField, type Extension, type Text } from "@codemirror/state";
import { indentUnit } from "@codemirror/language";
import { autocompletion, completeAnyWord, snippet, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { mount, unmount } from "svelte";
import { listenHere, type JavaDiagnostic, type JavaDiagnostics, type JavaLocation, type LspPack } from "../ipc/commands";
import { lspApplyAction, lspClose, lspCodeActions, lspComplete, lspDefinition, lspFormat, lspHover, lspInstall, lspOpen, lspPacks, lspPrepareRename, lspReferences, lspRename } from "../ipc/lsp";
import { boostOf, kindType, lspSnippet, offsetOf } from "./java-complete";
import { idKeymap } from "./id-keys";
import { nav } from "../state/nav.svelte";
import { notify } from "../state/notify.svelte";
import { problems } from "../state/problems.svelte";
import { progress } from "../state/progress.svelte";
import { tabs } from "../state/tabs.svelte";
import UsagesPopup from "./UsagesPopup.svelte";
import RenamePopup from "./RenamePopup.svelte";
import QuickFixPopup from "./QuickFixPopup.svelte";
import type { JumpHit } from "./jump";

const DEBOUNCE = 250;

// ── 哪些文件有语言服务器：包的列表问一次 ──

let packList: Promise<LspPack[]> | null = null;
const nameMatches = (pat: string, name: string) => (pat.startsWith("*") ? name.length > pat.length - 1 && name.endsWith(pat.slice(1)) : pat === name);

/** 这个文件归哪个语言包（同 Rust 侧 `pack_for` 的认法）；没有是 null */
export async function packFor(path: string): Promise<LspPack | null> {
  packList ??= lspPacks().then(
    (r) => {
      // 认不得的包文件说一次（你写的包少个字段，不能悄悄没效果）
      for (const p of r.problems) notify.hint(`语言包无效：${p}`);
      return r.packs;
    },
    () => [],
  );
  const name = path.slice(path.lastIndexOf("/") + 1);
  return (await packList).find((p) => p.files.some((pat) => nameMatches(pat, name))) ?? null;
}

// ── 诊断：按路径存一份，开着这个文件的编辑器各自画 ──

const diagsOf = new Map<string, JavaDiagnostic[]>();
const viewsOf = new Map<string, Set<EditorView>>();
/** didOpen 过的：路径 → 项目根。标签没了就 didClose */
const opened = new Map<string, string>();
/** 起不来（没装）的那句：每个包只说一次 */
const told = new Set<string>();

let wired = false;
function wire() {
  if (wired) return;
  wired = true;
  void listenHere<JavaDiagnostics>("lsp-diagnostics", (e) => {
    const d = e.payload;
    diagsOf.set(d.path, d.items);
    problems.set(d);
    for (const v of viewsOf.get(d.path) ?? []) v.dispatch({ effects: setDiags.of(d.items) });
  });
  // 同 Java：标签关了才 didClose。看的只是「哪些路径还开着」，一秒一次足够
  setInterval(() => {
    if (!opened.size) return;
    const now = new Set(tabs.list.map((t) => t.path));
    for (const [p, root] of opened) {
      if (now.has(p)) continue;
      opened.delete(p);
      diagsOf.delete(p);
      problems.drop(p);
      void lspClose(root, p).catch(() => {});
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
      if (to === from) {
        const line = doc.lineAt(from);
        if (from < line.to) to = from + 1;
        else if (from > line.from) from -= 1;
      }
      return { from, to, severity: d.severity, message: d.message };
    });
}

const markDeco = (sev: number) => Decoration.mark({ class: sev === 1 ? "cm-diag-err" : sev === 2 ? "cm-diag-warn" : "cm-diag-info" });

const diagField = StateField.define<{ deco: DecorationSet; marks: Mark[] }>({
  create: () => ({ deco: Decoration.none, marks: [] }),
  update(v, tr) {
    for (const e of tr.effects) {
      if (e.is(setDiags)) {
        const marks = marksOf(tr.state.doc, e.value);
        const sorted = marks.filter((m) => m.to > m.from).sort((a, b) => a.from - b.from || a.to - b.to);
        return { deco: Decoration.set(sorted.map((m) => markDeco(m.severity).range(m.from, m.to))), marks };
      }
    }
    if (!tr.docChanged) return v;
    return { deco: v.deco.map(tr.changes), marks: v.marks.map((m) => ({ ...m, from: tr.changes.mapPos(m.from, 1), to: tr.changes.mapPos(m.to, -1) })) };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});

/** 悬停在波浪线上看原因。文字走 textContent（不插 HTML） */
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
/** 开着的（装好语言服务器之后要它们重连） */
const live = new Set<Sync>();
let installing = false;

/**
 * 一键装语言服务器（npm 照编进应用的锁文件装进应用数据目录，Rust 那边 `langs::install`）。装好了，开着的这种文件当场接上，不用重开
 */
async function install(pack: LspPack) {
  if (installing) return;
  installing = true;
  const t = progress.start(`正在安装 ${pack.name} 语言服务器（${pack.installSize}，来自 npm 官方源）`);
  try {
    const r = await lspInstall(pack.id);
    told.delete(pack.id);
    notify.ok(`已安装 ${pack.name} 语言服务器（${(r.bytes / (1 << 20)).toFixed(0)} MB）`);
    for (const s of live) if (s.pack.id === pack.id) s.connect();
  } catch (e) {
    notify.fail(`安装 ${pack.name} 语言服务器失败：${e instanceof Error ? e.message : String(e)}`, 10_000);
  } finally {
    installing = false;
    t.end();
  }
}

class Sync {
  timer: ReturnType<typeof setTimeout> | null = null;
  /** 语言服务器起来了没有（没装、起不来的时候这一层整个不说话，退回原来的行为） */
  ready = false;
  view: EditorView;
  root: string;
  path: string;
  pack: LspPack;
  constructor(view: EditorView, root: string, path: string, pack: LspPack) {
    this.view = view;
    this.root = root;
    this.path = path;
    this.pack = pack;
    syncs.set(view, this);
    const vs = viewsOf.get(path) ?? new Set();
    vs.add(view);
    viewsOf.set(path, vs);
    const have = diagsOf.get(path);
    if (have) queueMicrotask(() => syncs.get(view) === this && view.dispatch({ effects: setDiags.of(have) }));
    live.add(this);
    this.connect();
  }

  /** 登记这份文档（服务器没起的起）。没装：只说一次；能一键装的，那句话上带「安装」 */
  connect() {
    const { root, path, pack, view } = this;
    // 第一次要起服务器（rust-analyzer 0.4 秒、索引更久）：进度格说一句在干什么（150 ms 之内回来的它自己不出现）
    const task = progress.start(`正在启动 ${pack.name} 语言服务器`);
    void lspOpen(root, path, view.state.doc.toString())
      .then((name) => {
        if (!name) return;
        this.ready = true;
        opened.set(path, root);
      })
      .catch((e) => {
        if (told.has(pack.id)) return;
        told.add(pack.id);
        if (pack.installSize) notify.hint(String(e), 15_000, { label: `安装（${pack.installSize}）`, run: () => void install(pack) });
        else notify.hint(String(e), 10_000);
      })
      .finally(() => task.end());
  }

  update(u: ViewUpdate) {
    if (!u.docChanged) return;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => this.flush(), DEBOUNCE);
  }

  /** 攒着的改动现在就发。问之前、编辑器销毁之前都要先发 */
  flush() {
    if (!this.timer) return;
    clearTimeout(this.timer);
    this.timer = null;
    if (this.ready) void lspOpen(this.root, this.path, this.view.state.doc.toString()).catch(() => {});
  }

  destroy() {
    this.flush();
    live.delete(this);
    viewsOf.get(this.path)?.delete(this.view);
    syncs.delete(this.view);
  }
}

// ── 跳转：同 java-lsp 的「先缓存、回头补下划线」 ──

const genField = StateField.define<number>({ create: () => 0, update: (g, tr) => (tr.docChanged ? g + 1 : g) });

interface Asked {
  gen: number;
  from: number;
  hit: JumpHit | null | undefined;
  wait: Promise<void>;
}
const asked = new WeakMap<EditorView, Asked>();

/** 光标底下的标识符（字母数字、下划线、$）。各语言的标识符字符不全一样，这几样是大家都认的 */
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

/** 落点 → 跳到哪。项目外的（标准库、依赖的源码）这一版不跳：nav 只认项目里的路径 */
function targetOf(loc: JavaLocation, root: string, self: string): JumpHit["target"] | null {
  if (!loc.path) return null;
  if (loc.path === self) return { rel: "", line: loc.line + 1, col: loc.character + 1, why: "语言服务器" };
  if (!loc.path.startsWith(`${root}/`)) return null;
  return { rel: loc.path.slice(root.length + 1), line: loc.line + 1, col: loc.character + 1, why: "语言服务器" };
}

export function lspResolve(view: EditorView, pos: number, recheck: () => void): JumpHit | null | undefined {
  const s = syncs.get(view);
  if (!s?.ready) return null;
  const w = wordAt(view.state.doc, pos);
  if (!w) return null;
  const gen = view.state.field(genField, false) ?? 0;
  const a = asked.get(view);
  if (a && a.gen === gen && a.from === w.from) return a.hit;
  s.flush();
  const line = view.state.doc.lineAt(w.from);
  const entry: Asked = { gen, from: w.from, hit: undefined, wait: Promise.resolve() };
  entry.wait = lspDefinition(s.root, s.path, line.number - 1, w.from - line.from)
    .then((locs) => {
      const loc = locs[0];
      const target = loc && view.state.field(genField, false) === gen ? targetOf(loc, s.root, s.path) : null;
      // 落点就是这个词自己（声明处）：不画下划线（点了原地不动比没有下划线更糟）
      const self = loc?.path === s.path && offset(view.state.doc, loc.line, loc.character) >= w.from && offset(view.state.doc, loc.line, loc.character) <= w.to;
      entry.hit = target && !self ? { from: w.from, to: w.to, text: view.state.doc.sliceString(w.from, w.to), target } : null;
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

export function lspSettle(view: EditorView): Promise<void> | null {
  const a = asked.get(view);
  return a && a.hit === undefined ? a.wait : null;
}

// ── 查用法（⌥F7）、文档（F1） ──

let popup: ReturnType<typeof mount> | null = null;
function closePopup() {
  if (popup) void unmount(popup);
  popup = null;
}

function here(view: EditorView): { s: Sync; w: { from: number; to: number }; name: string; line: number; ch: number } | null {
  const s = syncs.get(view);
  if (!s) return null;
  if (!s.ready) {
    notify.hint(`${s.pack.name} 语言服务器尚未就绪`);
    return null;
  }
  const w = wordAt(view.state.doc, view.state.selection.main.head);
  if (!w) {
    notify.hint("光标处没有标识符");
    return null;
  }
  s.flush();
  const l = view.state.doc.lineAt(w.from);
  return { s, w, name: view.state.doc.sliceString(w.from, w.to), line: l.number - 1, ch: w.from - l.from };
}

async function findUsages(view: EditorView) {
  const h = here(view);
  if (!h) return;
  const t = progress.start(`正在查找 ${h.name} 的用法`);
  let r: Awaited<ReturnType<typeof lspReferences>>;
  try {
    r = await lspReferences(h.s.root, h.s.path, h.line, h.ch);
  } catch (e) {
    return void notify.fail(`无法查找 ${h.name} 的用法：${e}`);
  } finally {
    t.end();
  }
  if (!view.dom.isConnected) return;
  const c = view.coordsAtPos(h.w.from);
  closePopup();
  popup = mount(UsagesPopup, {
    target: document.body,
    props: {
      name: h.name,
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
        const target = targetOf(loc, h.s.root, h.s.path);
        if (!target) return void notify.hint(`${loc.label} 不在此项目中，暂不支持跳转`);
        void nav.jumpTo({ from: h.w.from, to: h.w.to, text: h.name, target });
      },
    },
  });
}

async function quickDoc(view: EditorView) {
  const h = here(view);
  if (!h) return;
  let md: string;
  try {
    md = await lspHover(h.s.root, h.s.path, h.line, h.ch);
  } catch (e) {
    return void notify.fail(`无法获取 ${h.name} 的文档：${e}`);
  }
  if (!view.dom.isConnected) return;
  if (!md.trim()) return void notify.hint(`${h.name} 没有文档`);
  const c = view.coordsAtPos(h.w.from);
  closePopup();
  const { default: DocPopup } = await import("./DocPopup.svelte");
  popup = mount(DocPopup, {
    target: document.body,
    props: {
      md,
      x: c?.left ?? 200,
      y: c?.bottom ?? 200,
      onclose: () => {
        closePopup();
        view.focus();
      },
    },
  });
}

// ── 改名（⇧F6）、格式化（⌥⌘L）、快速修复（⌥↵）：照 java-lsp 那几段，落盘走跨文件替换那套 ──

/** 跨文件的改动落了盘：开着的标签跟上、记下「撤销上次的替换 / 改名」、胶囊上带撤销（同 Java 的改名） */
async function landed(root: string, label: string, o: import("../ipc/commands").ReplaceOutcome) {
  const ops = await import("../state/replace-ops");
  await ops.landInTabs(o);
  const { replace } = await import("../state/replace.svelte");
  const hits = o.changed.reduce((n, c) => n + c.count, 0);
  const text = `${label}：${o.changed.length} 个文件 ${hits} 处`;
  replace.done = o.undo ? { root, files: o.changed.length, hits, label: text } : null;
  notify.ok(text, undefined, o.undo ? ops.UNDO : undefined);
}

async function renameAt(view: EditorView) {
  const s = syncs.get(view);
  if (!s) return;
  if (!s.ready) return void notify.hint(`${s.pack.name} 语言服务器尚未就绪，暂时无法重命名`);
  const head = view.state.selection.main.head;
  const line = view.state.doc.lineAt(head);
  const at = { line: line.number - 1, character: head - line.from };
  let target: Awaited<ReturnType<typeof lspPrepareRename>>;
  try {
    target = await lspPrepareRename(s.root, s.path, view.state.doc.toString(), at.line, at.character);
  } catch (e) {
    return void notify.fail(`无法重命名：${e}`);
  }
  if (!target) return void notify.hint("光标处的元素不支持重命名");
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
        void (async () => {
          const t = progress.start(`正在将 ${name} 重命名为 ${to}`);
          try {
            const ops = await import("../state/replace-ops");
            const o = await lspRename(s.root, s.path, view.state.doc.toString(), at.line, at.character, to, ops.openDocs());
            await landed(s.root, `已将 ${name} 重命名为 ${to}`, o);
          } catch (e) {
            notify.fail(`重命名失败：${e instanceof Error ? e.message : String(e)}`, 8000);
          } finally {
            t.end();
          }
        })();
      },
    },
  });
}

/** 改动交回编辑器，一笔事务（⌘Z 能撤，不写盘）。问的时候和回来的时候文本变了就不放：位置是在旧文本上算的 */
function putEdits(view: EditorView, edits: { line: number; character: number; endLine: number; endCharacter: number; text: string }[], gen: number, what: string, userEvent: string): boolean | null {
  if (!view.dom.isConnected) return null;
  if ((view.state.field(genField, false) ?? 0) !== gen) {
    notify.hint(`文件在${what}期间已被修改，未应用更改，请重试`);
    return null;
  }
  const doc = view.state.doc;
  const changes = edits
    .map((e) => ({ from: offset(doc, e.line, e.character), to: offset(doc, e.endLine, e.endCharacter), insert: e.text }))
    .filter((c) => c.insert !== doc.sliceString(c.from, c.to))
    .sort((a, b) => a.from - b.from || a.to - b.to);
  if (!changes.length) return false;
  view.dispatch({ changes, userEvent, scrollIntoView: true });
  return true;
}

async function formatDoc(view: EditorView) {
  const s = syncs.get(view);
  if (!s) return;
  if (!s.ready) return void notify.hint(`${s.pack.name} 语言服务器尚未就绪，暂时无法格式化`);
  const st = view.state;
  const unit = st.facet(indentUnit);
  const tabs = unit.includes("\t");
  const gen = st.field(genField, false) ?? 0;
  let edits: Awaited<ReturnType<typeof lspFormat>>;
  try {
    edits = await lspFormat(s.root, s.path, st.doc.toString(), tabs ? st.tabSize : unit.length, !tabs);
  } catch (e) {
    return void notify.fail(`格式化失败：${e instanceof Error ? e.message : String(e)}`);
  }
  const done = putEdits(view, edits, gen, "格式化", "input.format");
  if (done === false) notify.hint("代码格式无需调整");
  else if (done) notify.ok("已格式化");
}

async function quickFix(view: EditorView) {
  const s = syncs.get(view);
  if (!s) return;
  if (!s.ready) return void notify.hint(`${s.pack.name} 语言服务器尚未就绪，暂时无法快速修复`);
  const st = view.state;
  const sel = st.selection.main;
  const a = st.doc.lineAt(sel.from);
  const b = st.doc.lineAt(sel.to);
  const range = { line: a.number - 1, character: sel.from - a.from, endLine: b.number - 1, endCharacter: sel.to - b.from };
  let items: Awaited<ReturnType<typeof lspCodeActions>>;
  try {
    items = await lspCodeActions(s.root, s.path, st.doc.toString(), range);
  } catch (e) {
    return void notify.fail(`无法获取快速修复：${e instanceof Error ? e.message : String(e)}`);
  }
  if (!view.dom.isConnected) return;
  if (!items.length) return void notify.hint("此处没有可用的快速修复");
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
      onpick: (it: { title: string; raw: string }) => {
        closePopup();
        view.focus();
        void (async () => {
          const t = progress.start(`正在执行：${it.title}`);
          try {
            const ops = await import("../state/replace-ops");
            const r = await lspApplyAction(s.root, s.path, it.raw, ops.openDocs());
            if (r.outcome) return void (await landed(s.root, it.title, r.outcome));
            const done = putEdits(view, r.edits, gen, "修复", "input.fix");
            if (done === false) notify.hint(`「${it.title}」未产生更改`);
            else if (done) notify.ok(it.title);
          } catch (e) {
            notify.fail(`「${it.title}」失败：${e instanceof Error ? e.message : String(e)}`, 8000);
          } finally {
            t.end();
          }
        })();
      },
    },
  });
}

// ── 补全 ──

/** 补全源。语言服务器没起好时退回「文档里出现过的词」；只在第一个字和 `.` / `:` 后面问，之后在已有的里面筛（除非服务器说它截了） */
async function source(ctx: CompletionContext): Promise<CompletionResult | null> {
  const view = ctx.view;
  const s = view && syncs.get(view);
  if (!s?.ready) return completeAnyWord(ctx);
  const word = ctx.matchBefore(/[\p{L}\p{N}_$]*/u);
  const from = word?.from ?? ctx.pos;
  const before = ctx.state.sliceDoc(from - 1, from);
  if (!ctx.explicit && from === ctx.pos && before !== "." && before !== ":") return null;
  const doc = ctx.state.doc;
  const line = doc.lineAt(ctx.pos);
  let r: Awaited<ReturnType<typeof lspComplete>>;
  try {
    r = await lspComplete(s.root, s.path, doc.toString(), line.number - 1, ctx.pos - line.from);
  } catch {
    return null;
  }
  if (ctx.aborted || !r.items.length) return null;
  const starts = r.items.filter((i) => i.edit).map((i) => offsetOf(doc, i.edit!.line, i.edit!.character));
  const start = starts.length ? Math.min(from, ...starts) : from;
  return {
    from: start,
    options: r.items.map((it, i) => {
      const text = it.edit?.text ?? it.insert ?? it.label;
      return {
        label: it.label,
        detail: it.detail || it.description || undefined,
        type: kindType(it.kind),
        boost: boostOf(i, r.items.length),
        apply: (v: EditorView, c: unknown, f: number, t: number) => {
          if (it.snippet) snippet(lspSnippet(text))(v, c as never, f, t);
          else v.dispatch({ changes: { from: f, to: t, insert: text }, selection: { anchor: f + text.length }, userEvent: "input.complete", scrollIntoView: true });
        },
      };
    }),
    validFor: r.incomplete ? undefined : /^[\p{L}\p{N}_$]*$/u,
  };
}

export function lspGeneric(root: string | null, path: string, pack: LspPack): Extension {
  if (!root || !path.startsWith(`${root}/`)) return [];
  wire();
  return [
    // 编辑器上标一下「接着哪个语言包」：验收（lsp.sh）等它出现再按键 —— 这个扩展是问完包的列表才装上的，开文件当场就按键会落空
    EditorView.editorAttributes.of({ "data-lsp": pack.id }),
    genField,
    diagField,
    diagHover,
    autocompletion({ override: [source] }),
    ViewPlugin.define((view) => new Sync(view, root, path, pack)),
    Prec.high(
      idKeymap([
        { id: "find-usages", preventDefault: true, run: (v) => (void findUsages(v), true) },
        { id: "quick-doc", preventDefault: true, run: (v) => (void quickDoc(v), true) },
        { id: "rename", preventDefault: true, run: (v) => (void renameAt(v), true) },
        { id: "quick-fix", preventDefault: true, run: (v) => (void quickFix(v), true) },
        // ⌥⌘L：同 Java 那边挂在「格式化 JSON」那一行上（keymap.ts 里是同一行，说明写了两种），Prec.high 排在 Editor 里那条前面
        { id: "cm-format-json", preventDefault: true, run: (v) => (void formatDoc(v), true) },
      ]),
    ),
    EditorView.theme({
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
