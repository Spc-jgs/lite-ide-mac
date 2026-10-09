/**
 * 编辑 `settings.json` 时专用的扩展（issue #44 第 4 步，docs/SETTINGS.md 7.4）。跟着编辑器懒加载，不进入口包。
 *
 * - **高亮**：不用 `@codemirror/lang-json`。第 0 步实测：lezer 的 JSON 语法不认注释，注释里的 `"JetBrains Mono"` 被认成键名，
 *   注释掉的整行默认值和生效的设置一个颜色 —— 模板恰恰是「所有键都注释着列出来」。legacy-modes 的 `json` 认注释；
 *   它把键标成 `property`，不补这张表就和值一个颜色（还打一行 `Unknown highlighting tag property`）
 * - **补全**：行首敲 `"` 列出全部设置键，选了插入 `"键": 默认值`。键的定义来自 Rust（`settings_schema`），这边不另抄一份
 * - **问题标在行上**：Rust 读这份文件时发现的问题（行、列、一句话）画成那一行的底色 + 行尾一句话。
 *   没引 `@codemirror/lint`：它不是这个项目的直接依赖，而这里要的只是「这行有问题、问题是什么」，几十行的事
 */
import { StreamLanguage } from "@codemirror/language";
import { json } from "@codemirror/legacy-modes/mode/javascript";
import { tags } from "@lezer/highlight";
import { autocompletion, type Completion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet } from "@codemirror/view";
import { RangeSetBuilder, StateEffect, StateField, type Extension } from "@codemirror/state";
import type { SettingDef, SettingProblem, Settings } from "../ipc/commands";
import { settingsSchema } from "../ipc/app";
import { settings } from "../state/settings.svelte";

const jsonc = StreamLanguage.define({ ...json, tokenTable: { property: tags.propertyName } });

// ── 补全 ──

let defs: Promise<SettingDef[]> | null = null;
const schema = () => (defs ??= settingsSchema().catch(() => ((defs = null), [])));

/**
 * 只在「行首写键」的位置给：`^\s*"` 后面跟着键名的一部分。值的位置、注释里都不给 ——
 * 值该写什么，插入键的时候默认值已经一起插好了
 */
export async function keySource(ctx: CompletionContext): Promise<CompletionResult | null> {
  const line = ctx.state.doc.lineAt(ctx.pos);
  const m = /^\s*"([\w.]*)$/.exec(line.text.slice(0, ctx.pos - line.from));
  if (!m) return null;
  const from = ctx.pos - m[1].length - 1; // 连同那个 `"` 一起替换
  const options: Completion[] = (await schema()).map((d) => ({
    label: `"${d.key}"`,
    detail: d.default,
    info: d.min !== null ? `${d.doc}（${d.min}–${d.max}）` : d.doc,
    type: "property",
    apply: (view, _c, f, t) => {
      const c = keyInsert(view.state.doc.toString(), f, t, d);
      view.dispatch({ changes: c, selection: { anchor: c.from + c.insert.length } });
    },
  }));
  return { from, options, validFor: /^"[\w.]*$/ };
}

/**
 * 选了一个键之后替换哪一段、插什么。括号自动配对已经补了一个收尾的 `"`：一起吃掉，
 * 不然插完是 `"editor.fontSize": 13"`。拎成纯函数是为了在裸 node 里测（那里起不了 EditorView）
 */
export function keyInsert(doc: string, from: number, to: number, d: Pick<SettingDef, "key" | "default">) {
  const end = doc.slice(to, to + 1) === '"' ? to + 1 : to;
  return { from, to: end, insert: `"${d.key}": ${d.default}` };
}

// ── 问题标在行上 ──

const setProblems = StateEffect.define<SettingProblem[]>();

class Note extends WidgetType {
  // 不写成 `constructor(readonly text)`：那是要编译的 TS 写法，测试跑的 node 只会剥类型，不认它
  readonly text: string;
  constructor(text: string) {
    super();
    this.text = text;
  }
  override eq(o: Note) {
    return o.text === this.text;
  }
  override toDOM() {
    const s = document.createElement("span");
    s.className = "cm-setting-note";
    s.textContent = this.text;
    return s;
  }
}

/** 问题 → 装饰。行号越界的夹到最后一行（问题是按存盘那份算的，编辑器里可能已经删了几行） */
export function decorate(doc: EditorView["state"]["doc"], problems: SettingProblem[]): DecorationSet {
  const byLine = new Map<number, string[]>();
  for (const p of problems) {
    const n = Math.min(Math.max(1, p.line ?? 1), doc.lines);
    byLine.set(n, [...(byLine.get(n) ?? []), p.text]);
  }
  const b = new RangeSetBuilder<Decoration>();
  for (const n of [...byLine.keys()].sort((a, c) => a - c)) {
    const ln = doc.line(n);
    b.add(ln.from, ln.from, Decoration.line({ class: "cm-setting-bad" }));
    b.add(ln.to, ln.to, Decoration.widget({ widget: new Note(byLine.get(n)!.join("；")), side: 1 }));
  }
  return b.finish();
}

const problemField = StateField.define<DecorationSet>({
  create: (st) => decorate(st.doc, settings.v.problems),
  update(deco, tr) {
    for (const e of tr.effects) if (e.is(setProblems)) return decorate(tr.state.doc, e.value);
    // 改着的时候标注跟着文字走；存盘之后 Rust 重新算，再整份换掉
    return tr.docChanged ? deco.map(tr.changes) : deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** 设置变了（存盘 → Rust 重读 → 广播）就把标注整份换掉 */
const follow = ViewPlugin.define((view) => {
  const off = settings.subscribe((s: Settings) => {
    // 广播是在别的调用栈里来的，推迟一拍再 dispatch，别撞上正在进行的更新
    queueMicrotask(() => view.dispatch({ effects: setProblems.of(s.problems) }));
  });
  return { destroy: off };
});

const theme = EditorView.baseTheme({
  ".cm-setting-bad": { backgroundColor: "rgba(214, 170, 60, 0.12)" },
  ".cm-setting-note": {
    marginLeft: "2em",
    color: "var(--lvl-warn, #d6aa3c)",
    fontStyle: "italic",
    fontSize: "0.9em",
  },
});

export function settingsFile(): Extension {
  return [jsonc, autocompletion({ override: [keySource] }), problemField, follow, theme];
}
