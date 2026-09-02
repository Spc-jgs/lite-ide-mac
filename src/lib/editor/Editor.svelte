<script lang="ts">
  import { EditorState, Compartment } from "@codemirror/state";
  import { EditorView, keymap, lineNumbers, highlightActiveLine,
           highlightActiveLineGutter, drawSelection, rectangularSelection,
           crosshairCursor, highlightSpecialChars } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { searchKeymap, highlightSelectionMatches, search } from "@codemirror/search";
  import { bracketMatching, foldGutter, foldKeymap, indentOnInput,
           indentUnit } from "@codemirror/language";
  import { ideaDarkTheme, ideaDarkHighlight } from "./theme-idea-dark";
  import { langOf, loadLang } from "./langs";
  import { outlineOf, type Sym } from "./outline";
  import { minimap, setMinimapMarks, type MarkKind } from "./minimap";

  let {
    path,
    initial,
    baseline = null,
    savedTick = 0,
    gotoLine = null,
    outlineTick = 0,
    marks = null,
    showMinimap = true,
    onChange,
    onSave,
    onStash,
    onOutline,
    onCursor,
  }: {
    path: string;
    /** 要显示的文本。有未保存的草稿时**是草稿**，不是磁盘上那份 */
    initial: string;
    /**
     * 磁盘上那份，dirty 的判据。null 表示「和 initial 一样」。
     *
     * 必须和 `initial` **分开**：切标签时草稿会被交回 App 存起来，
     * 切回来时 `initial` 就是草稿了 —— 只有一个字段的话，基线会被草稿顶掉，
     * 于是「有未保存改动」这个标记当场消失，而人完全看不出自己丢了东西。
     */
    baseline?: string | null;
    /** 每次保存成功后自增。用它重置 dirty 基线，比暴露组件 ref 耦合更松 */
    savedTick?: number;
    /** 搜索结果跳转用的目标行（1-based）。同一行连点也要能重新定位，故带 nonce */
    gotoLine?: { line: number; nonce: number } | null;
    /** 自增即重新提取大纲。放在 Editor 里算是因为语法树在它手上 */
    outlineTick?: number;
    /** 相对 HEAD 的改动行，画在缩略图左缘。null 表示不在仓库里或没有改动 */
    marks?: Map<number, MarkKind> | null;
    showMinimap?: boolean;
    onChange: (dirty: boolean) => void;
    onSave: (content: string) => void;
    /**
     * 换文件或组件被销毁**之前**，把编辑器里的实时文本交出去。
     *
     * 没有这一步，`{#key active.id}` 一销毁重建，未保存的改动就没了 ——
     * 而且是静悄悄地没：新实例拿 `initial` 当基线，算出来「不脏」，
     * 连标签上那个圆点都跟着消失。
     *
     * 带上 path 是因为**销毁时读 prop 已经是新标签的值了** ——
     * 组件自己在挂载时快照了一份，交的是它自己那份。
     */
    onStash?: (path: string, text: string) => void;
    onOutline?: (syms: Sym[]) => void;
    /**
     * 光标换行时报一次（1-based）。会话快照用它记住「上次看到哪」。
     *
     * 只在**行号变了**时才报 —— 同一行里左右移动一个字符也回调的话，
     * 敲一行字就是几十次无谓调用。
     */
    onCursor?: (line: number) => void;
  } = $props();

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | null = null;
  /** 语言扩展放在 compartment 里，切文件时热替换而不重建整个 state */
  const langSlot = new Compartment();
  /** 缩略图同理：开关一下不该把光标和撤销栈也重置掉 */
  const mapSlot = new Compartment();
  /** dirty 判定的基线：当前磁盘上的内容。挂载与换文件时更新，不在顶层读 prop */
  let baseText = "";
  /**
   * 视图里现在装的是哪个文件。
   *
   * 换文件和销毁时都要把草稿**还给上一个路径**，而那时 `path` 这个 prop
   * 已经指向新的了 —— 所以自己记一份。
   */
  let curPath = "";
  /** 上次报出去的光标行，用来把「同一行内移动」滤掉 */
  let lastLine = 0;

  function build(doc: string) {
    return EditorState.create({
      doc,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        highlightSpecialChars(),
        drawSelection(),
        /*
         * 这一行不能省，`rectangularSelection` 和 `crosshairCursor` 全靠它。
         *
         * CM6 没开这个 facet 时，**每次事务的选区都会被 `asSingle()` 压成一个**
         * （@codemirror/state 里 `tr.startState.facet(allowMultipleSelections)
         * ? tr.newSelection : tr.newSelection.asSingle()`）。
         * 结果是 ⌥ 拖矩形选择、⌥ 点加光标、搜索里的「选中所有匹配」
         * 三样**全都装了却不生效** —— 扩展照样进包，只是什么也不做。
         * 这类 bug 不报错、不崩溃，只是「按了没反应」。
         */
        EditorState.allowMultipleSelections.of(true),
        rectangularSelection(),
        crosshairCursor(),
        history(),
        foldGutter(),
        indentOnInput(),
        bracketMatching(),
        highlightSelectionMatches(),
        search({ top: true }),
        mapSlot.of(showMinimap ? minimap() : []),
        indentUnit.of("    "),
        langSlot.of([]),
        ideaDarkTheme,
        ideaDarkHighlight,
        keymap.of([
          // ⌘S 存盘。放在最前面，别被默认键位截胡
          {
            key: "Mod-s",
            preventDefault: true,
            run: (v) => {
              onSave(v.state.doc.toString());
              return true;
            },
          },
          ...defaultKeymap,
          ...historyKeymap,
          ...searchKeymap,
          ...foldKeymap,
          indentWithTab,
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) onChange(u.state.doc.toString() !== baseText);
          if (onCursor && (u.selectionSet || u.docChanged)) {
            const line = u.state.doc.lineAt(u.state.selection.main.head).number;
            if (line !== lastLine) {
              lastLine = line;
              onCursor(line);
            }
          }
        }),
      ],
    });
  }

  /**
   * 把编辑器里的实时文本交回 App。换文件之前、销毁之前各调一次。
   *
   * 交的是 `curPath`（自己挂载时记下的那个），不是 `path` ——
   * 销毁发生在切标签之后，那时 `path` 这个 prop 已经是新标签的了。
   */
  function stash() {
    if (!view) return;
    onStash?.(curPath, view.state.doc.toString());
  }

  // 建视图：只在挂载时做一次
  $effect(() => {
    if (!host || view) return;
    curPath = path;
    baseText = baseline ?? initial;
    view = new EditorView({ state: build(initial), parent: host });
    void applyLang(path);
    // 草稿恢复回来时它本来就是脏的，得说出来 —— 不说的话标签上的圆点不会亮
    onChange(initial !== baseText);
    return () => {
      stash();
      view?.destroy();
      view = null;
    };
  });

  // 换文件：整份换掉文档，并热替换语言
  $effect(() => {
    const p = path;
    const text = initial;
    const base = baseline;
    if (!view) return;
    // 真换了文件才收草稿；同一个文件只是内容被外部改了（重读），不能当草稿收走
    if (p !== curPath) stash();
    curPath = p;
    baseText = base ?? text;
    view.setState(build(text));
    void applyLang(p);
    onChange(text !== baseText);
  });

  async function applyLang(p: string) {
    const ext = await loadLang(langOf(p));
    if (!view) return;
    view.dispatch({ effects: langSlot.reconfigure(ext ?? []) });
  }

  // 跳到指定行并居中。nonce 变化即触发，所以连点同一条搜索结果也能重新定位
  $effect(() => {
    const g = gotoLine;
    if (!view || !g) return;
    const total = view.state.doc.lines;
    const line = Math.min(Math.max(1, g.line), total);
    const pos = view.state.doc.line(line).from;
    view.dispatch({
      selection: { anchor: pos },
      effects: EditorView.scrollIntoView(pos, { y: "center" }),
    });
    view.focus();
  });

  // 大纲：语法树在 CM6 手上，直接从它提取，不另挂一套 parser
  $effect(() => {
    const tick = outlineTick;
    if (!view || tick === 0) return;
    onOutline?.(outlineOf(view.state));
  });

  // 缩略图开关：热替换而不重建 state
  $effect(() => {
    const on = showMinimap;
    if (!view) return;
    view.dispatch({ effects: mapSlot.reconfigure(on ? minimap() : []) });
  });

  // 改动标记：换文件或 git 状态变了都要重下
  $effect(() => {
    const m = marks;
    if (!view) return;
    view.dispatch({ effects: setMinimapMarks.of(m ?? new Map()) });
  });

  // 保存成功：把当前文档定为新基线（不换 state，光标与撤销栈都保住）
  $effect(() => {
    savedTick;
    if (!view || savedTick === 0) return;
    baseline = view.state.doc.toString();
    onChange(false);
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    overflow: hidden;
    background: var(--editor-bg);
  }
  /* CM6 自己管内部 DOM，这里只保证它撑满容器 */
  .editor :global(.cm-editor) { height: 100%; }
  .editor :global(.cm-editor.cm-focused) { outline: none; }
</style>
