/**
 * Java 的单文件符号表：一个名字在这儿指的是局部变量、字段，还是认不出来。
 *
 * # 为什么要有
 *
 * IDEA 把字段的**使用处**也画成紫色（static 的斜体）：`repo.save()` 里的 `repo`、
 * `this.repo`、`order.id` 里的 `id`、循环里的 `MAX_RETRY`。光看语法树做不到 ——
 * 裸的 `repo` 是字段还是同名局部变量要看作用域；`order.id` 里 `order` 和 `id` 在树上
 * 都是 `FieldAccess/Identifier`。这里建一张按作用域嵌套的表回答它。
 *
 * 同一张表第 3 步还要喂 ⌘Click 的成员跳转（「`repo` 声明的类型是什么」），
 * 所以表里顺手记了每个名字的**声明类型**。
 *
 * # 只看本文件、只认写出来的
 *
 * 这是 IDEA 的 PSI 缩一圈：不跨文件、不推类型。代价是有些该紫的不紫（父类继承的字段、
 * Lombok 生成的 `log`）；换来的是不用常驻进程、不用建全局索引。
 *
 * **判不准的一律不上色 —— 宁可漏，不可错。** 错一次紫色（把局部变量画成字段），
 * 人就再也不信这个颜色了；漏一次只是和改之前一样。下面每个「保守」都是这条。
 *
 * # 作用域
 *
 * 往上走祖先：碰到作用域节点（块、方法、lambda、for、catch、try-with-resources），
 * 看它**自己**声明的局部变量（不钻进嵌套的作用域 —— 里层块的变量漏不到外层）；
 * 碰到类体，看它的字段。先碰到谁算谁，这正是 Java 的遮蔽规则（局部遮字段、
 * 匿名类的字段遮外层方法的局部）。
 *
 * 声明在使用处**之后**的同名局部变量也算遮住 —— 按位置判要处理「先用后声明」的非法代码，
 * 算遮住只会少画，保守。
 *
 * # 语法包不认的写法（`@lezer/java` 1.1.3）
 *
 * - `o instanceof Foo x`：`x` 是错误节点 ⚠，不是 `Definition`。它的作用域是**流敏感**的
 *   （`if (!(o instanceof Foo x)) return; use(x);` 里 x 在 if 之后才可用），
 *   所以整个方法里出现过的错误节点文字，都当成这个方法的局部变量 —— 最保守的那个答案。
 * - `case RED -> ...`：解析成 lambda，`RED` 成了 lambda 参数。只会少画，不管。
 * - `record Pt(int x)`：解析成方法，分量成了参数。record 体里用分量不会紫，不管。
 */
import type { SyntaxNode, Tree } from "@lezer/common";
import { javaLanguage } from "@codemirror/lang-java";

export type MarkKind = "fnDecl" | "field" | "staticField";

export interface Mark {
  from: number;
  to: number;
  kind: MarkKind;
}

type Text = (from: number, to: number) => string;

export interface FieldInfo {
  static: boolean;
  /** 声明（`Definition`）的起点 —— 跳转落到这里 */
  pos: number;
  /** 声明类型的简单名（`OrderRepository`、`List<Item>` 取 `List`）；基本类型 / 数组 / var 为 null */
  type: string | null;
}

export interface LocalInfo {
  type: string | null;
  pos: number;
}

/** 一个名字解析的结果。`null` = 本文件里认不出来 */
export type Resolved = ({ kind: "local" } & LocalInfo) | { kind: "field"; info: FieldInfo; body: SyntaxNode };

const CLASS_BODIES = new Set(["ClassBody", "EnumBody", "InterfaceBody", "AnnotationTypeBody"]);
/** 局部变量的作用域。`Block` 也包括方法体 / 初始化块 */
const SCOPES = new Set([
  "Block",
  "ConstructorBody",
  "MethodDeclaration",
  "ConstructorDeclaration",
  "LambdaExpression",
  "ForStatement",
  "EnhancedForStatement",
  "CatchClause",
  "TryWithResourcesStatement",
  "SwitchBlock",
]);
/** 这些节点下的 `Definition` 是类型 / 方法的名字，不是变量 */
const NOT_VARIABLE = new Set([
  "MethodDeclaration",
  "ConstructorDeclaration",
  "ClassDeclaration",
  "InterfaceDeclaration",
  "EnumDeclaration",
  "AnnotationTypeDeclaration",
  "TypeParameter",
  "EnumConstant",
]);
/** 碰到它们就算到了「方法」这一层：模式变量（错误节点）按整个方法算 */
const METHOD_LIKE = new Set(["MethodDeclaration", "ConstructorDeclaration", "LambdaExpression", "StaticInitializer"]);

