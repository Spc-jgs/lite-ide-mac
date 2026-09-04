/**
 * 编辑器的 CodeMirror 6 主题。
 *
 * 分两层：`ideaDarkTheme` 管编辑器外壳（背景、光标、选区、行号栏），
 * `ideaDarkHighlight` 管语法着色。
 *
 * # 外壳色值全部走 CSS 变量，这一层不该出现 `#`
 *
 * 原来这个文件把 app.css 的色值又抄了一遍，文件头写着「两边改色要一起改」——
 * 一条靠人记住的规矩。换皮肤时它就是第一个走样的地方：外壳变了、
 * 编辑器还是老底色，两块背景差几个色阶，看着像没加载完。
 *
 * CM6 的 theme 只是生成一张样式表，`var(--x)` 原样写进去就能用。
 * 于是调色板只有 app.css 一处，这个文件只负责"哪个部件用哪个 token"。
 *
 * 语法着色（下面的 HIGHLIGHT_SPEC）**仍然是字面色值**，这是有意的：
 * 那是一整套配色方案，几十个色互相之间才有意义，拆成几十个
 * CSS 变量既没人看得懂也没人会去调。它换的时候是整套换。
 */

import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t, tagHighlighter, type Tag } from "@lezer/highlight";

/*
 * 内容层由 `Editor.svelte` 的 `.editor` 容器画，**这里不再画一遍** ——
 * 两层半透明叠起来会把 6% 透光压成 0.36%，于是编辑器比旁边的日志视图
 * 明显"更实"，两块内容区的色调对不上。
 */
const BG = "transparent";
/* 行号栏是吸住的，正文从它底下滚过去 —— 这里必须挡光 */
const GUTTER_BG = "var(--content-solid)";
const GUTTER_FG = "var(--gutter-fg)";
const GUTTER_FG_ACTIVE = "var(--gutter-fg-active)";
const TEXT = "var(--text)";
const CARET = "var(--caret)";
const SELECTION = "var(--editor-sel)";
const ACTIVE_LINE = "var(--editor-active-line)";
const MATCH = "var(--search-hit)";

export const ideaDarkTheme = EditorView.theme(
  {
    "&": {
      color: TEXT,
      backgroundColor: BG,
      height: "100%",
      fontSize: "13px",
    },
    ".cm-scroller": {
      fontFamily: "var(--code-font)",
      lineHeight: "1.55",
      overflow: "auto",
    },
    ".cm-content": { caretColor: CARET, padding: "6px 0" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: CARET, borderLeftWidth: "2px" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: SELECTION,
    },
    ".cm-activeLine": { backgroundColor: ACTIVE_LINE },
    ".cm-gutters": {
      backgroundColor: GUTTER_BG,
      color: GUTTER_FG,
      border: "none",
      borderRight: "1px solid var(--border-soft)",
    },
    ".cm-activeLineGutter": { backgroundColor: ACTIVE_LINE, color: GUTTER_FG_ACTIVE },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 16px" },
    ".cm-foldPlaceholder": {
      backgroundColor: "var(--selected)",
      border: "none",
      color: "var(--text-dim)",
      padding: "0 6px",
      borderRadius: "var(--r-sm)",
    },
    ".cm-searchMatch": { backgroundColor: MATCH, outline: "none" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--accent)" },
    ".cm-selectionMatch": { backgroundColor: "var(--selection-match)" },
    ".cm-matchingBracket, .cm-nonmatchingBracket": {
      backgroundColor: "var(--bracket-match)",
      outline: "none",
    },
    ".cm-panels": { backgroundColor: "var(--elevated)", color: TEXT, borderColor: "var(--border)" },
    ".cm-panels input, .cm-panels button": {
      backgroundColor: "var(--elevated-hi)",
      color: TEXT,
      border: "1px solid var(--border)",
      borderRadius: "var(--r-sm)",
      padding: "2px 6px",
      fontFamily: "var(--code-font)",
    },
    ".cm-tooltip": {
      backgroundColor: "var(--elevated)",
      border: "1px solid var(--border)",
      color: TEXT,
      boxShadow: "var(--shadow-pop)",
    },
  },
  { dark: true },
);

