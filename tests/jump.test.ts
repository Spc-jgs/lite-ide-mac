/*
 * ⌘Click 跳转的两层解析。
 *
 * 这里测的全是「**该不该给下划线**」—— 那是这个功能唯一的承诺：
 * 有下划线就一定跳得准，跳不准的一律什么都不画。所以**不命中的那些用例
 * 和命中的一样重要**，下面负例比正例多。
 */
import { Compartment, EditorState } from "@codemirror/state";
import { javaLanguage } from "@codemirror/lang-java";
import { typescriptLanguage } from "@codemirror/lang-javascript";
import { resolveJump, importsOf, packageOf, type JumpCtx } from "../src/lib/editor/jump.ts";
import { outlineOf, symbolCache } from "../src/lib/editor/outline.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const java = (doc: string) => EditorState.create({ doc, extensions: [javaLanguage] });
const ts = (doc: string) => EditorState.create({ doc, extensions: [typescriptLanguage] });

/** 停在 `doc` 里第 n 次出现 `word` 的中间 */
const at = (doc: string, word: string, n = 1) => {
  let i = -1;
  for (let k = 0; k < n; k++) i = doc.indexOf(word, i + 1);
  return i + 1;
};

// ── Java：他那个 Spring 项目的形状 ────────────────────────────────────

const JAVA_SRC = `package com.etianqu.admin;

import com.etianqu.framework.client.IEtqAiBlackWordsClient;
import org.springframework.web.bind.annotation.RestController;
import com.etianqu.admin.*;

/** 注释里也提一句 IEtqAiBlackWordsClient，这里不该亮 */
@RestController
public class AdminAiBlackWordsController {
  private final IEtqAiBlackWordsClient blackWordsClient;
  public AdminAiBlackWordsController(IEtqAiBlackWordsClient c) { this.blackWordsClient = c; }
  public void go() { helper(); }
  private void helper() {}
}
`;

const FILES = [
  "etianqu-api/src/main/java/com/etianqu/admin/AdminAiBlackWordsController.java",
  "etianqu-framework/src/main/java/com/etianqu/framework/client/IEtqAiBlackWordsClient.java",
  "etianqu-admin/src/main/java/com/etianqu/admin/OrderService.java",
];
const JBASE: Omit<JumpCtx, "symbols"> = {
  files: FILES,
  rel: "etianqu-api/src/main/java/com/etianqu/admin/AdminAiBlackWordsController.java",
  lang: "java",
};

/**
 * 符号表由调用方给（见 `JumpCtx.symbols`）—— 真实调用里 Editor 会按文档版本
 * 缓存一份，测试里就地算。
 */
const ctxOf = (st: EditorState, base: Omit<JumpCtx, "symbols"> = JBASE): JumpCtx => ({
  ...base,
  symbols: outlineOf(st),
});

