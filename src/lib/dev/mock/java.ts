/**
 * 桩：Java 智能（docs/IDEA.md 第 2 步），对应 `src-tauri/src/commands/java.rs`（规则在 `src-tauri/src/java.rs`）。
 *
 * 照真实现的规则走：只有 Maven / Gradle 项目才起（根上或第一层有构建文件，否则 `java_open` 给 null）；
 * 状态 starting → importing（带「32% …」那种进度）→ ready，**故意走一阵**（0.4 秒、1.6 秒）—— 围着「导入中」做的状态栏那格
 * 在 0ms 的桩上看不见（frontend.md「桩要能停在中间一会儿」）。诊断在改动后 450ms 回来（第 0 步实测 0.43–0.5 秒）。
 *
 * 不模拟的：真的语义。报错只认一种 —— 调了名字以 `lll` 结尾的方法（第 0 步实测用的就是 `findAll` → `findAlll`）；
 * 跳转只认「类名 → 声明这个类的文件」和「方法名 → 某个文件里 `名字(` 开头的声明」，`SpringApplication` 跳进一个假 jar。
 */
import { type A, bump, FILES, NOT_MINE } from "./data";
import { mockEmit } from "./events";
import { recordRename } from "./replace";

type Phase = "starting" | "importing" | "ready" | "failed" | "stopped";
const phaseOf = new Map<string, Phase>();
const docs = new Map<string, string>(); // path → 最新全文

const BUILD = ["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"];
function isBuildRoot(root: string): boolean {
  return Object.keys(FILES).some((p) => {
    if (!p.startsWith(`${root}/`)) return false;
    const rest = p.slice(root.length + 1).split("/");
    return BUILD.includes(rest[rest.length - 1]) && rest.length <= 2 && !rest[0].startsWith(".");
  });
}

function status(root: string) {
  const phase = phaseOf.get(root) ?? "stopped";
  return { root, phase, message: phase === "importing" ? "32% Project 'proj'" : "", detail: "Java 21（内置，桩）· Lombok lombok-1.18.46.jar" };
}

function setPhase(root: string, phase: Phase) {
  phaseOf.set(root, phase);
  mockEmit("java-status", status(root));
}

function start(root: string) {
  if (phaseOf.has(root) && phaseOf.get(root) !== "stopped" && phaseOf.get(root) !== "failed") return;
  setPhase(root, "starting");
  setTimeout(() => setPhase(root, "importing"), 400);
  setTimeout(() => {
    setPhase(root, "ready");
    for (const [p, t] of docs) if (p.startsWith(`${root}/`)) diagnose(root, p, t);
  }, 2000);
}

/** 只认一种错：调了 `xxxlll(` —— 照第 0 步的「findAll → findAlll」 */
function diagnose(root: string, path: string, text: string) {
  const items = text.split("\n").flatMap((l, line) =>
    [...l.matchAll(/\.(\w+lll)\(/g)].map((m) => ({
      line,
      character: m.index! + 1,
      endLine: line,
      endCharacter: m.index! + 1 + m[1].length,
      severity: 1,
      message: `The method ${m[1]}() is undefined for the type ${path.slice(path.lastIndexOf("/") + 1, -5)}`,
    })),
  );
  mockEmit("java-diagnostics", { root, path, total: items.length, items });
}

const timers = new Map<string, ReturnType<typeof setTimeout>>();
function later(root: string, path: string) {
  clearTimeout(timers.get(path));
  if (phaseOf.get(root) !== "ready") return;
  timers.set(path, setTimeout(() => diagnose(root, path, docs.get(path) ?? ""), 450));
}

function wordAt(text: string, line: number, ch: number): string | null {
  const l = text.split("\n")[line] ?? "";
  let a = ch;
  let b = ch;
  while (a > 0 && /[\w$]/.test(l[a - 1])) a--;
  while (b < l.length && /[\w$]/.test(l[b])) b++;
  return a < b ? l.slice(a, b) : null;
}

const loc = (path: string, text: string, re: RegExp) => {
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const m = re.exec(lines[i]);
    if (m) {
      const c = m.index + m[0].length - m[1].length;
      return { uri: `file://${path}`, path, label: path.slice(path.lastIndexOf("/") + 1), line: i, character: c, endLine: i, endCharacter: c + m[1].length, text: "" };
    }
  }
  return null;
};

