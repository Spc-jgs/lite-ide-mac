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

export async function javaCmd(cmd: string, a: A): Promise<unknown> {
  const root = String(a.root ?? "");
  const path = String(a.path ?? "");
  switch (cmd) {
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
    case "java_complete":
      await wait(60); // 实测 59–66 ms（第 3 步的探针）
      docs.set(path, String(a.text ?? ""));
      return complete(path, String(a.text ?? ""), Number(a.line), Number(a.character));
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
