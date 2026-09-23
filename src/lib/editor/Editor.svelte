<script lang="ts">
  import { untrack } from "svelte";
  import { EditorState, Compartment } from "@codemirror/state";
  import { EditorView, keymap, lineNumbers, highlightActiveLine,
           highlightActiveLineGutter, drawSelection, rectangularSelection,
           crosshairCursor, highlightSpecialChars } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
  import { searchPanel } from "./search-panel";
  import { autocompletion, closeBrackets, closeBracketsKeymap, completeAnyWord, completionKeymap } from "@codemirror/autocomplete";
  import { bracketMatching, foldGutter, foldKeymap, indentOnInput,
           indentUnit } from "@codemirror/language";
  import { ideaDarkTheme, ideaDarkHighlight } from "./theme-idea-dark";
  import { langOf } from "./langs";
  import { loadLang } from "./langs-load";
  import { outlineOf, symbolCache, type Sym } from "./outline";
  import { minimap } from "./minimap";
  import { changeMarks, setChangeMarks } from "./changemarks";
  import { blameGutter, blameSlot, setBlame } from "./blame";
  import { detectIndent } from "./indent";
  import type { BlameHunk } from "../ipc/commands";
  import { diffLines } from "../git/linediff";
  import { resolveJump, rawWordAt, javaToolsOf, type JumpHit } from "./jump";
  import { jumpExtension, recheckJump } from "./jump-ext";
  import { foldFrontmatter } from "./frontmatter-fold";
  import { bodyPlaceholder } from "./body-placeholder";
  import type { ViewPos } from "../state/docs.svelte";

  let {
    path,
    initial,
    baseline = null,
    savedTick = 0,
    selfSaveTick = 0,
    gotoLine = null,
    placeholder = null,
    onGotoDone,
    outlineTick = 0,
    headText = null,
    blame = null,
    onBlamePick,
    showMinimap = true,
    wrap = false,
    indent = null,
    autofocus = false,
    focusTick = 0,
    onChange,
    onSave,
    onStash,
    onLive,
    onOutline,
    onCursor,
    onCaret,
    initialView = null,
    onView,
    onViewStash,
    jumpFiles = [],
    jumpRel = null,
    jumpLang = "",
    onJump,
    onWordProbe,
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
    /** 只在我们自己写盘成功时自增（`docs.selfSaveTick`）。判据见「换文件」那条 effect */
    selfSaveTick?: number;
    /** 搜索结果跳转用的目标行（1-based）。同一行连点也要能重新定位，故带 nonce */
    gotoLine?: { line: number; col?: number; nonce: number } | null;
    /** `gotoLine` 已经落到位了，上层把它销掉 */
    onGotoDone?: () => void;
    /** 正文（文件头之后）为空时显示的一行灰字，空草稿用它放快捷键提示；敲第一个字就没了 */
    placeholder?: string | null;
    /** 自增即重新提取大纲。放在 Editor 里算是因为语法树在它手上 */
    outlineTick?: number;
    /**
     * 这个文件在 HEAD 里的内容，改动行标记的基线（issue #33 ④）。
     * null = 不在仓库里 / 不在 HEAD 里 / 太大 —— 不标。
     * 标记在**这里**算而不是外面传进来：打字要实时跟着动，只有编辑器手上有实时文本。
     */
    headText?: string | null;
    /** 注解（blame）段落；null = 关着，那列 gutter 整个不装 */
    blame?: BlameHunk[] | null;
    /** 点了某段注解：开那次提交的差异 */
    onBlamePick?: (h: BlameHunk) => void;
    showMinimap?: boolean;
    /** 软换行。笔记要，代码不要；判据在 `state/tab.ts` 的 `wrapsByDefault` */
    wrap?: boolean;
    /** 缩进单位的手动覆盖（状态栏那格，issue #37）。null = 按文件内容猜 */
    indent?: "tab" | number | null;
    /**
     * 挂上就把光标放进去。⌘N、⌘P、双击、系统送进来的文件都是「我要用它」，
     * 光标不在里面就是「按了 ⌘N 打字没反应」—— 这条断过（2026-09-16 才发现）。
     * 单击树 / 搜索命中那种预览**不**抢：焦点在树上，人正用方向键往下走。
     */
    autofocus?: boolean;
    /** 已挂载的编辑器被要求收回焦点（见 `docs.focusEditor`）。挂载时对齐，只响应之后的变化 */
    focusTick?: number;
    /**
     * ⌘Click / ⌘B 跳转要的三样，全从 App 来（见 `lib/editor/jump.ts`）：
     * ⌘P 那份文件索引、当前文件相对项目根的路径、语言 id。
     * 缺任何一样时第二层（import）自动歇菜，只剩本文件那一层 —— 不报错。
     */
    jumpFiles?: string[];
    jumpRel?: string | null;
    jumpLang?: string;
    /** 跳。压导航栈、开标签都是 App 的事，这里只报「往哪儿跳」 */
    onJump?: (hit: JumpHit) => void;
    /**
     * 交出「读出光标底下那个词」的能力，给菜单里那条「在项目里找这个名字」。
     *
     * 契约和 `onLive` 一模一样（挂载时给函数、销毁时给 null，带上自己那份
     * `curPath` 用于认领）—— **另开一条而不是往 `onLive` 上挂**：那条通道
     * 是保存路径上的，出过一次会丢数据的 bug，不值得为一个搜索入口去动它。
     */
    onWordProbe?: (path: string, get: (() => string | null) | null) => void;
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
    /**
     * 把「读出编辑器里此刻的文本」这个能力交给 App。挂载时给一个取值函数，
     * 销毁时给 null。
     *
     * 为什么非要这条通道：`onStash` 只在换文件或销毁时回写一次，
     * 而「保存并关闭」「命令面板里的保存」发生在编辑器**还活着**的时候 ——
     * App 手里的 `draft` 和 `content` 那时都可能停在几步之前。
     * 原来它就是拿 `draft ?? content` 去写盘的：打开文件打几个字直接点
     * 「保存并关闭」，写回去的是**磁盘原文**，界面还说「已保存」。
     *
     * 交函数而不是每次 onChange 都把全文传出去：后者在大文件上等于
     * 每敲一个键复制一份全文。
     */
    onLive?: (path: string, get: (() => string) | null) => void;
    onOutline?: (syms: Sym[]) => void;
    /**
     * 光标换行时报一次（1-based）。会话快照用它记住「上次看到哪」。
     *
     * 只在**行号变了**时才报 —— 同一行里左右移动一个字符也回调的话，
     * 敲一行字就是几十次无谓调用。
     */
    onCursor?: (line: number) => void;
    /**
     * 光标的行:列，**每次选区变都报**（1-based，列按字符数）。给状态栏那一格用。
     * 和 `onCursor` 分开：那个只在换行时报，是给会话快照记位置的，频率要压；
     * 这个是显示，本来就该跟着光标走，而状态栏改一个字符串不值一提。
     */
    onCaret?: (line: number, col: number) => void;
    /**
     * 挂载时把光标和视口摆回去的位置（2026-09-17）。编辑器是 `{#key active.id}` 包着的，
     * 切标签就销毁重建 —— 没有这个，切回来光标在第一行、滚动条在顶上，每切一次就丢一次
     * 「我看到哪儿了」。只有 `line` 的（老快照）把那一行居中；有 `top` 的按原样摆回视口。
     */
    initialView?: ViewPos | null;
    /** 「读此刻的视口」的口子，同 `onLive`：快照要的是此刻，不是上次换行时 */
    onView?: (path: string, get: (() => ViewPos) | null) => void;
    /** 销毁前把视口交回去，同 `onStash`。交的是 `curPath` 那份 */
    onViewStash?: (path: string, pos: ViewPos) => void;
  } = $props();

  /**
   * 此刻的视口。视口顶行用 `lineBlockAtHeight`：CM6 的行高在没量过的地方是估的，
   * 记「顶上是哪一行 + 露出多少像素」比记 scrollTop 稳 —— 恢复时让 CM6 自己把那一行
   * 滚到顶上（它会先量再滚），再补上零头。
   */
  function measureTop(v: EditorView): { top: number; toff: number } {
    const h = v.scrollDOM.getBoundingClientRect().top - v.documentTop;
    const blk = v.lineBlockAtHeight(h);
    return { top: v.state.doc.lineAt(blk.from).number, toff: Math.max(0, Math.round(h - blk.top)) };
  }
  /**
   * 最近一次滚动时量到的视口顶行。销毁那一刻 DOM 可能已经摘下来了（`{#key}` 换块时
   * 先拆后建的次序不归我们管），摘下来之后量出来的全是 0 —— 所以边滚边记，
   * 销毁时 DOM 还在就现量，不在就用这份。选区不在这儿记：它从 state 里读，不碰布局
   */
  let lastTop: { top: number; toff: number } | null = null;
  const onScroll = () => {
    if (view) lastTop = measureTop(view);
  };
  /** 视口变了（滚动、跳转、窗口大小）之后量一次。走 requestMeasure：update() 里不能读布局 */
  function noteTop(v: EditorView) {
    v.requestMeasure({ read: measureTop, write: (t) => (lastTop = t) });
  }
  function viewNow(): ViewPos {
    const v = view!;
    const head = v.state.selection.main.head;
    const ln = v.state.doc.lineAt(head);
    const t = v.dom.isConnected ? measureTop(v) : lastTop;
    return { line: ln.number, col: head - ln.from + 1, ...(t ?? {}) };
  }

  /** 把 `initialView` 摆回去。行列越界就夹到文档范围内 —— 文件在我们不在时可能变短了 */
  function applyView(v: EditorView, p: ViewPos) {
    const doc = v.state.doc;
    const ln = doc.line(Math.min(Math.max(1, p.line), doc.lines));
    const pos = ln.from + Math.min(Math.max(1, p.col ?? 1), ln.length + 1) - 1;
    if (p.top === undefined) {
      v.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "center" }) });
      return;
    }
    const topLine = doc.line(Math.min(Math.max(1, p.top), doc.lines));
    v.dispatch({
      selection: { anchor: pos },
      effects: EditorView.scrollIntoView(topLine.from, { y: "start", yMargin: 0 }),
    });
    // scrollIntoView 在下一帧的测量阶段才真滚；零头等它滚完再补，不然会被它盖掉
    const toff = p.toff ?? 0;
    if (toff > 0) {
      requestAnimationFrame(() => requestAnimationFrame(() => {
        if (view === v) v.scrollDOM.scrollTop += toff;
      }));
    }
  }

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | null = null;
  /** 语言扩展放在 compartment 里，切文件时热替换而不重建整个 state */
  const langSlot = new Compartment();
  /** 补全按语言开关（见 `completionFor`）：换文件时和语言包一起热替换 */
  const completeSlot = new Compartment();

  /**
   * 补全只给代码，不给散文。Markdown / 纯文本 / 没认出语言的文件一律不装 ——
   * 记笔记时每敲一个词弹一个框，是「记两笔」这条路上最败兴的东西
   * （VS Code 也默认对 markdown 关 quickSuggestions）。括号配对不受此限，
   * 写笔记里的 `(` 也该有 `)`。
   */
  function completionFor(lang: ReturnType<typeof langOf>) {
    if (lang === null || lang === "markdown") return [];
    return [autocompletion(), EditorState.languageData.of(() => [{ autocomplete: completeAnyWord }])];
  }
  /** 缩略图同理：开关一下不该把光标和撤销栈也重置掉 */
  const mapSlot = new Compartment();
  const wrapSlot = new Compartment();
  /** 缩进单位同理（issue #37）：状态栏改一下不该把撤销栈也重置掉 */
  const indentSlot = new Compartment();
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
  /** 已经认过的 savedTick。挂载时对齐一次，见「保存成功」那条 effect */
  let seenTick = 0;

  /**
   * 本文件的符号表，按「文档 + 语法树」缓存 —— 实现和为什么见
   * [`symbolCache`]（抽到 jump.ts 里是为了能测：**语言是懒加载的**，
   * 而这个缓存踩过「树还空着就把空表存成结论」那个坑）。
   */
  const symbolsOf = symbolCache();

  const jumpHooks = {
    /*
     * **读的是 prop 的当前值。** 这个闭包在 `build()` 里被创建（那时套着
     * `untrack`），但它真正执行是在 mousemove / mousedown 里 —— 那时读到的
     * 是最新的 `jumpFiles` / `jumpRel`。所以文件索引晚一点才到位也不影响：
     * 到位之前第二层查不到东西，到位之后自动就能跳了。
     */
    resolve: (pos: number) =>
      view
        ? resolveJump(view.state, pos, {
            symbols: symbolsOf(view.state),
            files: jumpFiles,
            rel: jumpRel,
            lang: jumpLang,
            recheck: () => view?.dispatch({ effects: recheckJump.of(null) }),
          })
        : null,
    jump: (hit: JumpHit) => onJump?.(hit),
    settle: () => (view ? (javaToolsOf(view.state, 0)?.settle() ?? null) : null),
  };

  /** `indentUnit` 要的那个字符串：覆盖优先，没有就猜；猜不出按 4 空格 */
  function indentUnitOf(over: "tab" | number | null): string {
    const ind = over ?? detectIndent(baseline ?? initial);
    return ind === "tab" ? "\t" : " ".repeat(typeof ind === "number" ? ind : 4);
  }

  function build(doc: string) {
    return EditorState.create({
      doc,
      extensions: [
        lineNumbers(),
        // 紧挨着行号、在折叠标记左边 —— IDEA / VS Code 都是这个位置。gutter 的
        // 左右顺序就是扩展列表里的顺序
        changeMarks(),
        blameSlot.of([]),
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
        /*
         * 括号 / 引号自动配对 + 补全（2026-09-21）。之前一直没装：打 `(` 不出 `)`、
         * 打 `"` 不出 `"` —— 这是 Sublime 都有的底线。
         *
         * 补全的来源有两层：语言包自己登记的（lang-javascript 的关键字与片段、lang-html
         * 的标签…）走 `autocompletion()` 默认的 languageData；再全局挂一条 `completeAnyWord`，
         * 没有 LSP 的编辑器靠「文档里出现过的词」就够把变量名和方法名补出来 —— 这个
         * 应用立项时就排除了 LSP（PLAN.md），所以这一层就是补全的全部。
         * 用 `EditorState.languageData` 而不是 `override`：override 会把语言包那层挤掉。
         */
        closeBrackets(),
        completeSlot.of(completionFor(langOf(path))),
        highlightSelectionMatches(),
        // 自研的查找 / 替换面板（连同那条一直没接上的 ⌥⌘F）。
        // 它自己包着 `search({ top: true, createPanel })`，别在这儿再装一次 —— 
        // 装两遍的话后一个 `createPanel` 静默不生效，画出来的还是默认面板。
        searchPanel(),
        placeholder ? bodyPlaceholder(placeholder) : [],
        mapSlot.of(showMinimap ? minimap() : []),
        wrapSlot.of(wrap ? EditorView.lineWrapping : []),
        /*
         * 缩进单位（issue #33 ③）按盘上那份内容猜（`editor/indent.ts`）：回车、Tab、
         * 自动缩进都照它来 —— 4 空格的文件里回车缩进出一个 Tab，就是那种「每次保存都
         * 多一片改动」的来源。null = 猜不出，按 4 空格。**在这里算而不是外面传进来**
         * （issue #32）：外面传要在入口包里带上 `indent.ts`，而只有编辑器和状态栏用它，
         * 两个都是有标签之后的事。猜只在建 state 时做一次：文件打开之后风格不会变；
         * 状态栏那格手动改的（`indent` prop，issue #37）走 compartment 热替换。
         */
        indentSlot.of(indentUnit.of(indentUnitOf(indent))),
        langSlot.of([]),
        jumpExtension(jumpHooks),
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
          /*
           * 配对和补全的键在 defaultKeymap 前面：Backspace 删配对的两个字符、
           * 补全弹层里的 ↑↓↵ / Esc 都得先于默认键位吃到，否则 ↵ 变成换行、Esc 落空
           */
          ...closeBracketsKeymap,
          ...completionKeymap,
          ...defaultKeymap,
          ...historyKeymap,
          ...searchKeymap,
          ...foldKeymap,
          indentWithTab,
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) {
            onChange(u.state.doc.toString() !== baseText);
            scheduleMarks();
          }
          if (u.viewportChanged || u.geometryChanged) noteTop(u.view);
          /*
           * 拿到焦点也报一次光标（`u.focusChanged`）：分屏之后状态栏那格行:列跟着焦点组走
           * （issue #35），用键盘切过来时这个编辑器的光标没动，不报的话状态栏还印着
           * 另一边的位置。同一个编辑器失焦时也会进这里，多报一次同样的数无妨
           */
          if (u.selectionSet || u.docChanged || (u.focusChanged && u.view.hasFocus)) {
            const head = u.state.selection.main.head;
            const ln = u.state.doc.lineAt(head);
            onCaret?.(ln.number, head - ln.from + 1);
            if (onCursor && ln.number !== lastLine) {
              lastLine = ln.number;
              onCursor(ln.number);
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

  /*
   * 建视图：只在挂载时做一次。
   *
   * **整个函数体套在 `untrack` 里，这不是可有可无的。** effect 的依赖集是
   * 由「第一次实际读了哪些信号」决定的 —— 不 untrack 的话，`path`、
   * `baseline`、`initial`，以及 `build()` 里那个 `showMinimap`，全都会进去。
   * 于是**每次保存都会重建整个编辑器**：`save()` 把 `tab.content` 换成新的
   * → `baseline` 变 → cleanup 跑 → `view.destroy()`。
   *
   * 表现是保存之后**光标跳回文件开头、撤销栈整个清空**（实测：光标 120 → 0，
   * view 换了一个实例，⌘Z 撤不回刚才那次修改）。而下面「保存成功」那条
   * effect 的注释一直写着「不换 state，光标与撤销栈都保住」——
   * 它自己是对的，是被这里连累的。切缩略图同理。
   *
   * `host` 留在外面：它是唯一该触发重建的东西（`bind:this` 从 undefined
   * 变成真元素那一次）。
   */
  $effect(() => {
    if (!host) return;
    untrack(() => {
      curPath = path;
      baseText = baseline ?? initial;
      view = new EditorView({ state: build(initial), parent: host });
      // 挂载先报一次：updateListener 只在有更新时才跑，不报的话状态栏那格
      // 会停在上一个标签的位置上，直到人动一下光标
      onCaret?.(1, 1);
      // 草稿的锚点头折起来、光标落到正文（M10）。只看 markdown：别的文件的 `---` 开头不归这儿管。
      // 放在 onCaret 之后：它会 dispatch 一次，updateListener 报的才是挪完的位置
      if (/\.(md|markdown)$/i.test(path)) foldFrontmatter(view);
      // 上次离开时的光标和视口。放在折叠之后：折叠会把停在头部的光标挪到正文，别让它盖掉这份
      if (initialView) applyView(view, initialView);
      noteTop(view);
      // 基线多半在挂载前就到了（切标签时上一份还在），那条 effect 那时 view 还是 null
      recomputeMarks();
      applyBlame();
      void applyLang(path);
      // 草稿恢复回来时它本来就是脏的，得说出来 —— 不说的话标签上的圆点不会亮
      onChange(initial !== baseText);
      // 对齐计数器：挂载不是一次「刚保存」，见下面那条 effect 的注释
      seenTick = savedTick;
      seenSelfSave = selfSaveTick; // 同上：挂载不是一次「刚存的落盘了」
      onLive?.(curPath, () => view?.state.doc.toString() ?? "");
      onView?.(curPath, () => viewNow());
      view.scrollDOM.addEventListener("scroll", onScroll, { passive: true });
      onWordProbe?.(curPath, () =>
        view ? rawWordAt(view.state, view.state.selection.main.head) : null,
      );
      seenFocus = focusTick;
      if (autofocus) view.focus();
    });
    return () => {
      stash();
      untrack(() => {
        if (view) {
          view.scrollDOM.removeEventListener("scroll", onScroll);
          onViewStash?.(curPath, viewNow());
        }
        onView?.(curPath, null);
        onLive?.(curPath, null);
        onWordProbe?.(curPath, null);
      });
      view?.destroy();
      view = null;
    };
  });

  let seenSelfSave = 0;
  // 换文件：整份换掉文档，并热替换语言
  $effect(() => {
    const p = path;
    const text = initial;
    const base = baseline;
    const own = selfSaveTick;
    if (!view) return;
    const 换了文件 = p !== curPath;
    /*
     * `initial` 这次变，是不是因为**我们自己刚存的落盘了**？
     *
     * 写盘是 await 的，IPC 往返里人还在打字。落盘后 `settled()` 把 content 换成存下去
     * 的那份，`initial` 跟着变 —— 若照下面「文本变了就换 state」处理，文档会被换回
     * 存下去的那份，往返期间敲的字就没了（实测：打 G、存、写盘中打 H，落盘后 H 消失且
     * 标签不脏）。手动 ⌘S 时人不打字所以没露过；草稿自动保存每次落盘都在打字间隙。
     * 外部重读 / 冲突选「用磁盘上的」不加 selfSaveTick，那两种照旧换文档。
     */
    const 自己存的落盘了 = own !== seenSelfSave;
    seenSelfSave = own;
    // 真换了文件才收草稿；同一个文件只是内容被外部改了（重读），不能当草稿收走
    if (换了文件) {
      stash();
      untrack(() => {
        onLive?.(curPath, null);
        onWordProbe?.(curPath, null);
      });
    }
    curPath = p;
    baseText = base ?? text;
    /*
     * 文本没变就**不能**换 state。
     *
     * 保存成功时 `baseline` 会变（磁盘那份成了新基线），这条 effect 因此
     * 被叫醒 —— 但那时文档本身一个字都没动。照旧 `setState` 的话，
     * 光标回到文件开头、撤销栈整个清空，而这正是保存最不该干的事。
     * （上面那条 mount effect 的 untrack 解决的是「view 被整个重建」，
     * 这里解决的是「view 还在但 state 被换掉」—— 两处都得堵，
     * 只堵一处的表现是一样的：⌘S 之后 ⌘Z 撤不回来。）
     *
     * 先比长度再比内容：外部重读时才真的要换，那时长度多半也不一样。
     */
    const 文本变了 = text.length !== view.state.doc.length || text !== view.state.doc.toString();
    if (换了文件) {
      view.setState(build(text));
      void applyLang(p);
      // 新 state 里注解那个槽是空的，改动标记的字段也是新的：两个都要重下
      blameInstalled = false;
      untrack(() => {
        applyBlame();
        recomputeMarks();
      });
    } else if (文本变了 && !自己存的落盘了) {
      /*
       * 同一个文件被外部改了（构建工具重写、git checkout、格式化器）：**换内容，不换 state**。
       *
       * 原来这里也是 `setState`——光标回到第一行、滚动条回到顶上、撤销栈清空。人正读到
       * 第 3000 行，旁边的 gradle 一跑，视线被拽回文件开头（2026-09-17 体感那轮列的
       * 「光标不在预期的地方」，这是仓库里唯一一处非用户发起的跳动）。
       *
       * 整份替换成一次事务：光标和视口先记下、换完按行列摆回去（越界就夹到文档范围内，
       * `applyView` 本来就会夹）；撤销栈留着 —— ⌘Z 能退回外部改之前那份，
       * 那时标签变脏，和 IDEA 的 local history 一个意思。
       */
      const keep = viewNow();
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
      applyView(view, keep);
    }
    // 脏不脏看**文档**对基线，不看 prop：自己存的落盘那次文档可能已经领先 initial
    onChange(view.state.doc.toString() !== baseText);
    untrack(() => onLive?.(p, () => view?.state.doc.toString() ?? ""));
  });

  async function applyLang(p: string) {
    const lang = langOf(p);
    const ext = await loadLang(lang);
    if (!view) return;
    view.dispatch({ effects: [langSlot.reconfigure(ext ?? []), completeSlot.reconfigure(completionFor(lang))] });
  }

  // 跳到指定行并居中。nonce 变化即触发，所以连点同一条搜索结果也能重新定位。
  // 跳完叫 `onGotoDone` 把指令销掉 —— 理由见 `nav.done`
  $effect(() => {
    const g = gotoLine;
    if (!view || !g) return;
    const total = view.state.doc.lines;
    const line = Math.min(Math.max(1, g.line), total);
    const ln = view.state.doc.line(line);
    // 列越界就停在行尾 —— 「跳到 12:999」的意思是「第 12 行尽头」，不是报错
    const pos = ln.from + Math.min(Math.max(1, g.col ?? 1), ln.length + 1) - 1;
    view.dispatch({
      selection: { anchor: pos },
      effects: EditorView.scrollIntoView(pos, { y: "center" }),
    });
    view.focus();
    onGotoDone?.();
  });

  // 大纲：语法树在 CM6 手上，直接从它提取，不另挂一套 parser
  $effect(() => {
    const tick = outlineTick;
    if (!view || tick === 0) return;
    onOutline?.(outlineOf(view.state));
  });

  /*
   * 注解：开关 = 装 / 拆那列 gutter；数据变了就整份重下。
   * **装和喂数据要分两次 dispatch**：reconfigure 那一笔里新字段只跑 `create`，
   * 同一笔里的 effect 它看不见 —— 合成一笔的表现是 gutter 出来了但一片空白。
   */
  let blameInstalled = false;
  function applyBlame() {
    if (!view) return;
    const b = blame;
    if (b === null) {
      if (blameInstalled) view.dispatch({ effects: blameSlot.reconfigure([]) });
      blameInstalled = false;
      return;
    }
    if (!blameInstalled) {
      view.dispatch({ effects: blameSlot.reconfigure(blameGutter((h) => onBlamePick?.(h))) });
      blameInstalled = true;
    }
    view.dispatch({ effects: setBlame.of(b) });
  }
  $effect(() => {
    blame;
    if (!view) return;
    untrack(applyBlame);
  });

  // 缩略图开关：热替换而不重建 state
  $effect(() => {
    const on = showMinimap;
    if (!view) return;
    view.dispatch({ effects: mapSlot.reconfigure(on ? minimap() : []) });
  });
  let seenFocus = 0;
  $effect(() => {
    const t = focusTick;
    if (!view || t === seenFocus) return;
    seenFocus = t;
    view.focus();
  });
  // 软换行同理
  $effect(() => {
    const on = wrap;
    if (!view) return;
    view.dispatch({ effects: wrapSlot.reconfigure(on ? EditorView.lineWrapping : []) });
  });
  // 缩进单位同理（issue #37）。`baseline` / `initial` 在 untrack 里读：它们变了不该重算
  $effect(() => {
    const over = indent;
    if (!view) return;
    const unit = untrack(() => indentUnitOf(over));
    view.dispatch({ effects: indentSlot.reconfigure(indentUnit.of(unit)) });
  });

  /*
   * 改动标记：基线（HEAD 那份）变了立刻重算；打字则防抖 150ms 再算。
   *
   * 150ms 是「连着打字时不算、停下来立刻见」的分界：比它短，每个字都算一遍
   * （几千行的文件一次 diff 要几毫秒，连打时那是纯浪费）；比它长，停笔之后
   * 色带明显慢半拍。防抖期间旧标记由 `changemarks.ts` 按改动平移，不会错行。
   *
   * `diffLines` 给 null（文件太大 / 改得太多）时清掉 —— 一份标着几万条的
   * 色带没有信息量，还不如没有。
   */
  let marksTimer: ReturnType<typeof setTimeout> | null = null;
  function recomputeMarks() {
    marksTimer = null;
    if (!view) return;
    const base = headText;
    const m = base === null ? null : diffLines(base, view.state.doc.toString());
    view.dispatch({ effects: setChangeMarks.of(m ?? new Map()) });
  }
  function scheduleMarks() {
    if (untrack(() => headText) === null) return;
    if (marksTimer !== null) clearTimeout(marksTimer);
    marksTimer = setTimeout(recomputeMarks, 150);
  }
  $effect(() => {
    headText;
    if (!view) return;
    if (marksTimer !== null) clearTimeout(marksTimer);
    recomputeMarks();
    return () => {
      if (marksTimer !== null) clearTimeout(marksTimer);
      marksTimer = null;
    };
  });

  /*
   * 保存成功：把当前文档定为新基线（不换 state，光标与撤销栈都保住）。
   *
   * **必须跟上次见到的值比，不能只判非零。** 组件是 `{#key active.id}` 包着的，
   * 切标签就是一个全新实例，而 `savedTick` 是 App 上的累计值 ——
   * 只要这个会话里保存过一次，新实例挂载时它就已经非零，这条 effect
   * 于是当场跑一次 `onChange(false)`，把刚从草稿恢复出来的「有未保存改动」
   * 抹掉。表现和 M25 修的那个 bug 一模一样：**字还在，标签上的圆点没了** ——
   * 而圆点没了，⌘W 就不会拦你。
   *
   * 另外写的是 `baseText` 而不是 `baseline` 那个 prop：改 prop 会让上面
   * 两条读它的 effect 跟着醒来，绕一圈回来又是一次 setState。
   */
  $effect(() => {
    const t = savedTick;
    if (!view || t === seenTick) return;
    seenTick = t;
    /*
     * 基线取 `baseline`（`settled()` 写回的、真正落在盘上的那份），**不是此刻的文档**。
     * 原来写的是 `view.state.doc.toString()`：写盘是 await 的，IPC 往返里人还在打字，
     * 回来时把含新字的文档定成基线、报 onChange(false) —— 那几个字盘上没有、标签却不脏，
     * 自动保存不再触发、⌘W 不问、退出快照也不 stash，就这么丢了。手动 ⌘S 时人一般
     * 不打字所以没露过，草稿自动保存（issue #40）每次落盘都在打字间隙，露了。
     * `untrack`：这条 effect 只该被 savedTick 叫醒，不该被 baseline 本身叫醒
     * （它俩在保存时同一拍变，外部重读时也一起变）。
     */
    baseText = untrack(() => baseline ?? initial);
    onChange(view.state.doc.toString() !== baseText);
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  /* 内容层在这儿画，且**只在这儿画一次** —— CM6 主题里的 BG 已经改成
     transparent，见 theme-idea-dark.ts 顶上那段 */
  .editor {
    height: 100%;
    overflow: hidden;
    background: var(--content-bg);
  }
  /* CM6 自己管内部 DOM，这里只保证它撑满容器 */
  .editor :global(.cm-editor) { height: 100%; }
  .editor :global(.cm-editor.cm-focused) { outline: none; }
</style>