/** 大写开头、但不是全大写常量 —— 当类型名看（`List`、`Entry`、单字母 `T`） */
function looksLikeType(s: string): boolean {
  return /^[A-Z]/.test(s) && !/^[A-Z][A-Z0-9_]+$/.test(s);
}

/** 从类型节点取简单名：`TypeName` 本身、`GenericType` 的第一个 `TypeName`、`ScopedTypeName` 的最后一段 */
function simpleType(node: SyntaxNode | null, text: Text): string | null {
  if (!node) return null;
  if (node.name === "TypeName") {
    const s = text(node.from, node.to);
    return s === "var" ? null : s;
  }
  if (node.name === "GenericType") return simpleType(node.getChild("TypeName") ?? node.getChild("ScopedTypeName"), text);
  if (node.name === "ScopedTypeName") return simpleType(node.lastChild, text);
  return null; // PrimitiveType / ArrayType：没有成员可跳，也不需要
}

const TYPE_NODES = new Set(["TypeName", "GenericType", "ScopedTypeName", "ArrayType", "PrimitiveType"]);

/** 一个变量 `Definition` 的声明类型 */
function declType(def: SyntaxNode, text: Text): string | null {
  const p = def.parent;
  if (!p) return null;
  if (p.name === "VariableDeclarator") {
    // `int a, b[]` —— 类型挂在声明上，不在 declarator 上
    for (let c = p.parent?.firstChild; c; c = c.nextSibling) if (TYPE_NODES.has(c.name)) return simpleType(c, text);
    return null;
  }
  // FormalParameter / Resource / ForSpec / CatchFormalParameter：类型就在名字前面
  const prev = def.prevSibling;
  return prev && TYPE_NODES.has(prev.name) ? simpleType(prev, text) : null;
}

function isStaticDecl(decl: SyntaxNode): boolean {
  if (decl.name === "ConstantDeclaration") return true; // 接口里的常量天生 static final
  for (let c = decl.getChild("Modifiers")?.firstChild; c; c = c.nextSibling) if (c.name === "static") return true;
  return false;
}

export class JavaModel {
  private fieldCache = new Map<number, Map<string, FieldInfo>>();
  private localCache = new Map<number, Map<string, LocalInfo>>();
  // 不写成 `constructor(private text: Text)`：那是要生成代码的 TS 语法，node 的原生剥类型不支持，测试跑不了
  private text: Text;
  constructor(text: Text) {
    this.text = text;
  }

  /** 一个类体里声明的字段（枚举常量算 static 字段） */
  fieldsOf(body: SyntaxNode): Map<string, FieldInfo> {
    const hit = this.fieldCache.get(body.from);
    if (hit) return hit;
    const out = new Map<string, FieldInfo>();
    const visit = (parent: SyntaxNode) => {
      for (let c = parent.firstChild; c; c = c.nextSibling) {
        if (c.name === "FieldDeclaration" || c.name === "ConstantDeclaration") {
          const st = isStaticDecl(c);
          for (const vd of c.getChildren("VariableDeclarator")) {
            const def = vd.getChild("Definition");
            if (def) out.set(this.text(def.from, def.to), { static: st, type: declType(def, this.text), pos: def.from });
          }
        } else if (c.name === "EnumConstant") {
          const def = c.getChild("Definition");
          if (def) out.set(this.text(def.from, def.to), { static: true, type: null, pos: def.from });
        } else if (c.name === "EnumBodyDeclarations") {
          visit(c);
        }
      }
    };
    visit(body);
    this.fieldCache.set(body.from, out);
    return out;
  }

