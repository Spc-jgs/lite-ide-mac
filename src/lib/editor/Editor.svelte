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

  let {
    path,
    initial,
    savedTick = 0,
    gotoLine = null,
    onChange,
    onSave,
  }: {
    path: string;
    initial: string;
    /** 每次保存成功后自增。用它重置 dirty 基线，比暴露组件 ref 耦合更松 */
    savedTick?: number;
    /** 搜索结果跳转用的目标行（1-based）。同一行连点也要能重新定位，故带 nonce */
    gotoLine?: { line: number; nonce: number } | null;
    onChange: (dirty: boolean) => void;
    onSave: (content: string) => void;
  } = $props();

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | null = null;
  /** 语言扩展放在 compartment 里，切文件时热替换而不重建整个 state */
  const langSlot = new Compartment();
  /** dirty 判定的基线：当前磁盘上的内容。挂载与换文件时更新，不在顶层读 prop */
  let baseline = "";

  function build(doc: string) {
    return EditorState.create({
      doc,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        highlightSpecialChars(),
        drawSelection(),
        rectangularSelection(),
        crosshairCursor(),
        history(),
        foldGutter(),
        indentOnInput(),
        bracketMatching(),
        highlightSelectionMatches(),
        search({ top: true }),
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
          if (u.docChanged) onChange(u.state.doc.toString() !== baseline);
        }),
      ],
    });
  }

  // 建视图：只在挂载时做一次
  $effect(() => {
    if (!host || view) return;
    baseline = initial;
    view = new EditorView({ state: build(initial), parent: host });
    void applyLang(path);
    return () => {
      view?.destroy();
      view = null;
    };
  });

  // 换文件：整份换掉文档，并热替换语言
  $effect(() => {
    const p = path;
    const text = initial;
    if (!view) return;
    baseline = text;
    view.setState(build(text));
    void applyLang(p);
    onChange(false);
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
