/**
 * MyBatis 的 Mapper 接口 ↔ XML 跳转（docs/IDEA.md 第 24 节，IDEA 的 MyBatisX 插件那套）：接口方法旁边一个图标，点了跳到 XML 里那段 SQL；
 * XML 里 `<select|insert|update|delete id="…">` 旁边一个，点了跳回接口方法。两边都有的才画 —— 画一个点了没反应的图标比不画糟。
 *
 * 配对的规则和两边的解析都在 Rust（`mybatis.rs`）：这里把编辑器里此刻的文本交过去（没保存的也算），拿回「这边的点」和「对面的点」，按名字配。
 * 不靠 jdtls：打开就有。打开时问一次；改动停下 1 秒再问一次（加了一个方法、改了 id）；中间图标跟着行走。
 * 只有看着像的才问：Java 里有 `interface`、XML 里有 `<mapper` —— 普通文件不白跑一趟项目搜索
 */
import { EditorView, GutterMarker, ViewPlugin, gutter } from "@codemirror/view";
import { RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import type { MybatisLinks, MybatisSpot } from "../ipc/commands";
import { mybatisForJava, mybatisForXml } from "../ipc/java";
import { nav } from "../state/nav.svelte";

class Mark extends GutterMarker {
  // 不写成 `constructor(readonly …)`：那是要编译的 TS 语法，状态层测试在裸 node 里剥类型跑，剥不掉（java-debug-gutter 同一条）
  readonly to: MybatisSpot;
  readonly fromJava: boolean;
  constructor(to: MybatisSpot, fromJava: boolean) {
    super();
    this.to = to;
    this.fromJava = fromJava;
  }
  override eq(o: Mark) {
    return o.to.path === this.to.path && o.to.line === this.to.line && o.to.name === this.to.name;
  }
  override toDOM() {
    const el = document.createElement("span");
    el.className = this.fromJava ? "cm-mb to-xml" : "cm-mb to-java";
    const file = this.to.path.slice(this.to.path.lastIndexOf("/") + 1);
    el.title = this.fromJava ? `跳到 XML：<${this.to.kind} id="${this.to.name}">（${file}:${this.to.line}）` : `跳到接口方法：${file.replace(/\.java$/, "")}.${this.to.name}`;
    return el;
  }
}

const setMarks = StateEffect.define<RangeSet<Mark>>();

const marks = StateField.define<RangeSet<Mark>>({
  create: () => RangeSet.empty,
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setMarks)) return e.value;
    return tr.docChanged ? v.map(tr.changes) : v;
  },
});

/** 回答 → 图标：这边的每个点，对面有同名的（取第一个）就在它那一行画一个 */
export function marksFor(state: EditorState, links: MybatisLinks, fromJava: boolean): RangeSet<Mark> {
  const there = new Map<string, MybatisSpot>();
  for (const t of links.there) if (!there.has(t.name)) there.set(t.name, t);
  const doc = state.doc;
  const out = links.here
    .filter((h) => h.line >= 1 && h.line <= doc.lines && there.has(h.name))
    .map((h) => new Mark(there.get(h.name)!, fromJava).range(doc.line(h.line).from));
  return RangeSet.of(out, true);
}

const looksLike = (text: string, java: boolean) => (java ? /\binterface\s+\w/.test(text) : text.includes("<mapper"));

export function mybatisGutter(root: string | null, path: string): Extension {
  const java = path.endsWith(".java");
  if (!root || !(java || path.endsWith(".xml"))) return [];
  const ask = java ? mybatisForJava : mybatisForXml;
  return [
    marks,
    gutter({
      class: "cm-mybatisGutter",
      markers: (v) => v.state.field(marks),
      domEventHandlers: {
        mousedown(view, line) {
          let hit: Mark | null = null;
          view.state.field(marks).between(line.from, line.from, (_f, _t, m) => {
            hit = m;
          });
          const m = hit as Mark | null;
          if (!m) return false;
          void nav.openAt(m.to.path, m.to.line, false);
          return true;
        },
      },
    }),
    ViewPlugin.define((view) => {
      let timer: ReturnType<typeof setTimeout> | null = null;
      let seq = 0;
      const query = async () => {
        timer = null;
        const text = view.state.doc.toString();
        const my = ++seq;
        if (!looksLike(text, java)) {
          if (view.state.field(marks).size) view.dispatch({ effects: setMarks.of(RangeSet.empty) });
          return;
        }
        let links: MybatisLinks;
        try {
          links = await ask(root, text);
        } catch {
          return; // 读不了 / 搜不了：不画就是了，不为一个图标糊一句红字
        }
        // 问的时候又改了：这次的作废（下一次已经排上了）；编辑器关了也不画
        if (my !== seq || !view.dom.isConnected || view.state.doc.toString() !== text) return;
        view.dispatch({ effects: setMarks.of(marksFor(view.state, links, java)) });
      };
      void query();
      return {
        update(u) {
          if (!u.docChanged) return;
          if (timer) clearTimeout(timer);
          timer = setTimeout(() => void query(), 1000);
        },
        destroy() {
          if (timer) clearTimeout(timer);
          seq++;
        },
      };
    }),
    EditorView.baseTheme({
      ".cm-mybatisGutter .cm-gutterElement": { padding: "0 2px", display: "flex", alignItems: "center", justifyContent: "center" },
      // IDEA 的 gutter 图标本来就带色（绿底的圆 + 箭头），当背景图画
      ".cm-mb": { width: "14px", height: "14px", cursor: "pointer", backgroundSize: "14px 14px", backgroundRepeat: "no-repeat", backgroundPosition: "center" },
      ".cm-mb.to-xml": { backgroundImage: "url(/icons/to-xml.svg)" },
      ".cm-mb.to-java": { backgroundImage: "url(/icons/to-java.svg)" },
    }),
  ];
}
