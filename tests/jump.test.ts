/*
 * ⌘Click 跳转的两层解析。
 *
 * 这里测的全是「**该不该给下划线**」—— 那是这个功能唯一的承诺：
 * 有下划线就一定跳得准，跳不准的一律什么都不画。所以**不命中的那些用例
 * 和命中的一样重要**，下面负例比正例多。
 */
import { EditorState } from "@codemirror/state";
import { javaLanguage } from "@codemirror/lang-java";
import { typescriptLanguage } from "@codemirror/lang-javascript";
import { resolveJump, importsOf, packageOf, type JumpCtx } from "../src/lib/editor/jump.ts";
import { outlineOf } from "../src/lib/editor/outline.ts";

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

console.log(`${fail === 0 ? "✅" : "❌"} 跳转解析：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
