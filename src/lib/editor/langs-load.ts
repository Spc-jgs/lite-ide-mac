/**
 * 语言支持按需懒加载：id → CodeMirror 扩展。
 *
 * 每个 parser 都是几十到几百 KB，全部静态引入会让冷启动变慢 ——
 * 而"秒开"是这个项目的立身之本。改成动态 import 后，打开 .java 才付 Java 的代价：
 * 六十多种语言分散在各自的 chunk 里，入口包一个字节都不为它们买单。
 *
 * 两类实现：
 * - **Lezer**（官方独立包）：增量解析，高亮质量最好，优先用
 * - **legacy stream parser**：逐行扫描，质量略逊，但覆盖面广，
 *   对配置文件、脚本、小众语言完全够用
 *
 * **这个文件必须只被编辑器引用。** 识别语言（`langOf` / `langLabel`）在
 * [langs.ts](./langs.ts)，那半边是入口包要的；这半边一旦被入口包里的谁
 * import 一下，下面这 500 行连同 67 个 `import()` 的桩就又回到入口包里了。
 */

import type { EditorState, Extension } from "@codemirror/state";
import type { LangId } from "./langs";
import type { JavaTools } from "./jump";

const cache = new Map<string, Extension>();

/** 加载语言扩展。同一语言只会真正 import 一次。 */
export async function loadLang(id: LangId): Promise<Extension | null> {
  if (!id) return null;
  const hit = cache.get(id);
  if (hit) return hit;

  let ext: Extension;
  switch (id) {
    case "markdown": {
      const [m, live, logs, paste] = await Promise.all([
        import("@codemirror/lang-markdown"),
        import("./markdown-live"),
        import("./log-segments"),
        import("./paste-fence"),
      ]);
      // base 换成 markdownLanguage：它带 GFM（删除线、表格、任务列表），
      // 默认的 commonmarkLanguage 不认 ~~删除线~~
      ext = [
        // 日志段 / diff 段在语法树里是不做 inline 解析的叶子：粘 2000 行日志之后
        // 每键 20ms 就是它们被当成一个大 Paragraph 反复解析（md-blocks.ts 头上有数字）
        m.markdown({ base: m.markdownLanguage, codeLanguages: [], extensions: [logs.logDiffBlocks] }),
        live.markdownLivePreview,
        // 粘进来的日志按 LogView 那套着色（M10 ②）。挂在 markdown 上而不只是草稿：
        // README 里贴一段日志也该能看清，判据是文件类型不是文件在哪
        logs.logSegments,
        // 粘 JSON 自动包围栏（M10 ②b）
        paste.pasteFence,
      ];
      break;
    }
    case "java": {
      const [m, lang, hl, sem, scope, peek] = await Promise.all([
        import("@codemirror/lang-java"),
        import("@codemirror/language"),
        import("./java-highlight"),
        import("./java-semantic"),
        import("./java-scope"),
        import("./java-peek"),
      ]);
      // 语法包的标签太粗：注解补在标签层（编辑器、缩略图、差异视图都从这里拿语言，三处一起生效）；
      // 方法 / 字段声明补在 ViewPlugin 里，只有编辑器有 —— 为什么分两处见 java-highlight.ts
      const language = m.javaLanguage.configure({ props: [hl.javaStyleTags] });
      const text = (st: EditorState) => (a: number, z: number) => st.sliceDoc(a, z);
      // 成员跳转的那一层（jump.ts 的 `JavaTools`）。走 languageData 交过去，jump.ts 就不用引
      // java-scope —— 不然这几个文件会跟着 jump.ts 进编辑器 chunk，别的语言也得背
      const javaTools: JavaTools = {
        refAt: (st, pos) => scope.javaRefAt(lang.syntaxTree(st), text(st), pos),
        membersHere: (st, cls) => scope.javaMembersOf(lang.syntaxTree(st), text(st), cls),
        peek: peek.peekMembers,
        pick: scope.pickMember,
        settle: peek.settlePeeks,
      };
      ext = new lang.LanguageSupport(language, [sem.javaSemantic, language.data.of({ javaTools })]);
      break;
    }
    case "javascript": {
      const m = await import("@codemirror/lang-javascript");
      ext = m.javascript({ jsx: true });
      break;
    }
    case "typescript": {
      const m = await import("@codemirror/lang-javascript");
      ext = m.javascript({ jsx: true, typescript: true });
      break;
    }
    case "python": {
      const m = await import("@codemirror/lang-python");
      ext = m.python();
      break;
    }
    case "json": {
      const m = await import("@codemirror/lang-json");
      ext = m.json();
      break;
    }
    case "rust": {
      const m = await import("@codemirror/lang-rust");
      ext = m.rust();
      break;
    }
    case "yaml": {
      const m = await import("@codemirror/lang-yaml");
      ext = m.yaml();
      break;
    }
    case "html": {
      const m = await import("@codemirror/lang-html");
      ext = m.html();
      break;
    }
    case "css": {
      const m = await import("@codemirror/lang-css");
      ext = m.css();
      break;
    }
    case "sass": {
      const m = await import("@codemirror/lang-sass");
      ext = m.sass();
      break;
    }
    case "less": {
      const m = await import("@codemirror/lang-less");
      ext = m.less();
      break;
    }
    case "xml": {
      const m = await import("@codemirror/lang-xml");
      ext = m.xml();
      break;
    }
    case "sql": {
      const m = await import("@codemirror/lang-sql");
      ext = m.sql();
      break;
    }
    case "cpp": {
      const m = await import("@codemirror/lang-cpp");
      ext = m.cpp();
      break;
    }
    case "php": {
      const m = await import("@codemirror/lang-php");
      ext = m.php();
      break;
    }
    case "vue": {
      const m = await import("@codemirror/lang-vue");
      ext = m.vue();
      break;
    }
    case "liquid": {
      const m = await import("@codemirror/lang-liquid");
      ext = m.liquid();
      break;
    }
    case "go": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/go"),
      ]);
      ext = StreamLanguage.define(m.go);
      break;
    }
    case "csharp": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/clike"),
      ]);
      ext = StreamLanguage.define(m.csharp);
      break;
    }
    case "kotlin": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/clike"),
      ]);
      ext = StreamLanguage.define(m.kotlin);
      break;
    }
    case "scala": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/clike"),
      ]);
      ext = StreamLanguage.define(m.scala);
      break;
    }
    case "objc": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/clike"),
      ]);
      ext = StreamLanguage.define(m.objectiveC);
      break;
    }
    case "dart": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/clike"),
      ]);
      ext = StreamLanguage.define(m.dart);
      break;
    }
    case "swift": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/swift"),
      ]);
      ext = StreamLanguage.define(m.swift);
      break;
    }
    case "ruby": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/ruby"),
      ]);
      ext = StreamLanguage.define(m.ruby);
      break;
    }
    case "perl": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/perl"),
      ]);
      ext = StreamLanguage.define(m.perl);
      break;
    }
    case "lua": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/lua"),
      ]);
      ext = StreamLanguage.define(m.lua);
      break;
    }
    case "r": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/r"),
      ]);
      ext = StreamLanguage.define(m.r);
      break;
    }
    case "julia": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/julia"),
      ]);
      ext = StreamLanguage.define(m.julia);
      break;
    }
    case "haskell": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/haskell"),
      ]);
      ext = StreamLanguage.define(m.haskell);
      break;
    }
    case "clojure": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/clojure"),
      ]);
      ext = StreamLanguage.define(m.clojure);
      break;
    }
    case "erlang": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/erlang"),
      ]);
      ext = StreamLanguage.define(m.erlang);
      break;
    }
    case "elm": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/elm"),
      ]);
      ext = StreamLanguage.define(m.elm);
      break;
    }
    case "scheme": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/scheme"),
      ]);
      ext = StreamLanguage.define(m.scheme);
      break;
    }
    case "lisp": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/commonlisp"),
      ]);
      ext = StreamLanguage.define(m.commonLisp);
      break;
    }
    case "groovy": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/groovy"),
      ]);
      ext = StreamLanguage.define(m.groovy);
      break;
    }
    case "powershell": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/powershell"),
      ]);
      ext = StreamLanguage.define(m.powerShell);
      break;
    }
    case "vb": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/vb"),
      ]);
      ext = StreamLanguage.define(m.vb);
      break;
    }
    case "pascal": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/pascal"),
      ]);
      ext = StreamLanguage.define(m.pascal);
      break;
    }
    case "fortran": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/fortran"),
      ]);
      ext = StreamLanguage.define(m.fortran);
      break;
    }
    case "verilog": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/verilog"),
      ]);
      ext = StreamLanguage.define(m.verilog);
      break;
    }
    case "vhdl": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/vhdl"),
      ]);
      ext = StreamLanguage.define(m.vhdl);
      break;
    }
    case "tcl": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/tcl"),
      ]);
      ext = StreamLanguage.define(m.tcl);
      break;
    }
    case "coffee": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/coffeescript"),
      ]);
      ext = StreamLanguage.define(m.coffeeScript);
      break;
    }
    case "pug": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/pug"),
      ]);
      ext = StreamLanguage.define(m.pug);
      break;
    }
    case "stylus": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/stylus"),
      ]);
      ext = StreamLanguage.define(m.stylus);
      break;
    }
    case "toml": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/toml"),
      ]);
      ext = StreamLanguage.define(m.toml);
      break;
    }
    case "shell": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/shell"),
      ]);
      ext = StreamLanguage.define(m.shell);
      break;
    }
    case "properties": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/properties"),
      ]);
      ext = StreamLanguage.define(m.properties);
      break;
    }
    case "dockerfile": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/dockerfile"),
      ]);
      ext = StreamLanguage.define(m.dockerFile);
      break;
    }
    case "nginx": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/nginx"),
      ]);
      ext = StreamLanguage.define(m.nginx);
      break;
    }
    case "protobuf": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/protobuf"),
      ]);
      ext = StreamLanguage.define(m.protobuf);
      break;
    }
    case "cmake": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/cmake"),
      ]);
      ext = StreamLanguage.define(m.cmake);
      break;
    }
    case "diff": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/diff"),
      ]);
      ext = StreamLanguage.define(m.diff);
      break;
    }
    case "http": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/http"),
      ]);
      ext = StreamLanguage.define(m.http);
      break;
    }
    case "gherkin": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/gherkin"),
      ]);
      ext = StreamLanguage.define(m.gherkin);
      break;
    }
    case "jinja2": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/jinja2"),
      ]);
      ext = StreamLanguage.define(m.jinja2);
      break;
    }
    case "smalltalk": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/smalltalk"),
      ]);
      ext = StreamLanguage.define(m.smalltalk);
      break;
    }
    case "crystal": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/crystal"),
      ]);
      ext = StreamLanguage.define(m.crystal);
      break;
    }
    case "haxe": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/haxe"),
      ]);
      ext = StreamLanguage.define(m.haxe);
      break;
    }
    case "d": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/d"),
      ]);
      ext = StreamLanguage.define(m.d);
      break;
    }
    case "cobol": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/cobol"),
      ]);
      ext = StreamLanguage.define(m.cobol);
      break;
    }
    case "sparql": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/sparql"),
      ]);
      ext = StreamLanguage.define(m.sparql);
      break;
    }
    case "turtle": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/turtle"),
      ]);
      ext = StreamLanguage.define(m.turtle);
      break;
    }
    case "mathematica": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/mathematica"),
      ]);
      ext = StreamLanguage.define(m.mathematica);
      break;
    }
    case "octave": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/octave"),
      ]);
      ext = StreamLanguage.define(m.octave);
      break;
    }
    default:
      return null;
  }
  cache.set(id, ext);
  return ext;
}

/**
 * 缓存里已经装了几个语言包。**只给诊断用**（`LITE_IDE_DEBUG=1` 那条通道）。
 *
 * 这个 Map 只 set 不 evict 是有意的：67 个语言封顶，加载过就留着，
 * 换回同一种语言不该再付一次解析器的钱。写在这里是为了让
 * 「它到底有没有上限」这件事可以被**量**，而不是只能靠读代码相信。
 */
export function langCacheSize(): number {
  return cache.size;
}
