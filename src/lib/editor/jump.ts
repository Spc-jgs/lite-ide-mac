/**
 * ⌘Click / ⌘B 跳到声明。
 *
 * # 这里只做「敢跳的」那两层
 *
 * IDEA 靠 PSI 全量索引，VSCode 靠 LSP —— 两条路都要一个常驻进程和一次
 * 建索引的等待，和这个应用的立身之本冲突（PLAN.md 里 LSP 是明确排除的）。
 * 所以这里只做**不需要类型解析就能确定**的两层：
 *
 * | 层 | 靠什么 | 准不准 |
 * |---|---|---|
 * | 一、本文件里的声明 | CM6 已经解析好的 Lezer 树 | 准 |
 * | 二、import / package 推出来的文件 | 语言规范强制的「包路径 = 目录路径」 | 准 |
 * | 三、全项目按名字搜 | —— | **不做**，见下 |
 *
 * 第三层是「⇧⌘F 搜这个词」，它在菜单里单独一项，**不挂在 ⌘Click 上**：
 * 一个长得像跳转、精度却是正则的东西，会在你最需要它的那一次把你带到错的
 * 地方，而你不会怀疑它。GitHub 全站的 code navigation 也是按名字匹配，
 * 但它照样把候选列出来让人挑，不假装自己知道答案。
 *
 * # 所以「有下划线」才是这里真正的产品
 *
 * ⌘hover 只在上面两层命中时才画下划线。给不出准确答案的词什么都不显示 ——
 * 于是下划线不只是「可以点」，它是在说「**这一下我确定**」。
 *
 * 对 Spring 项目还有个白捡的好处：跨模块的类在同一个项目根下，查得到源码；
 * 第三方（Spring、Lombok）在 jar 里，查不到。下划线正好把「我们自己的代码」
 * 和「外部依赖」分开了 —— 那恰恰是看陌生代码时最想要的一条界线。
 */

import { syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
// `import type` 在剥类型之后一个字节都不剩 —— 这里**不能**改成值导入：
// 那会让 jump.ts 依赖 outline.ts，而 tests/ 是 node 直接跑 .ts 的，
// 相对 import 少个扩展名就跑不起来（`ERR_MODULE_NOT_FOUND`）。
// 符号表由调用方传进来，理由见 `JumpCtx.symbols`。
import type { Sym } from "./outline";

export interface JumpTarget {
  /** 相对项目根的路径。**空串表示就在本文件里** */
  rel: string;
  /** 1-based 行号；没有就是跳到文件开头 */
  line?: number;
  /** 凭什么敢跳 —— 进 tooltip，让人知道这一下的依据 */
  why: "本文件" | "import" | "同包";
}

export interface JumpHit {
  /** 命中的那个词在文档里的范围，用来画下划线 */
  from: number;
  to: number;
  text: string;
  target: JumpTarget;
}

/**
 * 能当「引用」跳出去的节点。
 *
 * **这张白名单实际拦住的是 `Definition` 系** —— 那是声明本身，从它跳到它
 * 自己没有意义，而画一条点了原地不动的下划线比不画更糟。（验过：把
 * `Definition` 加进来，「声明处不该给下划线」当场变红。）
 *
 * **它拦不住、也不需要拦注释和字符串**：那条由 [`wordAt`] 里的形状检查兜着，
 * 见那边的注释。别以为白名单在防注释而把那条检查删掉。
 *
 * 名字按语言各不相同，所以这里收的是各语言的并集：
 * `TypeName` 是 Java 的类型引用、`VariableName` 是 TS/JS 的值引用、
 * `Identifier` 是方法名那一档（父节点 `MethodName`）。
 */
const REF_NODES = new Set([
  "TypeName",
  "TypeIdentifier",
  "VariableName",
  "PropertyName",
  "Identifier",
  "FieldIdentifier",
]);

/** Java 的包路径就是目录路径，这是语言规范强制的 —— 第二层准确性的全部来源 */
const JAVA_LIKE = new Set(["java", "kotlin", "scala", "groovy"]);
const TS_LIKE = new Set(["typescript", "javascript", "svelte", "vue"]);

/** TS/JS 省略扩展名时按这个顺序试 */
const TS_EXTS = [".ts", ".tsx", ".js", ".jsx", ".mjs", ".svelte", ".vue"];

/**
 * 光标（或鼠标）落点上的标识符。
 *
 * 走语法树而不是正则取词，注释里的类名因此不会亮 —— 但**机制和想的不一样**，
 * 值得写下来：`resolveInner` 落在注释里时给回的是**整个 `BlockComment` /
 * `LineComment` 节点**，也就是「/** 注释里提一句 Foo，这里不该亮 *&#47;」一整段。
 * 拦住它的是下面那行形状检查（有空白、或者太长），不是上面那张白名单。
 *
 * 所以那行检查**不是保险丝，是主闸**。删掉它，一句 `//Foo` 这样不含空格的
 * 行注释就会被当成标识符 `//Foo`（虽然带着 `//` 匹配不上任何符号，
 * 但那只是碰巧不命中，不是拦住了）。
 */
export function wordAt(
  state: EditorState,
  pos: number,
): { from: number; to: number; text: string } | null {
  const tree = syntaxTree(state);
  /*
   * **两侧都要试。**
   *
   * `resolveInner(pos, 1)` 是向后看的：光标停在词**尾**时它给回的是下一个
   * token（`;` 之类），⌘B 于是什么都不做。而「光标停在词尾」根本不是边角
   * 情况 —— ⌘F 找到一个匹配之后光标正好落在那儿，双击选中一个词也是。
   * IDEA 里贴着词的任一侧都能跳。
   *
   * 先试 `1` 再试 `-1`：这个顺序保证「光标在词首」时拿到的是**后面**那个词
   * （那是人指着的那个），只有它不是标识符时才回头看前一个。
   *
   * 这个 bug 是真机 smoke 抓到的 —— 浏览器里我点的是词中间，两侧都对，
   * 一次都没露出来。
   */
  for (const side of [1, -1] as const) {
    const node = tree.resolveInner(pos, side);
    if (!REF_NODES.has(node.name)) continue;
    const text = state.doc.sliceString(node.from, node.to).trim();
    // 长度设个上限：解析没跟上时 resolveInner 可能给回一大块
    if (!text || text.length > 200 || /\s/.test(text)) continue;
    return { from: node.from, to: node.to, text };
  }
  return null;
}

/**
 * 光标底下那个词，**不挑节点类型**。
 *
 * 和 [`wordAt`] 是两件事，别合并：那个服务「能不能精确跳」，所以只认标识符
 * 节点；这个服务「拿它去搜一下」，注释里、字符串里的词照样值得搜。
 * 按字符抠而不是走语法树，正是因为它不该受语法结构的约束。
 */
export function rawWordAt(state: EditorState, pos: number): string | null {
  const line = state.doc.lineAt(pos);
  const col = pos - line.from;
  const text = line.text;
  const isWord = (c: string) => /[\w$]/.test(c);
  let a = col;
  let b = col;
  while (a > 0 && isWord(text[a - 1])) a--;
  while (b < text.length && isWord(text[b])) b++;
  const w = text.slice(a, b);
  return w && !/^\d+$/.test(w) ? w : null;
}

/**
 * `import` 语句：简单名 → 它来自哪儿。
 *
 * 两种语言的「来自哪儿」不是一回事，所以这里只负责抠出**原样的说明符**，
 * 变成路径是 [`candidatesFor`] 的活：
 *
 * - Java：`import com.a.b.C;` → `C` → `com.a.b.C`（全限定名）
 * - TS：`import { x } from "./foo"` → `x` → `./foo`（模块路径）
 *
 * **通配符 import 一律跳过**（`import com.a.*`、`import * as ns`）：
 * 它给不出唯一答案，而给一个「可能是它」的下划线就是在破坏
 * 「有下划线 = 我确定」这条唯一的承诺。这种词落到菜单里那条搜索退路上。
 */
export function importsOf(state: EditorState, lang: string): Map<string, string> {
  const out = new Map<string, string>();
  const tree = syntaxTree(state);
  const java = JAVA_LIKE.has(lang);

  tree.iterate({
    enter: (node) => {
      if (node.name !== "ImportDeclaration") return;
      const text = state.doc.sliceString(node.from, node.to);

      if (java) {
        // `import static com.a.b.C.method;` 里要的仍是 C 那一段，
        // 而 `import com.a.*;` 没有简单名可言 —— 两种都由这个正则筛掉
        const m = /^\s*import\s+(?:static\s+)?([\w.]+)\s*;/.exec(text);
        if (!m) return;
        const fqn = m[1];
        const simple = fqn.slice(fqn.lastIndexOf(".") + 1);
        if (!simple || simple === "*") return;
        out.set(simple, fqn);
        return;
      }

      // TS/JS：模块路径在那个 String 节点里，名字在 ImportGroup / 默认导入里
      const spec = /from\s*["']([^"']+)["']/.exec(text)?.[1];
      if (!spec) return;
      // `import * as ns from "..."` —— 命名空间导入给不出唯一的符号，跳过
      const names = text.replace(/^\s*import\s+type\s+/, "import ").replace(/\bfrom\b[\s\S]*$/, "");
      if (/\*\s*as\b/.test(names)) return;
      for (const raw of names.replace(/^\s*import\s*/, "").split(/[{},]/)) {
        const n = raw.replace(/\btype\b/g, "").trim();
        // `a as b` 用的是 b
        const alias = /\bas\s+([\w$]+)/.exec(n)?.[1];
        const name = alias ?? n;
        if (name && /^[A-Za-z_$][\w$]*$/.test(name)) out.set(name, spec);
      }
    },
  });
  return out;
}

/** Java 的 `package a.b.c;`。同包的类不写 import，靠它推同目录 */
export function packageOf(state: EditorState): string | null {
  const head = state.doc.sliceString(0, Math.min(state.doc.length, 2000));
  return /^\s*package\s+([\w.]+)\s*;/m.exec(head)?.[1] ?? null;
}

/** `a/b/../c` → `a/c`。TS 的相对 import 必须先规范化才能拿去比 */
function normalize(p: string): string {
  const out: string[] = [];
  for (const seg of p.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") out.pop();
    else out.push(seg);
  }
  return out.join("/");
}

/**
 * 说明符 → 候选的相对路径（**按语言规范推，不是猜**）。
 *
 * Java 给的是后缀（`com/a/b/C.java`）：项目里的模块结构无从得知
 * （`etianqu-api/src/main/java/com/...`），但包路径那一段是确定的，
 * 拿它去和文件索引做后缀匹配就够了。
 *
 * TS 给的是完整相对路径：`./foo` 要按**当前文件的位置**解析，
 * 再逐个试扩展名和 `/index`。
 */
function candidatesFor(spec: string, lang: string, fromRel: string): string[] {
  if (JAVA_LIKE.has(lang)) {
    const base = spec.replace(/\./g, "/");
    return [`${base}.java`, `${base}.kt`, `${base}.scala`, `${base}.groovy`];
  }
  if (!TS_LIKE.has(lang)) return [];
  // 非相对路径是包名（node_modules 里的），项目里没有源码
  if (!spec.startsWith(".")) return [];
  const dir = fromRel.slice(0, fromRel.lastIndexOf("/"));
  const base = normalize(`${dir}/${spec}`);
  const out: string[] = [];
  // 写全了扩展名的（`./Icon.svelte`）直接就是答案
  if (/\.[a-z]+$/.test(base)) out.push(base);
  for (const e of TS_EXTS) out.push(`${base}${e}`);
  for (const e of TS_EXTS) out.push(`${base}/index${e}`);
  return out;
}

/**
 * 在文件索引里坐实一个候选。
 *
 * Java 走**后缀**匹配（模块前缀未知），TS 走全等。
 * 索引是相对项目根的路径列表，也就是 ⌘P 那一份 —— 没有为跳转多建一套索引，
 * 「这个文件在不在项目里」这个问题它本来就答得了。
 */
function findIn(files: string[], cands: string[], suffix: boolean): string | null {
  for (const c of cands) {
    if (suffix) {
      const hit = files.find((f) => f === c || f.endsWith(`/${c}`));
      if (hit) return hit;
    } else if (files.includes(c)) {
      return c;
    }
  }
  return null;
}

export interface JumpCtx {
  /**
   * 本文件的符号表（`outlineOf(state)`）。
   *
   * **由调用方传进来而不是这里现算**：`outlineOf` 要遍历整棵语法树，
   * 而 ⌘hover 鼠标每动一格就问一次 —— 现算等于把整棵树遍历成百上千遍。
   * 调用方按文档版本缓存一份，鼠标怎么动都只算一次。
   */
  symbols: Sym[];
  /** ⌘P 那份文件索引：相对项目根的路径 */
  files: string[];
  /** 当前文件相对项目根的路径。不在项目里（比如草稿）就是 null */
  rel: string | null;
  /** 语言 id，取自 `langs.langOf(path)` */
  lang: string;
}

/**
 * 解析一次跳转。**同步纯函数** —— ⌘hover 每动一下都要问它一次，
 * 中间隔一次 IPC 的话下划线会跟不上鼠标。
 *
 * 顺序就是可信度从高到低：本文件的声明 → import → 同包。
 * 一层都不中就返回 null，界面上什么都不画。
 */
export function resolveJump(state: EditorState, pos: number, ctx: JumpCtx): JumpHit | null {
  const w = wordAt(state, pos);
  if (!w) return null;

  // ── 一、本文件里的声明 ──
  const here = state.doc.lineAt(w.from).number;
  const local = ctx.symbols.find((s) => s.name === w.text);
  // 声明就在光标这一行时不给下划线：跳到自己等于点了没反应
  if (local && local.line !== here) {
    return { ...w, target: { rel: "", line: local.line, why: "本文件" } };
  }

  if (!ctx.rel) return null;
  const javaLike = JAVA_LIKE.has(ctx.lang);

  // ── 二、import ──
  const spec = importsOf(state, ctx.lang).get(w.text);
  if (spec) {
    const hit = findIn(ctx.files, candidatesFor(spec, ctx.lang, ctx.rel), javaLike);
    if (hit) return { ...w, target: { rel: hit, why: "import" } };
  }

  // ── 三、同包（Java 同包不写 import，包路径就是目录路径）──
  if (javaLike) {
    const pkg = packageOf(state);
    if (pkg) {
      const hit = findIn(ctx.files, candidatesFor(`${pkg}.${w.text}`, ctx.lang, ctx.rel), true);
      // 命中自己那一份不算 —— 那就是当前文件
      if (hit && hit !== ctx.rel) return { ...w, target: { rel: hit, why: "同包" } };
    }
  }

  return null;
}