/**
 * 语法着色的唯一真源。
 *
 * 抽成独立的表是因为**缩略图要用同一套颜色**：`HighlightStyle` 产出的是
 * CSS 类名，canvas 上没法用。让两边各写一份色值，迟早会走样。
 * 这里定义一次，编辑器走 `HighlightStyle`，缩略图走 `tagHighlighter`
 * ——后者能把标签映射成任意字符串，正好用来直接映射成色值。
 */
export const HIGHLIGHT_SPEC: { tag: Tag | Tag[]; color: string; fontStyle?: string; fontWeight?: string; textDecoration?: string }[] = [
    // 关键字系：IDEA 里那个标志性的暖橙
    { tag: [t.keyword, t.moduleKeyword, t.controlKeyword, t.operatorKeyword], color: "#cf8e6d" },
    { tag: [t.self, t.null, t.bool, t.atom], color: "#cf8e6d" },

    { tag: [t.string, t.special(t.string), t.regexp], color: "#6aab73" },
    { tag: [t.escape], color: "#cf8e6d" },
    { tag: [t.number, t.integer, t.float], color: "#2aacb8" },

    { tag: [t.comment, t.blockComment, t.lineComment], color: "#7a7e85", fontStyle: "italic" },
    { tag: [t.docComment], color: "#5f826b", fontStyle: "italic" },

    { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "#56a8f5" },
    { tag: [t.definition(t.function(t.variableName))], color: "#56a8f5" },
    { tag: [t.propertyName], color: "#c77dbb" },
    { tag: [t.variableName, t.definition(t.variableName)], color: TEXT },
    { tag: [t.constant(t.variableName), t.standard(t.variableName)], color: "#c77dbb" },

    { tag: [t.typeName, t.className, t.namespace], color: TEXT },
    /*
     * XML / HTML 的标签名。**只需要这一条**。
     *
     * `@lezer/highlight` 里这几个 tag 都是有父 tag 的：
     * `attributeName→propertyName`、`attributeValue→string`、
     * `angleBracket→bracket`、`processingInstruction→meta` ——
     * 上面那些规则已经把它们覆盖到了，实测 pom.xml 里属性名是粉的、
     * 属性值是绿的、尖括号是灰蓝的。唯独 `tagName→typeName` 撞上了
     * 「IDEA 里类名就是正文色」这条，于是元素名跟正文一个色，
     * 看起来像是「没上色」。这里把它单独拎出来给暗黄。
     *
     * （`tagName` 比 `typeName` 更具体，HighlightStyle 取最具体的那条，
     * 所以只会影响 XML/HTML/JSX，不会动 Java 的类名。）
     */
    { tag: [t.tagName], color: "#e8bf6a" },
    { tag: [t.annotation, t.meta], color: "#b3ae60" },
    { tag: [t.operator, t.punctuation, t.bracket, t.separator], color: "#a9b7c6" },
    { tag: [t.invalid], color: "#f75464" },

    // Markdown
    { tag: [t.heading], color: "#56a8f5", fontWeight: "600" },
    { tag: [t.strong], color: TEXT, fontWeight: "700" },
    { tag: [t.emphasis], color: TEXT, fontStyle: "italic" },
    { tag: [t.link, t.url], color: "#548af7", textDecoration: "underline" },
    { tag: [t.monospace], color: "#6aab73" },
    { tag: [t.quote], color: "#7a7e85", fontStyle: "italic" },
    { tag: [t.list], color: "#cf8e6d" },
];

export const ideaDarkHighlight = syntaxHighlighting(HighlightStyle.define(HIGHLIGHT_SPEC));

/**
 * 给缩略图用的高亮器：把语法标签直接映射成色值字符串。
 *
 * `tagHighlighter` 的第二个字段名叫 `class`，但它对返回值不作任何解释 ——
 * 传什么字符串就回传什么。canvas 需要的是色值，正好。
 */
export const minimapHighlighter = tagHighlighter(
  HIGHLIGHT_SPEC.map((r) => ({ tag: r.tag, class: r.color })),
);

/** 没有语法信息时的兜底色（未解析区域、纯文本文件） */
export const MINIMAP_DEFAULT = "#8b8f97";