  /** 一个作用域**自己**声明的局部变量（名字 → 声明类型），不含嵌套作用域里的 */
  localsOf(scope: SyntaxNode): Map<string, LocalInfo> {
    const hit = this.localCache.get(scope.from * 64 + scope.type.id);
    if (hit) return hit;
    const out = new Map<string, LocalInfo>();
    const walk = (n: SyntaxNode) => {
      for (let c = n.firstChild; c; c = c.nextSibling) {
        if (SCOPES.has(c.name) || CLASS_BODIES.has(c.name)) continue;
        if (c.name === "Definition" && !NOT_VARIABLE.has(n.name)) {
          out.set(this.text(c.from, c.to), { type: declType(c, this.text), pos: c.from });
        } else walk(c);
      }
    };
    walk(scope);
    if (METHOD_LIKE.has(scope.name) || (scope.name === "Block" && scope.parent && CLASS_BODIES.has(scope.parent.name))) {
      this.patternVars(scope, out);
    }
    this.localCache.set(scope.from * 64 + scope.type.id, out);
    return out;
  }

  /** 整个方法里的错误节点文字（多半是 instanceof 的模式变量）—— 见文件头 */
  private patternVars(root: SyntaxNode, out: Map<string, LocalInfo>) {
    const walk = (n: SyntaxNode) => {
      for (let c = n.firstChild; c; c = c.nextSibling) {
        if (CLASS_BODIES.has(c.name)) continue;
        if (c.type.isError) {
          const s = this.text(c.from, c.to).trim();
          if (/^[A-Za-z_$][\w$]*$/.test(s) && !out.has(s)) out.set(s, { type: null, pos: c.from });
        }
        walk(c);
      }
    };
    walk(root);
  }

  /** 裸名字 `name`（出现在 `at` 这个节点）指向什么 */
  resolve(at: SyntaxNode, name: string): Resolved | null {
    for (let a = at.parent; a; a = a.parent) {
      if (SCOPES.has(a.name) || a.name === "StaticInitializer") {
        const l = this.localsOf(a);
        const hit = l.get(name);
        if (hit) return { kind: "local", ...hit };
      } else if (CLASS_BODIES.has(a.name)) {
        const f = this.fieldsOf(a).get(name);
        if (f) return { kind: "field", info: f, body: a };
      }
    }
    return null;
  }

  /** 离 `at` 最近的类体 */
  enclosingBody(at: SyntaxNode): SyntaxNode | null {
    for (let a = at.parent; a; a = a.parent) if (CLASS_BODIES.has(a.name)) return a;
    return null;
  }

  /**
   * `q.m` 里的 `q` 是什么。决定 `m` 怎么上色：
   * - `var`：一个值（局部 / 字段 / this）→ `m` 是实例字段
   * - `type`：一个类型 → `m` 要么是嵌套类型、要么是 static 字段（语言规定：用类型名访问的字段一定是 static）
   * - `pkg`：包名的一段 → `m` 是包或类型，不上色
   * - `expr`：别的表达式（方法调用结果、数组元素……）→ `m` 是实例字段
   */
  classify(q: SyntaxNode): "var" | "type" | "pkg" | "expr" {
    if (q.name === "this") return "var";
    if (q.name === "Identifier") {
      const s = this.text(q.from, q.to);
      if (this.resolve(q, s)) return "var";
      // 认不出来的小写名字：多半是包名（`java.util`），也可能是继承来的字段 —— 都不上色
      return /^[A-Z]/.test(s) ? "type" : "pkg";
    }
    if (q.name === "FieldAccess") {
      const inner = q.firstChild;
      const last = q.lastChild;
      if (!inner || !last) return "expr";
      if (last.name === "this") return "var"; // Outer.this
      const c = this.classify(inner);
      const s = this.text(last.from, last.to);
      if (c === "pkg") return looksLikeType(s) ? "type" : "pkg";
      if (c === "type") return looksLikeType(s) ? "type" : "var";
      return "var";
    }
    return "expr";
  }
}

const models = new WeakMap<Tree, JavaModel>();
/** 同一棵树（= 同一份文档）只建一次表；滚动不重建 */
export function modelFor(tree: Tree, text: Text): JavaModel {
  let m = models.get(tree);
  if (!m) models.set(tree, (m = new JavaModel(text)));
  return m;
}

