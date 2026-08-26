/**
 * 文件结构大纲 —— 从 CodeMirror 已有的语法树里提取符号。
 *
 * 刻意**不引入 tree-sitter**：CM6 为了高亮本来就把文档解析好了，
 * 语法树就在那儿，再挂一套 WASM parser 等于同一份代码解析两遍，
 * 而每种语言的 tree-sitter wasm 有几百 KB —— 与"秒开"的立身之本冲突。
 *
 * 代价是只覆盖有 Lezer parser 的语言（Java / JS / TS / Python / Rust /
 * C++ / PHP / CSS / HTML / Markdown 等）。走 legacy stream parser 的语言
 * 没有语法树，大纲为空 —— 界面会明说，不假装。
 */

import { syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import type { SyntaxNode } from "@lezer/common";

export interface Sym {
  name: string;
  /** 显示给用户的类别 */
  kind: string;
  /** 1-based 行号 */
  line: number;
  /** 嵌套深度，用于缩进 */
  depth: number;
}

/**
 * 感兴趣的节点类型 → 类别名。
 * 各语言的 Lezer 语法节点名不统一，这里按名字匹配，覆盖主流语言。
 */
const KINDS: Record<string, string> = {
  // Java
  ClassDeclaration: "类",
  InterfaceDeclaration: "接口",
  EnumDeclaration: "枚举",
  RecordDeclaration: "记录",
  MethodDeclaration: "方法",
  ConstructorDeclaration: "构造",
  FieldDeclaration: "字段",
  AnnotationTypeDeclaration: "注解",
  // JavaScript / TypeScript
  FunctionDeclaration: "函数",
  MethodDefinition: "方法",
  PropertyDefinition: "属性",
  ClassExpression: "类",
  InterfaceDeclaration_TS: "接口",
  TypeAliasDeclaration: "类型",
  EnumDeclaration_TS: "枚举",
  // Python
  FunctionDefinition: "函数",
  ClassDefinition: "类",
  // Rust
  FunctionItem: "fn",
  StructItem: "struct",
  EnumItem: "enum",
  TraitItem: "trait",
  ImplItem: "impl",
  ModItem: "mod",
  TypeItem: "type",
  MacroItem: "macro",
  // C / C++
  FunctionDefinition_C: "函数",
  StructSpecifier: "struct",
  ClassSpecifier: "类",
  // PHP
  FunctionDefinition_PHP: "函数",
  // CSS
  RuleSet: "规则",
  // Markdown 标题单独处理
};

const HEADING = /^ATXHeading([1-6])$/;

/**
 * 可能是"名字"的子节点类型，**按优先级排列**。
 *
 * 顺序很重要：Java 的 `private final OrderRepository repo;` 里，
 * `TypeName`(OrderRepository) 和 `Definition`(repo) 都在，
 * 按集合匹配会先撞上类型名 —— 大纲里就会显示 `OrderRepository` 而不是 `repo`。
 * 方法同理：`public Order persist(...)` 会显示成 `Order`。
 * 所以声明名（Definition 系）必须排在类型名前面。
 */
const NAME_PRIORITY = [
  "Definition",
  "VariableDefinition",
  "PropertyDefinition",
  "BoundIdentifier",
  "FieldIdentifier",
  "VariableName",
  "PropertyName",
  "Identifier",
  "TypeIdentifier",
  "TypeName",
];

/** 从节点里找出符号名 */
function nameOf(state: EditorState, node: SyntaxNode): string | null {
  // 按优先级找：先要声明名，再退而求其次
  for (const want of NAME_PRIORITY) {
    for (let c = node.firstChild; c; c = c.nextSibling) {
      if (c.name !== want) continue;
      const t = state.doc.sliceString(c.from, c.to).trim();
      if (t) return t;
    }
    // 名字也可能包在 VariableDeclarator / Declarator 这层里
    for (let c = node.firstChild; c; c = c.nextSibling) {
      if (!c.name.endsWith("Declarator")) continue;
      for (let g = c.firstChild; g; g = g.nextSibling) {
        if (g.name !== want) continue;
        const t = state.doc.sliceString(g.from, g.to).trim();
        if (t) return t;
      }
    }
  }
  // 找不到就从声明头部按启发式抠一个：取 `(` 或 `{` 之前的最后一个标识符
  const head = state.doc.sliceString(node.from, Math.min(node.to, node.from + 160));
  const cut = head.search(/[({<=]/);
  const seg = cut > 0 ? head.slice(0, cut) : head;
  const ids = seg.match(/[A-Za-z_$][\w$]*/g);
  if (!ids || ids.length === 0) return null;
  return ids[ids.length - 1];
}

/** 提取当前文档的符号大纲 */
export function outlineOf(state: EditorState): Sym[] {
  const out: Sym[] = [];
  const tree = syntaxTree(state);
  // 文档还没解析完（超大文件）时不硬扛，返回空由界面提示
  if (tree.length < state.doc.length / 2 && state.doc.length > 200_000) return out;

  const stack: number[] = [];
  tree.iterate({
    enter: (node) => {
      // Markdown：标题层级天然就是大纲
      const h = HEADING.exec(node.name);
      if (h) {
        const line = state.doc.lineAt(node.from);
        const text = line.text.replace(/^#+\s*/, "").trim();
        if (text) out.push({ name: text, kind: `H${h[1]}`, line: line.number, depth: Number(h[1]) - 1 });
        return;
      }

      const kind = KINDS[node.name];
      if (!kind) return;
      // iterate 给的是 SyntaxNodeRef，.node 才是能遍历子节点的完整节点
      const name = nameOf(state, node.node);
      if (!name) return;

      // 用节点起点估算嵌套深度：栈里比当前节点晚结束的都是祖先
      while (stack.length && stack[stack.length - 1] <= node.from) stack.pop();
      const depth = stack.length;
      stack.push(node.to);

      out.push({ name, kind, line: state.doc.lineAt(node.from).number, depth });
    },
  });
  return out;
}