const JAR = "jdt://contents/spring-boot-4.0.1.jar/org.springframework.boot/SpringApplication.class?=proj/%2Frepo";

function definition(root: string, path: string, line: number, ch: number) {
  const text = docs.get(path) ?? FILES[path] ?? "";
  const w = wordAt(text, line, ch);
  if (!w) return [];
  if (w === "SpringApplication") {
    return [{ uri: JAR, path: null, label: "org.springframework.boot.SpringApplication", line: 152, character: 13, endLine: 152, endCharacter: 30, text: "" }];
  }
  const java = Object.keys(FILES).filter((p) => p.startsWith(`${root}/`) && p.endsWith(".java"));
  for (const p of java) {
    const l = loc(p, docs.get(p) ?? FILES[p], new RegExp(`(?:class|interface|record|enum)\\s+(${w})\\b`));
    if (l) return [l];
  }
  for (const p of java) {
    const l = loc(p, docs.get(p) ?? FILES[p], new RegExp(`^\\s*(?:public|private|protected|static|\\s)*[\\w<>\\[\\]]+\\s+(${w})\\s*\\(`));
    if (l && !(p === path && l.line === line)) return [l];
  }
  return [];
}

function references(root: string, path: string, line: number, ch: number) {
  const w = wordAt(docs.get(path) ?? FILES[path] ?? "", line, ch);
  if (!w) return { items: [], total: 0 };
  const items = Object.keys(FILES)
    .filter((p) => p.startsWith(`${root}/`) && p.endsWith(".java"))
    .flatMap((p) =>
      (docs.get(p) ?? FILES[p]).split("\n").flatMap((l, i) =>
        [...l.matchAll(new RegExp(`\\b${w}\\b`, "g"))]
          .filter(() => !(p === path && i === line))
          .map((m) => ({ uri: `file://${p}`, path: p, label: p.slice(p.lastIndexOf("/") + 1), line: i, character: m.index!, endLine: i, endCharacter: m.index! + w.length, text: l.trim() })),
      ),
    );
  return { items, total: items.length };
}