// ───────────────────────── 上色 ─────────────────────────

/** 这些父节点下的 Identifier 不是「表达式里的一个名字」 */
const NOT_A_NAME = new Set([
  "MethodName",
  "MethodReference",
  "ScopedIdentifier",
  "MarkerAnnotation",
  "Annotation",
  "ElementValuePair",
  "AnnotationTypeDeclaration",
]);

function declKind(def: SyntaxNode, text: Text): MarkKind | null {
  const p = def.parent;
  if (!p) return null;
  switch (p.name) {
    case "MethodDeclaration": {
      // `record Pt(int x) {}`：@lezer/java 1.1.3 不认 record，解析成返回类型叫 record 的方法。
      // 它是类名，不该是方法蓝（Java 14 起 record 不能当类型名，所以这么判不会误伤）
      const prev = def.prevSibling;
      if (prev?.name === "TypeName" && text(prev.from, prev.to) === "record") return null;
      return "fnDecl";
    }
    case "ConstructorDeclaration":
      return "fnDecl";
    case "EnumConstant":
      return "staticField";
    case "VariableDeclarator": {
      const decl = p.parent;
      if (decl?.name === "FieldDeclaration" || decl?.name === "ConstantDeclaration") {
        return isStaticDecl(decl) ? "staticField" : "field";
      }
      return null; // 局部变量 / for 里的，IDEA 不上色
    }
  }
  return null;
}

/** `q.m` 里的 `m` 该上什么色 */
function memberKind(m: JavaModel, fa: SyntaxNode, name: string): MarkKind | null {
  const q = fa.firstChild;
  if (!q) return null;
  const c = m.classify(q);
  if (c === "pkg") return null;
  if (c === "type") return looksLikeType(name) ? null : "staticField";
  // this.x：查得到就知道是不是 static；查不到（继承来的）按实例字段
  if (q.name === "this" || (q.name === "FieldAccess" && q.lastChild?.name === "this")) {
    const body = m.enclosingBody(fa);
    const f = body && m.fieldsOf(body).get(name);
    if (f?.static) return "staticField";
  }
  return "field";
}

function nameKind(m: JavaModel, id: SyntaxNode, text: Text): MarkKind | null {
  const p = id.parent;
  if (!p || NOT_A_NAME.has(p.name)) return null;
  const name = text(id.from, id.to);
  // `q.m` 的 m（FieldAccess 的最后一个孩子，前面是点）
  if (p.name === "FieldAccess" && id.prevSibling?.name === ".") return memberKind(m, p, name);
  const r = m.resolve(id, name);
  if (r?.kind !== "field") return null;
  return r.info.static ? "staticField" : "field";
}

/** 纯函数：[from, to) 里所有要上色的名字（声明 + 字段使用处），按位置排好 */
export function javaMarks(tree: Tree, text: Text, from: number, to: number): Mark[] {
  const m = modelFor(tree, text);
  const out: Mark[] = [];
  tree.iterate({
    from,
    to,
    enter(n) {
      // iterate 会把恰好贴着端点的节点也交出来（`n.from === to`）；可视区域不止一段时
      // 同一个词会被两段各画一次，所以这里要严格重叠
      if (n.from >= to || n.to <= from) return;
      let kind: MarkKind | null = null;
      if (n.name === "Definition") kind = declKind(n.node, text);
      else if (n.name === "Identifier") kind = nameKind(m, n.node, text);
      if (kind) out.push({ from: n.from, to: n.to, kind });
    },
  });
  return out;
}

// ───────────────────────── 跳转 ─────────────────────────

/**
 * 光标下这个名字，按本文件的符号表能说出什么。给 `jump.ts` 用（经 languageData 交过去，
 * 那边不引这个文件 —— 见 `langs-load.ts` 的 java 分支）。
 *
 * - `decl`：本文件里声明的局部变量 / 字段 → 跳到 `pos`
 * - `member`：`q.name` 且 `q` 的类型写得出来（`recv`）→ 去 recv 那个类里找 name
 * - `opaque`：这是一个成员，但接收者的类型推不出来 → **不给下划线，也不许往下落**
 *   到按名字找的那几层：`order.getItems()` 按名字找会找到本文件里一个同名方法，
 *   跳过去还亮着下划线 —— 这正是它要堵的（2026-09-23 之前就是这样）
 * - `null`：不归这里管（类型名、没限定的方法调用……），照旧走按名字的那几层
 */