{
  const st = java(JAVA_SRC);
  const ctx = ctxOf(st);

  /*
   * 这一条是整个功能的立身之本：跨模块的类型引用。
   * `IEtqAiBlackWordsClient` 在 framework 模块，当前文件在 api 模块 ——
   * 靠的是 Java「包路径 = 目录路径」这条语言规范，不是猜。
   */
  const 类型引用 = resolveJump(st, at(JAVA_SRC, "IEtqAiBlackWordsClient", 3), ctx);
  ok(
    类型引用?.target.rel ===
      "etianqu-framework/src/main/java/com/etianqu/framework/client/IEtqAiBlackWordsClient.java",
    `跨模块的类型引用要跳到 framework 那份（实得 ${类型引用?.target.rel}）`,
  );
  ok(类型引用?.target.why === "import", "依据是 import");

  /*
   * **第三方一律不亮。** Spring 在 jar 里，项目索引中没有它的源码 ——
   * 这一条守的正是「有下划线 = 我确定」：不能让一半的下划线点下去是死路。
   */
  ok(
    resolveJump(st, at(JAVA_SRC, "RestController", 2), ctx) === null,
    "第三方（jar 里的）不该给下划线",
  );

  // 声明本身不亮 —— 从它跳到它自己，点了原地不动，比不给下划线更糟
  ok(
    resolveJump(st, at(JAVA_SRC, "AdminAiBlackWordsController", 2), ctx) === null,
    "类名的声明处不该给下划线",
  );

  /*
   * 注释里那句提到了同一个类名。
   *
   * **这条锁的不是 REF_NODES 白名单**（把 BlockComment 加进白名单，它照样绿）——
   * `resolveInner` 在注释里给回的是整段注释，被 `wordAt` 的形状检查拦下的。
   * 白名单真正拦的是 `Definition`，那是下面「声明处不该给下划线」那条在验。
   */
  ok(
    resolveJump(st, at(JAVA_SRC, "IEtqAiBlackWordsClient", 2), ctx) === null,
    "注释里的类名不该给下划线",
  );

  // 本文件里的方法调用 → 跳到它的声明那一行
  const 本文件 = resolveJump(st, at(JAVA_SRC, "helper", 1), ctx);
  ok(本文件?.target.rel === "" && 本文件?.target.why === "本文件", "本文件的方法调用跳到本文件");
  ok(
    本文件?.target.line ===
      JAVA_SRC.split("\n").findIndex((l) => l.includes("private void helper")) + 1,
    `行号要指到 helper 的声明（实得 ${本文件?.target.line}）`,
  );

  /*
   * **光标贴着词的哪一侧都要能跳。**
   *
   * `resolveInner(pos, 1)` 是向后看的：光标停在词**尾**时它给回的是下一个
   * token（`;` 之类），于是 ⌘B 什么都不做。这不是边角情况 ——
   * ⌘F 找到一个匹配之后光标正好落在词尾，双击选中一个词也是。
   * IDEA 里贴着任一侧都能跳，这条锁住它。
   * （真机 smoke 的 ⑮ 就是栽在这上面，而浏览器里我点的是词中间，没露出来。）
   */
  const 用法处 = JAVA_SRC.indexOf("IEtqAiBlackWordsClient", JAVA_SRC.indexOf("class"));
  for (const [where, pos] of [
    ["词首", 用法处],
    ["词中", 用法处 + 5],
    ["词尾", 用法处 + "IEtqAiBlackWordsClient".length],
  ] as const) {
    const hit = resolveJump(st, pos, ctx);
    ok(hit !== null, `光标在${where}也要跳得了（实得 ${hit ? "OK" : "跳不了"}）`);
  }

  const imps = importsOf(st, "java");
  ok(
    imps.get("IEtqAiBlackWordsClient") === "com.etianqu.framework.client.IEtqAiBlackWordsClient",
    "import 表抠得出全限定名",
  );
  // 通配符给不出唯一答案，不进表
  ok(!imps.has("*"), "通配符 import 不进表");
  ok(packageOf(st) === "com.etianqu.admin", "读得出 package");
}

// ── Java 同包：不写 import 的那一半 ──────────────────────────────────

{
  /*
   * 同包的类**不需要 import**，第二层在这里会落空 —— 而这是 Java 项目里
   * 极常见的一半。靠 `package` 声明推同目录补上，同样是语言规范给的确定性。
   */
  const src = `package com.etianqu.admin;

public class AdminAiBlackWordsController {
  private OrderService svc;
}
`;
  const st = java(src);
  const hit = resolveJump(st, at(src, "OrderService"), ctxOf(st));
  ok(
    hit?.target.rel === "etianqu-admin/src/main/java/com/etianqu/admin/OrderService.java",
    `同包的类要跳到同目录那份（实得 ${hit?.target.rel}）`,
  );
  ok(hit?.target.why === "同包", "依据是同包");

  // 同包里查无此人 —— 不能因为「同包」就瞎给一个
  const 没有的 = `package com.etianqu.admin;
public class X { private NotExisting n; }
`;
  const st2 = java(没有的);
  ok(
    resolveJump(st2, at(没有的, "NotExisting"), ctxOf(st2)) === null,
    "同包里不存在的类不给下划线",
  );
}