const CLASS = `// 反编译自 spring-boot-4.0.1.jar（桩：真实现里有源码 jar 就是源码）
package org.springframework.boot;

public class SpringApplication {
${Array.from({ length: 148 }, (_, i) => `    // ……第 ${i + 4} 行`).join("\n")}

    public static ConfigurableApplicationContext run(Class<?> primarySource, String... args) {
        return run(new Class<?>[] { primarySource }, args);
    }
}
`;

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/*
 * 补全（第 3 步）：形状照 jdtls 实测（docs/IDEA.md 第 13 节）—— 方法插 `name(${0})`（snippet），要 import 的类**只在 resolve 时**给 import。
 * `.` 后面：一小张 List 的成员表；裸词：几个 JDK 类 + 本文件里声明的方法。真的语义不模拟
 */
const MEMBERS = [
  { label: "add", detail: "(E e)", description: "boolean" },
  { label: "addAll", detail: "(Collection<? extends E> c)", description: "boolean" },
  { label: "get", detail: "(int index)", description: "E" },
  { label: "isEmpty", detail: "()", description: "boolean" },
  { label: "size", detail: "()", description: "int" },
  { label: "stream", detail: "()", description: "Stream<E>" },
];
const CLASSES = [
  { label: "ArrayList", pkg: "java.util" },
  { label: "HashMap", pkg: "java.util" },
  { label: "List", pkg: "java.util" },
  { label: "Optional", pkg: "java.util" },
  { label: "String", pkg: "java.lang" },
  { label: "StringBuilder", pkg: "java.lang" },
];

function complete(path: string, text: string, line: number, ch: number) {
  const l = text.split("\n")[line] ?? "";
  let a = ch;
  while (a > 0 && /[\w$]/.test(l[a - 1])) a--;
  const word = l.slice(a, ch);
  const range = { line, character: a, endLine: line, endCharacter: ch };
  const item = (label: string, kind: number, newText: string, detail: string, description: string, extra: object = {}) => ({
    label, kind, detail, description, snippet: newText.includes("${"), sort: "", edit: { ...range, text: newText },
    raw: JSON.stringify({ label, path, ...extra }),
  });
  if (l[a - 1] === ".") return { items: MEMBERS.map((m) => item(m.label, 2, `${m.label}(\${0})`, m.detail, m.description)), incomplete: false, total: MEMBERS.length };
  if (!word) return { items: [], incomplete: false, total: 0 };
  const own = [...text.matchAll(/^\s*(?:public|private|protected|static|\s)*[\w<>\[\]]+\s+(\w+)\s*\(([^)]*)\)/gm)].map((m) => item(m[1], 2, `${m[1]}(\${0})`, `(${m[2]})`, ""));
  const cls = CLASSES.map((c) => item(c.label, 7, c.label, "", c.pkg, c.pkg === "java.lang" ? {} : { import: `${c.pkg}.${c.label}` }));
  const items = [...own, ...cls].filter((i) => i.label.toLowerCase().startsWith(word.toLowerCase()));
  return { items, incomplete: false, total: items.length };
}

/*
 * 改名（第 3 步，docs/IDEA.md 第 14 节）：按词边界把项目里所有 .java 的同名词换掉（真的语义不模拟：同名的局部变量也会被换）；
 * 改的是类名、又有同名的 .java，连文件一起改名。**新名字被占时在改任何内容之前就报错**（同 `replacesvc::check_renames`）。
 * 写进替换的那份日志（mock/replace.ts），撤销卡片照样能撤、名字一起改回去。故意等 0.9 秒：真实现第一次改名要 4 秒，状态栏那格要看得见
 */
const KEYWORDS = new Set("abstract boolean break byte case catch char class continue default do double else enum extends final finally float for if implements import instanceof int interface long native new package private protected public return short static super switch synchronized this throw throws try void volatile while var record null true false".split(" "));

function renameTarget(path: string, line: number, ch: number) {
  const text = docs.get(path) ?? FILES[path] ?? "";
  const l = text.split("\n")[line] ?? "";
  let a = ch;
  let b = ch;
  while (a > 0 && /[\w$]/.test(l[a - 1])) a--;
  while (b < l.length && /[\w$]/.test(l[b])) b++;
  const w = l.slice(a, b);
  if (!w || /^\d/.test(w) || KEYWORDS.has(w)) return null;
  return { word: w, target: { line, character: a, endLine: line, endCharacter: b, placeholder: null } };
}

function rename(root: string, path: string, line: number, ch: number, to: string, open: { path: string; text: string; dirty: boolean }[]) {
  const t = renameTarget(path, line, ch);
  if (!t) throw new Error("光标底下不是能改名的东西");
  const w = t.word;
  const dirty = new Map(open.filter((d) => d.dirty).map((d) => [d.path, d.text]));
  const java = Object.keys(FILES).filter((p) => p.startsWith(`${root}/`) && p.endsWith(".java"));
  // 类改名：声明它的那个 W.java 跟着改名。先查新名字有没有被占
  const decl = java.find((p) => p.endsWith(`/${w}.java`) && new RegExp(`(class|interface|record|enum)\\s+${w}\\b`).test(dirty.get(p) ?? FILES[p]));
  const moveTo = decl ? `${decl.slice(0, decl.lastIndexOf("/"))}/${to}.java` : null;
  if (moveTo && FILES[moveTo] !== undefined) throw new Error(`${moveTo.slice(moveTo.lastIndexOf("/") + 1)} 已经有了，不覆盖`);
  const re = new RegExp(`\\b${w.replace(/\$/g, "\\$")}\\b`, "g");
  const entries = [];
  const changed = [];
  for (const p of java) {
    const text = dirty.get(p) ?? FILES[p];
    const hits = [...text.matchAll(re)];
    if (!hits.length) continue;
    const edits = hits.map((m) => ({ from: m.index!, to: m.index! + w.length, insert: to }));
    const out = text.replace(re, to);
    if (!dirty.has(p)) {
      FILES[p] = out;
      bump(p);
    }
    entries.push({ path: p, rel: p.slice(root.length + 1), before: text, after: out });
    changed.push({ rel: p.slice(root.length + 1), path: p, count: hits.length, edits, onDisk: !dirty.has(p) });
  }
  const renamed = [];
  if (decl && moveTo) {
    FILES[moveTo] = FILES[decl];
    delete FILES[decl];
    bump(moveTo);
    renamed.push({ from: decl, to: moveTo });
  }
  recordRename(entries, renamed);
  return { changed, skipped: [], undo: true, renamed };
}

/** 要 import 的：插在 package 那一行后面（文件里已经 import 过就不加，同 jdtls） */
function resolve(raw: string) {
  const it = JSON.parse(raw) as { label: string; path: string; import?: string };
  const doc = `${it.label}（桩里的说明文字：真实现是 Javadoc，markdown 原文）`;
  if (!it.import) return { edits: [], doc };
  const text = docs.get(it.path) ?? FILES[it.path] ?? "";
  if (text.includes(`import ${it.import};`)) return { edits: [], doc };
  const lines = text.split("\n");
  const pk = lines.findIndex((l) => l.startsWith("package "));
  const at = pk >= 0 ? { line: pk, character: lines[pk].length } : { line: 0, character: 0 };
  return { edits: [{ ...at, endLine: at.line, endCharacter: at.character, text: pk >= 0 ? `\n\nimport ${it.import};` : `import ${it.import};\n\n` }], doc };
}

/**
 * MyBatis（docs/IDEA.md 第 24 节）：同 mybatis.rs 的规则，桩用正则做个简化版（规则本身在 Rust 的测试里）——
 * XML 的 namespace / 语句，接口的包名 + 名字 / 方法（缩进一层、「类型 名字(」）
 */
const mbNs = (xml: string) => /<mapper\s[^>]*namespace\s*=\s*["']([^"']+)["']/.exec(xml)?.[1] ?? null;
const mbStatements = (xml: string) =>
  xml.split("\n").flatMap((l, i) => {
    const m = /<(select|insert|update|delete)\s[^>]*\bid\s*=\s*["']([^"']+)["']/.exec(l);
    return m ? [{ name: m[2], kind: m[1], path: "", line: i + 1 }] : [];
  });
const mbInterface = (java: string) => {
  const pkg = /^package\s+([\w.]+);/m.exec(java)?.[1];
  const name = /\binterface\s+(\w+)/.exec(java)?.[1];
  return name ? (pkg ? `${pkg}.${name}` : name) : null;
};
const mbMethods = (java: string) =>
  java.split("\n").flatMap((l, i) => {
    const m = /^ {4}[\w<>[\], ?]+\s+(\w+)\s*\(/.exec(l);
    return m ? [{ name: m[1], kind: "", path: "", line: i + 1 }] : [];
  });

/**
 * Spring（docs/IDEA.md 第 25 节）：同 spring.rs 的规则，桩用正则做个简化版（规则本身在 Rust 的测试里）——
 * 接口：@RestController 类上的 @RequestMapping("前缀") + 方法上的 @XxxMapping("段")；配置：YAML 按缩进拼键
 */
function springEndpoints(root: string) {
  const out: { methods: string[]; path: string; controller: string; method: string; file: string; line: number }[] = [];
  for (const [file, text] of Object.entries(FILES)) {
    if (!file.startsWith(`${root}/`) || !file.endsWith(".java") || typeof text !== "string" || !/@(Rest)?Controller\b/.test(text)) continue;
    const prefix = /@RequestMapping\("([^"]*)"\)\s*(?:public\s+)?class/.exec(text)?.[1] ?? "";
    const controller = /class\s+(\w+)/.exec(text)?.[1] ?? "";
    const lines = text.split("\n");
    lines.forEach((l, i) => {
      const m = /@(Get|Post|Put|Delete|Patch)Mapping(?:\("([^"]*)"\))?/.exec(l);
      if (!m) return;
      const decl = lines.slice(i + 1).findIndex((x) => /\w+\s*\(/.test(x) && !x.trim().startsWith("@"));
      const name = decl >= 0 ? /(\w+)\s*\(/.exec(lines[i + 1 + decl])?.[1] ?? "" : "";
      const path = `/${prefix}/${m[2] ?? ""}`.replace(/\/+/g, "/").replace(/(.)\/$/, "$1");
      out.push({ methods: [m[1].toUpperCase()], path, controller, method: name, file, line: i + 2 + Math.max(0, decl) });
    });
  }
  return out.sort((a, b) => a.path.localeCompare(b.path));
}

function springConfig(root: string, key: string) {
  const norm = (k: string) => k.replace(/[-_]/g, "").toLowerCase();
  const out: { path: string; name: string; line: number; text: string }[] = [];
  for (const [path, text] of Object.entries(FILES)) {
    const name = path.slice(path.lastIndexOf("/") + 1);
    if (!path.startsWith(`${root}/`) || !/^(application|bootstrap).*\.ya?ml$/.test(name) || typeof text !== "string") continue;
    const stack: [number, string][] = [];
    text.split("\n").forEach((l, i) => {
      const m = /^(\s*)([\w.-]+):/.exec(l);
      if (!m) return;
      while (stack.length && stack[stack.length - 1][0] >= m[1].length) stack.pop();
      const full = [...stack.map((s) => s[1]), m[2]].join(".");
      stack.push([m[1].length, m[2]]);
      if (norm(full) === norm(key)) out.push({ path, name, line: i + 1, text: l.trim() });
    });
  }
  return out.sort((a, b) => Number(a.name.includes("-")) - Number(b.name.includes("-")));
}

export async function javaCmd(cmd: string, a: A): Promise<unknown> {
  const root = String(a.root ?? "");
  const path = String(a.path ?? "");
  switch (cmd) {
    case "spring_endpoints":
      return springEndpoints(root);
    case "spring_config":
      return springConfig(root, String(a.key));
    case "mybatis_for_java": {
      const text = String(a.text);
      const fqn = mbInterface(text);
      if (!fqn) return { here: [], there: [] };
      const there = Object.entries(FILES)
        .filter(([p, t]) => p.startsWith(`${root}/`) && p.endsWith(".xml") && typeof t === "string" && mbNs(t) === fqn)
        .flatMap(([p, t]) => mbStatements(t as string).map((s) => ({ ...s, path: p })));
      return { here: mbMethods(text), there };
    }
    case "mybatis_for_xml": {
      const text = String(a.text);
      const fqn = mbNs(text);
      if (!fqn) return { here: [], there: [] };
      const tail = `/${fqn.replace(/\./g, "/")}.java`;
      const java = Object.keys(FILES).find((p) => p.startsWith(`${root}/`) && p.endsWith(tail));
      return { here: mbStatements(text), there: java ? mbMethods(String(FILES[java])).map((m) => ({ ...m, path: java })) : [] };
    }
    case "java_open":
      if (!phaseOf.has(root) && !isBuildRoot(root)) return null;
      docs.set(path, String(a.text ?? ""));
      start(root);
      later(root, path);
      return status(root);
    case "java_change":
      if (!phaseOf.has(root)) return null;
      docs.set(path, String(a.text ?? ""));
      later(root, path);
      return null;
    case "java_save":
      return null;
    case "java_close":
      docs.delete(path);
      return null;
    case "java_definition":
      await wait(15); // 热的时候 10 ms 左右（第 0 步）
      return definition(root, path, Number(a.line), Number(a.character));
    case "java_references":
      await wait(70); // 之后 p50 65 ms
      return references(root, path, Number(a.line), Number(a.character));
    // 跳到实现 / 父方法：桩不懂继承，拿查用法那份顶（看得到「一处直接跳、几处列出来」两种样子）；父方法只给第一处
    case "java_implementation":
      await wait(60);
      return references(root, path, Number(a.line), Number(a.character));
    case "java_hover":
      // 桩：照 jdtls 回的样子（签名代码块 + Javadoc 转的 Markdown）给一段
      await wait(50);
      return "```java\nlong com.demo.PriceRule.apply(long cents)\n```\n\n按规则算出**折后价**（分），见 [Money](jdt://contents/Money.class)。\n\n* **Parameters:**\n  * **cents** 原价\n* **Returns:**\n  * 折后价";
    case "java_highlights": {
      // 同名高亮：桩不懂作用域，按整词找这个文件里同样的字（真的是语义的，同名的别的变量不算）；后面跟着 `=`（不是 `==`）的算写。故意等 30 ms
      await wait(30);
      const text = docs.get(path) ?? String(FILES[path] ?? "");
      const lines = text.split("\n");
      const l = lines[Number(a.line)] ?? "";
      const ch = Number(a.character);
      const id = /[\w$]/;
      let s = ch, e = ch;
      while (s > 0 && id.test(l[s - 1])) s--;
      while (e < l.length && id.test(l[e])) e++;
      const w = l.slice(s, e);
      if (!w || /^\d/.test(w) || KEYWORDS.has(w)) return [];
      return lines.flatMap((x, i) =>
        [...x.matchAll(new RegExp(`(?<![\\w$])${w.replace(/\$/g, "\\$")}(?![\\w$])`, "g"))].map((m) => ({
          line: i, character: m.index!, endLine: i, endCharacter: m.index! + w.length, write: /^\s*=(?!=)/.test(x.slice(m.index! + w.length)),
        })),
      );
    }
    case "java_types": {
      // ⇧⇧ 的「类」：jdtls 没起给 null（同真实现，不为一次搜索起它）。起了就从桩里的 .java 认类型；匹配照 jdtls 的「包含 + 驼峰」做个简化版。
      // 故意等 60 ms（真 jdtls 热的时候几十毫秒）
      if (!phaseOf.has(root)) return null;
      await wait(60);
      const q = String(a.query ?? "").trim().toLowerCase();
      const camel = (n: string) => (n.match(/[A-Z]/g) ?? []).join("").toLowerCase();
      const ok = (n: string) => n.toLowerCase().includes(q) || camel(n).startsWith(q);
      const at = (uri: string, path: string | null, label: string, line: number) => ({ uri, path, label, line, character: 0, endLine: line, endCharacter: 0, text: "" });
      const items = [];
      for (const [p, text] of Object.entries(FILES)) {
        if (!p.startsWith(`${root}/`) || !p.endsWith(".java") || typeof text !== "string") continue;
        const pkg = /^package\s+([\w.]+);/m.exec(text)?.[1] ?? "";
        text.split("\n").forEach((l, i) => {
          const m = /\b(class|interface|enum|record|@interface)\s+(\w+)/.exec(l);
          if (m && ok(m[2])) items.push({ name: m[2], container: pkg, kind: m[1] === "interface" || m[1] === "@interface" ? 11 : m[1] === "enum" ? 10 : 5, at: at(`file://${p}`, p, p.slice(p.lastIndexOf("/") + 1), i) });
        });
      }
      if (a.libs) {
        const libs: [string, string, number, string][] = [["String", "java.lang", 5, "jdt://contents/java.base/java.lang/String.class"], ["Ordered", "org.springframework.core", 11, "jdt://contents/spring-core-7.0.2.jar/org.springframework.core/Ordered.class"], ["SpringApplication", "org.springframework.boot", 5, JAR]];
        for (const [name, container, kind, uri] of libs) if (ok(name)) items.push({ name, container, kind, at: at(uri, null, `${container}.${name}`, 0) });
      }
      return { items, truncated: false };
    }
    case "java_super": {
      await wait(40);
      const r = references(root, path, Number(a.line), Number(a.character));
      return { items: r.items.slice(0, 1), total: Math.min(1, r.total) };
    }
    case "java_complete":
      await wait(60); // 实测 59–66 ms（第 3 步的探针）
      docs.set(path, String(a.text ?? ""));
      return complete(path, String(a.text ?? ""), Number(a.line), Number(a.character));
    case "java_signature": {
      // 实测第一次 168 ms、之后几十 ms（docs/IDEA.md 第 13 节那个探针）：给 80，「敲了括号、过一会儿才出」在桩上看得见
      await wait(80);
      return signature(String(a.text ?? ""), Number(a.line), Number(a.character));
    }
    case "java_resolve":
      await wait(250); // 实测 137–254 ms：所以选中时就在后台问，不等回车
      return resolve(String(a.raw));
    case "java_prepare_rename":
      await wait(40);
      docs.set(path, String(a.text ?? ""));
      return renameTarget(path, Number(a.line), Number(a.character))?.target ?? null;
    case "java_rename":
      await wait(900);
      docs.set(path, String(a.text ?? ""));
      return rename(root, path, Number(a.line), Number(a.character), String(a.newName), (a.open as unknown as { path: string; text: string; dirty: boolean }[]) ?? []);
    case "java_format": {
      // 桩：只去掉行尾空白、把 `=` 两边补上空格（真的排版是 jdtls 的 Eclipse 格式化器）；位置照真实现是 UTF-16 行列。故意等 120 ms（实测）
      await wait(120);
      const text = String(a.text ?? "");
      docs.set(path, text);
      const r = a.range as unknown as { line: number; endLine: number } | null;
      const edits: { line: number; character: number; endLine: number; endCharacter: number; text: string }[] = [];
      text.split("\n").forEach((l, i) => {
        if (r && (i < r.line || i > r.endLine)) return;
        const t = l.replace(/\s+$/, "");
        if (t.length < l.length) edits.push({ line: i, character: t.length, endLine: i, endCharacter: l.length, text: "" });
        for (const m of t.matchAll(/(\w)=(\w)/g)) edits.push({ line: i, character: m.index! + 1, endLine: i, endCharacter: m.index! + 2, text: " = " });
      });
      return edits;
    }
    case "java_code_actions": {
      // 桩：光标那行有一个文件里没 import 过的 JDK 类名（List / Map / Optional）就给几条 import 候选（排序照真实现：java.* 在前、JDK 内部的最后），
      // 外加一条只改这一行的「Add final modifier」。真的修复是 jdtls 算的。故意等 250 ms（实测 190–280）
      await wait(250);
      const text = String(a.text ?? "");
      docs.set(path, text);
      const line = Number((a.range as unknown as { line: number }).line);
      const l = text.split("\n")[line] ?? "";
      const out = [];
      for (const [cls, pkgs] of [["List", ["java.util", "java.awt", "com.sun.tools.javac.util"]], ["Map", ["java.util"]], ["Optional", ["java.util"]]] as const) {
        if (!new RegExp(`\\b${cls}\\b`).test(l) || text.includes(`.${cls};`)) continue;
        for (const p of pkgs) out.push({ title: `Import '${cls}' (${p})`, kind: "quickfix", raw: JSON.stringify({ path, import: `${p}.${cls}` }) });
      }
      if (/^\s+\w/.test(l) && !l.includes("final")) out.push({ title: "Add final modifier", kind: "quickassist", raw: JSON.stringify({ path, finalAt: line }) });
      return out;
    }
    case "java_apply_action": {
      await wait(40);
      const it = JSON.parse(String(a.raw)) as { path: string; import?: string; finalAt?: number };
      if (it.import) return { edits: resolve(JSON.stringify({ label: "", path: it.path, import: it.import })).edits, outcome: null };
      const l = (docs.get(it.path) ?? FILES[it.path] ?? "").split("\n")[it.finalAt!] ?? "";
      const at = l.length - l.trimStart().length;
      return { edits: [{ line: it.finalAt!, character: at, endLine: it.finalAt!, endCharacter: at, text: "final " }], outcome: null };
    }
    case "java_class_contents":
      await wait(30);
      if (String(a.uri) !== JAR) throw new Error("这个类拿不到内容（jdtls 没有回源码，也没有反编译）");
      return CLASS;
    case "java_status":
      return phaseOf.has(root) ? status(root) : null;
    case "java_restart":
      phaseOf.delete(root);
      start(root);
      return null;
    default:
      return NOT_MINE;
  }
}

