/**
 * IDEA Dark 的 CodeMirror 6 主题。
 *
 * 分两层：`ideaDarkTheme` 管编辑器外壳（背景、光标、选区、行号栏），
 * `ideaDarkHighlight` 管语法着色。色值取自 IntelliJ IDEA 新 UI 的 Darcula，
 * 与 app.css 里的 token 同源，两边改色要一起改。
 */

import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

const BG = "#1e1f22";
const GUTTER_FG = "#4b5059";
const GUTTER_FG_ACTIVE = "#a1a3ab";
const TEXT = "#dfe1e5";
const CARET = "#cdd0d5";
const SELECTION = "#214283";
const ACTIVE_LINE = "#26282e";
const MATCH = "rgba(53, 116, 240, 0.42)";

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
      backgroundColor: BG,
      color: GUTTER_FG,
      border: "none",
      borderRight: "1px solid rgba(255,255,255,.06)",
    },
    ".cm-activeLineGutter": { backgroundColor: ACTIVE_LINE, color: GUTTER_FG_ACTIVE },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 16px" },
    ".cm-foldPlaceholder": {
      backgroundColor: "#35373b",
      border: "none",
      color: "#9da0a8",
      padding: "0 6px",
      borderRadius: "3px",
    },
    ".cm-searchMatch": { backgroundColor: MATCH, outline: "none" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "#3574f0" },
    ".cm-selectionMatch": { backgroundColor: "rgba(53,116,240,.18)" },
    ".cm-matchingBracket, .cm-nonmatchingBracket": {
      backgroundColor: "rgba(60,115,75,.45)",
      outline: "none",
    },
    ".cm-panels": { backgroundColor: "#2b2d30", color: TEXT, borderColor: "#393b40" },
    ".cm-panels input, .cm-panels button": {
      backgroundColor: BG,
      color: TEXT,
      border: "1px solid #393b40",
      borderRadius: "3px",
      padding: "2px 6px",
      fontFamily: "var(--code-font)",
    },
    ".cm-tooltip": { backgroundColor: "#2b2d30", border: "1px solid #393b40", color: TEXT },
  },
  { dark: true },
);

export const ideaDarkHighlight = syntaxHighlighting(
  HighlightStyle.define([
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
  ]),
);