// ── TS/JS：相对 import 按当前文件的位置解析 ──────────────────────────

{
  const src = `import { stashed } from "../state/doc";
import Icon from "../shell/Icon.svelte";
import { readFile } from "node:fs";
import * as path from "./util/path";

stashed(null, "x");
readFile("a");
`;
  const st = ts(src);
  const ctx = ctxOf(st, {
    files: ["src/lib/state/doc.ts", "src/lib/shell/Icon.svelte", "src/lib/editor/Editor.svelte"],
    rel: "src/lib/editor/Editor.svelte",
    lang: "typescript",
  });

  // `../state/doc` 要按**当前文件的位置**解析（从 src/lib/editor/ 出发），还要补扩展名
  const 相对 = resolveJump(st, at(src, "stashed", 2), ctx);
  ok(相对?.target.rel === "src/lib/state/doc.ts", `省略的扩展名要补上（实得 ${相对?.target.rel}）`);

  ok(
    resolveJump(st, at(src, "readFile", 2), ctx) === null,
    "包名 import（node_modules 里的）不给下划线",
  );

  const imps = importsOf(st, "typescript");
  ok(imps.get("Icon") === "../shell/Icon.svelte", "默认导入也进表");
  ok(!imps.has("path"), "命名空间导入（* as）不进表");
}

// ── grok review 找出来的三条（2026-09-10）──────────────────────────
//
// 三条都打在同一个地方：**下划线亮着，但跳到了别的文件**。
// 这比「跳不了」严重得多 —— 跳不了你会自己去搜，跳错了你不会怀疑。

{
  /*
   * 多模块重名：两个模块有同一个包、同一个类名。
   *
   * 索引是字典序的，原来 `findIn` 命中第一个就返回 —— 于是在 web 模块里
   * ⌘Click，打开的是 api 模块那份。他那个 etianqu 项目正是这个形状
   * （api / framework / main / module 四个模块共用一批包名）。
   *
   * 「敢跳」的做法是**认怂**：认不准就别画下划线，让它落到搜索退路上。
   */
  const SRC = `package com.demo.api;

import com.demo.core.Foo;

public class Bar {
  private final Foo foo;
}
`;
  const 两个模块都有 = [
    "module-api/src/main/java/com/demo/core/Foo.java",
    "module-web/src/main/java/com/demo/core/Foo.java",
    "module-web/src/main/java/com/demo/api/Bar.java",
  ];
  const st = java(SRC);
  const ctx = ctxOf(st, {
    files: 两个模块都有,
    rel: "module-web/src/main/java/com/demo/api/Bar.java",
    lang: "java",
  });
  ok(
    resolveJump(st, at(SRC, "Foo", 2), ctx) === null,
    "import 的类在两个模块里都有 → 认不准，不给下划线",
  );

  // 只有一份时照常跳 —— 别把「认怂」做成「永远不跳」
  const 只有一份 = ctxOf(st, {
    files: ["module-api/src/main/java/com/demo/core/Foo.java", "module-web/src/main/java/com/demo/api/Bar.java"],
    rel: "module-web/src/main/java/com/demo/api/Bar.java",
    lang: "java",
  });
  ok(
    resolveJump(st, at(SRC, "Foo", 2), 只有一份)?.target.rel ===
      "module-api/src/main/java/com/demo/core/Foo.java",
    "只有一份时照常跳",
  );
}

