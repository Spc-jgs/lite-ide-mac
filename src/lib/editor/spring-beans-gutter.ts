/**
 * Spring 注入点 → Bean（docs/TAKEOVER.md G30 的 Bean 导航，IDEA Ultimate 行号旁那个图标）：`@Autowired` 字段、Bean 类的构造器参数、
 * Lombok `@RequiredArgsConstructor` 的 final 字段旁边一个图标，点了跳到被注入的那个 Bean（实现类、`@Bean` 方法、`@Mapper` 接口）；
 * 配上好几个的弹一个列表挑（同 ⌥⌘B 跳到实现）。
 *
 * 认注入点、找 Bean 都在 Rust（`spring_beans.rs`，读文本、不等 jdtls）：这里把编辑器里此刻的文本交过去，拿回配上了的注入点画图标。
 * 打开时问一次；改动停下 1 秒再问（同 MyBatis 那个）。只有看着像 Spring 的才问：有 `@Autowired` / `@Resource` / `@Inject` 或者 Bean 类的注解
 */
import { EditorView, GutterMarker, ViewPlugin, gutter } from "@codemirror/view";
import { RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { mount, unmount } from "svelte";
import type { JavaLocation, SpringBean, SpringInject } from "../ipc/commands";
import { springBeans } from "../ipc/java";
import { nav } from "../state/nav.svelte";
import UsagesPopup from "./UsagesPopup.svelte";

class Mark extends GutterMarker {
  // 不写成 `constructor(readonly …)`：同 mybatis-gutter 那条
  readonly at: SpringInject;
  constructor(at: SpringInject) {
    super();
    this.at = at;
  }
  override eq(o: Mark) {
    return o.at.ty === this.at.ty && o.at.beans.length === this.at.beans.length && o.at.beans.every((b, i) => b.path === this.at.beans[i].path && b.line === this.at.beans[i].line);
  }
  override toDOM() {
    const el = document.createElement("span");
    el.className = "cm-sb";
    const b = this.at.beans;
    el.title = b.length === 1 ? `转到注入的 Bean：${b[0].label}` : `转到注入的 Bean（${b.length} 个候选：${b.map((x) => x.label).join("、")}）`;
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

/** 回答 → 图标：一行可能有两个注入点（构造器的两个参数），画一个、点了列全部 */
export function marksFor(state: EditorState, list: SpringInject[]): RangeSet<Mark> {
  const doc = state.doc;
  const byLine = new Map<number, SpringInject>();
  for (const i of list) {
    if (i.line < 1 || i.line > doc.lines) continue;
    const had = byLine.get(i.line);
    byLine.set(i.line, had ? { ...had, ty: `${had.ty}, ${i.ty}`, beans: [...had.beans, ...i.beans] } : i);
  }
  return RangeSet.of([...byLine.entries()].sort((a, b) => a[0] - b[0]).map(([l, i]) => new Mark(i).range(doc.line(l).from)), true);
}

const looksLike = (text: string) => /@(Autowired|Resource|Inject|Service|Component|Controller|RestController|Repository|Configuration)\b/.test(text);

let popup: ReturnType<typeof mount> | null = null;
const closePopup = () => {
  if (popup) void unmount(popup);
  popup = null;
};

const toLoc = (b: SpringBean): JavaLocation => ({ uri: "", path: b.path, label: b.label, line: b.line - 1, character: 0, endLine: b.line - 1, endCharacter: 0, text: b.label });

function open(view: EditorView, pos: number, at: SpringInject) {
  if (at.beans.length === 1) return void nav.openAt(at.beans[0].path, at.beans[0].line, false);
  const c = view.coordsAtPos(pos);
  closePopup();
  popup = mount(UsagesPopup, {
    target: document.body,
    props: {
      name: at.ty,
      title: `注入的 Bean：${at.ty}`,
      items: at.beans.map(toLoc),
      total: at.beans.length,
      x: c?.left ?? 200,
      y: c?.bottom ?? 200,
      onclose: () => {
        closePopup();
        view.focus();
      },
      onpick: (loc: JavaLocation) => {
        closePopup();
        if (loc.path) void nav.openAt(loc.path, loc.line + 1, false);
      },
    },
  });
}

export function springBeansGutter(root: string | null, path: string): Extension {
  if (!root || !path.endsWith(".java")) return [];
  return [
    marks,
    gutter({
      class: "cm-springBeansGutter",
      markers: (v) => v.state.field(marks),
      domEventHandlers: {
        mousedown(view, line) {
          let hit: Mark | null = null;
          view.state.field(marks).between(line.from, line.from, (_f, _t, m) => {
            hit = m;
          });
          const m = hit as Mark | null;
          if (!m) return false;
          open(view, line.from, m.at);
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
        if (!looksLike(text)) {
          if (view.state.field(marks).size) view.dispatch({ effects: setMarks.of(RangeSet.empty) });
          return;
        }
        let list: SpringInject[];
        try {
          list = await springBeans(root, text);
        } catch {
          return; // 读不了 / 搜不了：不画（同 MyBatis 那个）
        }
        if (my !== seq || !view.dom.isConnected || view.state.doc.toString() !== text) return;
        view.dispatch({ effects: setMarks.of(marksFor(view.state, list)) });
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
          closePopup();
        },
      };
    }),
    EditorView.baseTheme({
      ".cm-springBeansGutter .cm-gutterElement": { padding: "0 2px", display: "flex", alignItems: "center", justifyContent: "center" },
      // IDEA 社区版没有 Spring 的图标（在 Ultimate 里）：借 gutter/implementingMethod（「有实现，点了跳过去」，同 MyBatis 那个），SOURCES.md 记着
      ".cm-sb": { width: "14px", height: "14px", cursor: "pointer", backgroundSize: "14px 14px", backgroundRepeat: "no-repeat", backgroundPosition: "center", backgroundImage: "url(/icons/to-xml.svg)" },
    }),
  ];
}
