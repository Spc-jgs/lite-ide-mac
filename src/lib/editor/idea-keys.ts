/**
 * 编辑器键位照 IDEA（macOS 默认键位）。
 *
 * # 为什么
 *
 * 界面和导航键一直照 IDEA（⌘B ⌘E ⇧⇧ ⌘L ⌥⌘←），编辑器里却直接用了 CM6 的
 * `defaultKeymap` / `searchKeymap` —— 那是 VSCode 的手感。手上是 IDEA 肌肉记忆的人
 * 在编辑器里会一再撞上（2026-09-23 对着 CM6 源码逐条核的）：
 *
 * | 按 | IDEA | CM6 默认 |
 * |---|---|---|
 * | ⌥⇧↓ | 把这行往下挪 | **复制这行**（悄悄多一行重复代码 —— 最危险的一条） |
 * | ⌘D | 复制行 / 选区 | 选中下一个相同的词 |
 * | ⌥↑ | 扩大选区 | 把这行往上挪 |
 * | ⌘⌫ | 删掉这行 | 删到行首 |
 * | ⌘[ | 回到上一个位置 | 减一级缩进 |
 *
 * 混着用比统一成哪一家都糟 —— 两边的人都会被坑。用户拍板跟 IDEA。
 *
 * 这张表排在 `defaultKeymap` 前面（`Editor.svelte`），同一个键先到先得；
 * `⌘[` `⌘]` 从默认键位里摘掉（`editorDefaults`），让给窗口级的回退 / 前进。
 * 每一条都登记在 `state/keymap.ts`（owner = cm6）—— 菜单再来抢同一个键，
 * `keymap.test.ts` 的「同一个键位不许挂两条」当场红（`⌘/`、`⇧⌘G` 就是这么被抢了几周没人发现的：
 * CM6 自带的键不在表里，测试看不见它们）。
 */
import { EditorSelection, StateEffect, StateField, type StateCommand, type Extension } from "@codemirror/state";
import type { KeyBinding } from "@codemirror/view";
import {
  copyLineDown,
  defaultKeymap,
  deleteLine,
  moveLineDown,
  moveLineUp,
  selectParentSyntax,
  toggleBlockComment,
} from "@codemirror/commands";
import { selectNextOccurrence, selectSelectionMatches } from "@codemirror/search";

/**
 * ⌘D：没有选区复制当前行（光标落到下面那份，同 IDEA）；有选区就把选中的文字紧跟着再贴一份，
 * 选中新贴的那份 —— IDEA 的「Duplicate Line or Selection」。
 */
export const duplicate: StateCommand = ({ state, dispatch }) => {
  if (state.selection.ranges.every((r) => r.empty)) return copyLineDown({ state, dispatch });
  const tr = state.changeByRange((r) =>
    r.empty
      ? { range: r }
      : {
          changes: { from: r.to, insert: state.sliceDoc(r.from, r.to) },
          range: EditorSelection.range(r.to, r.to + (r.to - r.from)),
        },
  );
  dispatch(state.update(tr, { scrollIntoView: true, userEvent: "input.copyline" }));
  return true;
};

/**
 * ⌥↑ / ⌥↓：扩大 / 缩小选区。扩大走语法树（`selectParentSyntax`），每扩一次把原来的选区压栈，
 * 缩小就是出栈 —— 语法树往下没有唯一的「子选区」，只能记着来时的路。
 * 光标动了、文档改了，栈就作废（再按 ⌥↓ 不该跳回一个早就不相干的选区）。
 */
const pushSel = StateEffect.define<EditorSelection>();
const popSel = StateEffect.define<null>();

const selStack = StateField.define<readonly EditorSelection[]>({
  create: () => [],
  update(stack, tr) {
    for (const e of tr.effects) {
      if (e.is(pushSel)) return [...stack, e.value];
      if (e.is(popSel)) return stack.slice(0, -1);
    }
    return tr.docChanged || tr.selection ? [] : stack;
  },
});

export const extendSelection: StateCommand = ({ state, dispatch }) => {
  // 第一步先选中光标下的词（IDEA 就是这样）：在字符串 / 注释里，语法树最小的节点是整段
  let sel = state.selection;
  if (sel.ranges.some((r) => r.empty)) {
    sel = EditorSelection.create(
      sel.ranges.map((r) => {
        const w = r.empty ? state.wordAt(r.head) : null;
        return w ? EditorSelection.range(w.from, w.to) : r;
      }),
      sel.mainIndex,
    );
  }
  if (!sel.eq(state.selection)) {
    dispatch(state.update({ selection: sel, effects: pushSel.of(state.selection), scrollIntoView: true }));
    return true;
  }
  // 装进对象里接：在回调里给 let 赋值，TS 的控制流分析看不见，会把它一直当成 null
  const got: { sel?: EditorSelection } = {};
  selectParentSyntax({ state, dispatch: (tr) => (got.sel = tr.newSelection) });
  // 扩不动了也吃掉这一下：返回 false 会落到默认键位的 ⌥↑ = 挪行 —— 正是要消灭的那个意外
  if (got.sel) dispatch(state.update({ selection: got.sel, effects: pushSel.of(state.selection), scrollIntoView: true }));
  return true;
};