{
  /*
   * 同包层不该去全项目找后缀 —— 同包**就是同目录**，这是 Java 规定的。
   *
   * 原来它走的也是后缀匹配，于是另一个模块里同包同名的那份会先命中。
   * 而这一层本来是三层里最确定的：目录就在手上。
   */
  const SRC = `package com.demo.api;

public class Bar {
  private final Helper helper;
}
`;
  const st = java(SRC);
  const ctx = ctxOf(st, {
    files: [
      // 字典序在前，原来会被它抢走
      "module-api/src/main/java/com/demo/api/Helper.java",
      "module-web/src/main/java/com/demo/api/Helper.java",
      "module-web/src/main/java/com/demo/api/Bar.java",
    ],
    rel: "module-web/src/main/java/com/demo/api/Bar.java",
    lang: "java",
  });
  ok(
    resolveJump(st, at(SRC, "Helper", 1), ctx)?.target.rel ===
      "module-web/src/main/java/com/demo/api/Helper.java",
    "同包要跳到**自己这个目录**里那份，不是字典序第一个",
  );
}

{
  /*
   * `packageOf` 原来用 `/^\s*package\s+…/m` 扫前 2000 字。
   * `^` 在 `/m` 下匹配每一行行首 —— 注释里那行旧包名会赢。
   * 改包名时把旧行注释掉是很常见的写法。
   *
   * `wordAt` 特意走的语法树（就是为了不把注释当真），这一层不能开倒车。
   */
  const SRC = `/*
package com.old.pkg;
*/
package com.demo.api;

public class Bar {
  private final Helper helper;
}
`;
  const st = java(SRC);
  ok(packageOf(st) === "com.demo.api", `注释里的 package 不算数（实得 ${packageOf(st)}）`);
  const ctx = ctxOf(st, {
    files: [
      "module-web/src/main/java/com/old/pkg/Helper.java",
      "module-web/src/main/java/com/demo/api/Helper.java",
      "module-web/src/main/java/com/demo/api/Bar.java",
    ],
    rel: "module-web/src/main/java/com/demo/api/Bar.java",
    lang: "java",
  });
  ok(
    resolveJump(st, at(SRC, "Helper", 1), ctx)?.target.rel ===
      "module-web/src/main/java/com/demo/api/Helper.java",
    "被注释掉的旧包名不能把同包跳转带到 old 那份去",
  );
}

{
  /*
   * 符号表缓存：**语言是懒加载的，树会从空变成完整，而 `doc` 一个字没变。**
   *
   * 真实时序（Editor.svelte）：`build()` 先塞一个空的 langSlot 把编辑器立起来，
   * 之后 `await import("@codemirror/lang-java")` 回来再 reconfigure。
   * 从文件树 ⌘Click 进来、手还按着 ⌘ 划过编辑器，问到的就是这中间那一拍。
   *
   * 只按 `state.doc` 的身份缓存的话，那一拍存下来的**空表会一直活着**，
   * 本文件那一层从此永远查不到东西 —— 名字落到 import / 同包，
   * 项目里再有一份同名文件，就会亮着下划线跳到别的文件去。
   */
  const SRC = `package com.demo.api;

public class Bar {
  private void helper() {}
  public void go() { helper(); }
}
`;
  const slot = new Compartment();
  let st = EditorState.create({ doc: SRC, extensions: [slot.of([])] });
  const symbolsOf = symbolCache();

  const 语言没到时 = symbolsOf(st);
  ok(语言没到时.length === 0, "语言还没装上，符号表本来就是空的");

  // 语言装好了 —— 注意 doc 一个字都没改，是同一个 Text 对象
  const 装好了 = st.update({ effects: slot.reconfigure(javaLanguage) }).state;
  ok(装好了.doc === st.doc, "前提：reconfigure 不换 doc（换了这条测试就测不到东西）");
  ok(
    symbolsOf(装好了).some((y) => y.name === "helper"),
    `语言装上之后必须重算，不能拿那张空表当结论（实得 ${symbolsOf(装好了).length} 个符号）`,
  );

  // 树没变时不重算 —— 那正是这个缓存存在的理由
  ok(symbolsOf(装好了) === symbolsOf(装好了), "同一个树问两次要拿到同一份（缓存还在起作用）");
}

console.log(`${fail === 0 ? "✅" : "❌"} 跳转解析：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