/**
 * 参数提示（桩）：光标往回找到还没合上的那个 `(`，前面的名字就是方法；数它后面同一层的逗号就是第几个参数。
 * 名字带 `format` 的给 String.format 的两个重载，别的给一个「名字(String a, int b)」—— 真的语义在 jdtls 里，这里只演形状
 */
function signature(text: string, line: number, character: number) {
  const lines = text.split("\n");
  const before = lines.slice(0, line).join("\n") + (line > 0 ? "\n" : "") + (lines[line] ?? "").slice(0, character);
  let depth = 0;
  let commas = 0;
  let open = -1;
  for (let i = before.length - 1; i >= 0; i--) {
    const c = before[i];
    if (c === ")") depth++;
    else if (c === "(") {
      if (depth === 0) {
        open = i;
        break;
      }
      depth--;
    } else if (c === "," && depth === 0) commas++;
    else if (c === ";" || c === "{" || c === "}") break;
  }
  if (open < 0) return { signatures: [], active: 0, param: 0, total: 0 };
  const name = /([A-Za-z_$][\w$]*)\s*$/.exec(before.slice(0, open))?.[1] ?? "";
  const sig = (label: string, ps: string[]) => {
    let from = label.indexOf("(") + 1;
    return { label, doc: "", params: ps.map((p) => { const s = label.indexOf(p, from); from = s + p.length; return { start: s, end: s + p.length }; }) };
  };
  const list = name === "format"
    ? [sig("format(String format, Object... args) : String", ["String format", "Object... args"]), sig("format(Locale l, String format, Object... args) : String", ["Locale l", "String format", "Object... args"])]
    : [sig(`${name}(String a, int b) : void`, ["String a", "int b"])];
  return { signatures: list, active: 0, param: commas, total: list.length };
}