export type JavaRef =
  | { kind: "decl"; pos: number }
  | { kind: "member"; recv: string; name: string; call: boolean; argc: number }
  | { kind: "opaque" }
  | null;

/** 一个表达式的类型简单名。只认写出来的：变量的声明类型、`new Foo()`、`(Foo) x`、类型名本身 */
function exprType(m: JavaModel, q: SyntaxNode, text: Text): string | null | "self" {
  switch (q.name) {
    case "this":
      return "self";
    case "Identifier": {
      const s = text(q.from, q.to);
      const r = m.resolve(q, s);
      if (r) return r.kind === "local" ? r.type : r.info.type;
      // 认不出来的大写名字当类型（`OrderUtil.format()` 的 OrderUtil）；小写的多半是继承 / 生成的
      // 字段（Lombok 的 `log`）—— 类型不知道，不猜
      return looksLikeType(s) ? s : null;
    }
    case "FieldAccess": {
      // 只认 `this.field`：`a.b` 要知道 a 那个类里 b 的类型，那是跨文件
      if (q.firstChild?.name !== "this" || !q.lastChild) return null;
      const body = m.enclosingBody(q);
      return (body && m.fieldsOf(body).get(text(q.lastChild.from, q.lastChild.to))?.type) ?? null;
    }
    case "ObjectCreationExpression":
      return simpleType(q.getChild("TypeName") ?? q.getChild("GenericType") ?? q.getChild("ScopedTypeName"), text);
    case "CastExpression":
      return simpleType(q.getChild("TypeName") ?? q.getChild("GenericType"), text);
    case "ParenthesizedExpression":
      return q.firstChild?.nextSibling ? exprType(m, q.firstChild.nextSibling, text) : null;
  }
  return null;
}

/** `(a, b, c)` 里有几个实参 */
function argCount(args: SyntaxNode | null): number {
  let n = 0;
  for (let c = args?.firstChild; c; c = c.nextSibling) if (!["(", ")", ","].includes(c.name)) n++;
  return n;
}

export function javaRefAt(tree: Tree, text: Text, pos: number): JavaRef {
  const id = tree.resolveInner(pos, 1).name === "Identifier" ? tree.resolveInner(pos, 1) : tree.resolveInner(pos, -1);
  if (id.name !== "Identifier") return null;
  const m = modelFor(tree, text);
  const name = text(id.from, id.to);
  const p = id.parent;
  if (!p) return null;

  // 方法调用：`q.name(...)`
  if (p.name === "MethodName") {
    const call = p.parent;
    if (call?.name !== "MethodInvocation" || p.prevSibling?.name !== ".") return null; // 没限定的 helper()：按名字那层管
    const q = call.firstChild;
    if (!q || q === p) return null;
    if (q.name === "super") return { kind: "opaque" };
    const t = exprType(m, q, text);
    if (t === "self") return null; // this.helper()：就是本文件的方法，按名字那层找得到
    if (!t) return { kind: "opaque" };
    return { kind: "member", recv: t, name, call: true, argc: argCount(call.getChild("ArgumentList")) };
  }

  // 字段访问：`q.name`
  if (p.name === "FieldAccess" && id.prevSibling?.name === ".") {
    const q = p.firstChild;
    if (!q) return null;
    const t = exprType(m, q, text);
    if (t === "self") {
      const body = m.enclosingBody(p);
      const f = body && m.fieldsOf(body).get(name);
      return f ? { kind: "decl", pos: f.pos } : { kind: "opaque" };
    }
    if (!t) return { kind: "opaque" };
    return { kind: "member", recv: t, name, call: false, argc: 0 };
  }

  // 裸名字：局部变量 / 字段
  if (NOT_A_NAME.has(p.name)) return null;
  const r = m.resolve(id, name);
  if (!r) return null;
  return { kind: "decl", pos: r.kind === "local" ? r.pos : r.info.pos };
}