export const shrinkSelection: StateCommand = ({ state, dispatch }) => {
  const stack = state.field(selStack, false);
  const prev = stack?.[stack.length - 1];
  if (prev) dispatch(state.update({ selection: prev, effects: popSel.of(null), scrollIntoView: true }));
  return true; // 同上：不许落到默认的 ⌥↓ = 挪行
};

/**
 * ⌃⌘G：选中所有相同的词（IDEA 的 Select All Occurrences）。
 *
 * 从光标出发（没有选区）时要**整词**：光标在 `id` 上，`idx` 里那两个字母不算。
 * `selectSelectionMatches` 是按子串找的，所以这种情况改成反复 `selectNextOccurrence`
 * （它从光标起步时就是整词匹配），直到它不再加新的。有选区时照 IDEA 按子串。
 */
export const selectAllOccurrences: StateCommand = (target) => {
  if (!target.state.selection.main.empty) return selectSelectionMatches(target);
  let st = target.state;
  // 上限只是防御：每一轮至少多一个选区，一份文档里的词数到不了这么多
  for (let i = 0; i < 100_000; i++) {
    let moved = false;
    selectNextOccurrence({ state: st, dispatch: (tr) => ((st = tr.state), (moved = true)) });
    if (!moved) break;
  }
  if (st === target.state) return false;
  target.dispatch(target.state.update({ selection: st.selection, scrollIntoView: true }));
  return true;
};

/**
 * ⇧⌘U：切换大小写。有小写字母就全变大写，否则全变小写（IDEA 的判据）。
 * 没有选区时作用于光标下的词，光标不动。
 */
export const toggleCase: StateCommand = ({ state, dispatch }) => {
  let any = false;
  const tr = state.changeByRange((r) => {
    const span = r.empty ? state.wordAt(r.head) : r;
    if (!span || span.from === span.to) return { range: r };
    const text = state.sliceDoc(span.from, span.to);
    const next = /\p{Ll}/u.test(text) ? text.toUpperCase() : text.toLowerCase();
    if (next === text) return { range: r };
    any = true;
    // 大小写转换可能改变长度（ß → SS），选区按新长度重算
    return {
      changes: { from: span.from, to: span.to, insert: next },
      range: r.empty ? r : EditorSelection.range(span.from, span.from + next.length),
    };
  });
  if (!any) return false;
  dispatch(state.update(tr, { scrollIntoView: true, userEvent: "input" }));
  return true;
};

export const ideaKeymap: readonly KeyBinding[] = [
  { key: "Mod-d", run: duplicate, preventDefault: true },
  { key: "Mod-Backspace", run: deleteLine, preventDefault: true },
  { key: "Shift-Alt-ArrowUp", run: moveLineUp, preventDefault: true },
  { key: "Shift-Alt-ArrowDown", run: moveLineDown, preventDefault: true },
  { key: "Alt-ArrowUp", run: extendSelection, preventDefault: true },
  { key: "Alt-ArrowDown", run: shrinkSelection, preventDefault: true },
  { key: "Ctrl-g", run: selectNextOccurrence, preventDefault: true },
  { key: "Ctrl-Mod-g", run: selectAllOccurrences, preventDefault: true },
  { key: "Alt-Mod-/", run: toggleBlockComment, preventDefault: true },
  { key: "Shift-Mod-u", run: toggleCase, preventDefault: true },
];

/**
 * CM6 的默认键位，摘掉 `⌘[` `⌘]`（它拿来减 / 加缩进）—— 那两个键在 IDEA 里是回退 / 前进，
 * 由窗口级 keydown 接（`App.svelte`）。缩进照旧有 Tab / ⇧Tab。
 * 只摘不留：CM6 先跑、`preventDefault` 之后事件照样冒泡到 window，两边都在的话按一下
 * 既减缩进又回退。
 */
export const editorDefaults: readonly KeyBinding[] = defaultKeymap.filter((b) => b.key !== "Mod-[" && b.key !== "Mod-]");

/** 扩大 / 缩小选区要的那个栈 */
export const ideaKeysState: Extension = selStack;