export interface MemberSig {
  pos: number;
  kind: "method" | "field";
  /** 形参个数；字段为 0 */
  argc: number;
  /** 最后一个形参是 `T...` */
  varargs: boolean;
}

/**
 * 一个类（按简单名找，嵌套的也算）自己声明的成员：名字 → 各个重载。
 * 找不到这个类就是 null。**不含继承来的** —— 那要读父类的文件，这里不跨文件。
 */
export function javaMembersOf(tree: Tree, text: Text, cls: string): Map<string, MemberSig[]> | null {
  let body: SyntaxNode | null = null;
  tree.iterate({
    enter(n) {
      if (body) return false;
      if (!/^(Class|Interface|Enum)Declaration$/.test(n.name)) return;
      const def = n.node.getChild("Definition");
      if (def && text(def.from, def.to) === cls) {
        body = n.node.getChild("ClassBody") ?? n.node.getChild("InterfaceBody") ?? n.node.getChild("EnumBody");
        return false;
      }
    },
  });
  if (!body) return null;
  const out = new Map<string, MemberSig[]>();
  const add = (def: SyntaxNode | null, sig: Omit<MemberSig, "pos">) => {
    if (!def) return;
    const k = text(def.from, def.to);
    out.set(k, [...(out.get(k) ?? []), { pos: def.from, ...sig }]);
  };
  const visit = (parent: SyntaxNode) => {
    for (let c = parent.firstChild; c; c = c.nextSibling) {
      if (c.name === "MethodDeclaration") {
        const params = c.getChild("FormalParameters");
        const list = params ? [...params.getChildren("FormalParameter"), ...params.getChildren("SpreadParameter")] : [];
        add(c.getChild("Definition"), { kind: "method", argc: list.length, varargs: params?.getChild("SpreadParameter") != null });
      } else if (c.name === "FieldDeclaration" || c.name === "ConstantDeclaration") {
        for (const vd of c.getChildren("VariableDeclarator")) add(vd.getChild("Definition"), { kind: "field", argc: 0, varargs: false });
      } else if (c.name === "EnumConstant") {
        add(c.getChild("Definition"), { kind: "field", argc: 0, varargs: false });
      } else if (c.name === "EnumBodyDeclarations") {
        visit(c);
      }
    }
  };
  visit(body);
  return out;
}

/**
 * 从重载里挑出**唯一**匹配的那个。按实参个数筛（`T...` 吃掉 ≥ n-1 个）；
 * 剩不下一个（没有 / 同个数的几个重载靠类型区分）就是 null —— 不猜。
 */
export function pickMember<T extends MemberSig>(sigs: T[] | undefined, call: boolean, argc: number): T | null {
  if (!sigs) return null;
  const want = sigs.filter((s) =>
    call ? s.kind === "method" && (s.argc === argc || (s.varargs && argc >= s.argc - 1)) : s.kind === "field",
  );
  return want.length === 1 ? want[0] : null;
}

export type PeekMembers = Map<string, (MemberSig & { line: number })[]>;

/** 把一批位置换成行号（1-based）：建一次行首表，二分 */
function lineMapper(text: string): (pos: number) => number {
  const starts = [0];
  for (let i = text.indexOf("\n"); i !== -1; i = text.indexOf("\n", i + 1)) starts.push(i + 1);
  return (pos) => {
    let lo = 0;
    let hi = starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (starts[mid] <= pos) lo = mid;
      else hi = mid - 1;
    }
    return lo + 1;
  };
}

/** 一份 Java 源码里（成员跳转读别的文件用，见 java-peek.ts），和文件同名的那个类的成员（带行号）。不是这个类就是 null */
export function membersFromSource(src: string, cls: string): PeekMembers | null {
  const tree = javaLanguage.parser.parse(src);
  const ms = javaMembersOf(tree, (a, z) => src.slice(a, z), cls);
  if (!ms) return null;
  const lineOf = lineMapper(src);
  const out: PeekMembers = new Map();
  for (const [k, sigs] of ms) out.set(k, sigs.map((s) => ({ ...s, line: lineOf(s.pos) })));
  return out;
}

