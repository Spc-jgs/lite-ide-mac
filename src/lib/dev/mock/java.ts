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
    label, kind, detail, description, snippet: newText.includes("${"), sort: "", edit: { ...range, text: newText }, insert: null,
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

/**
 * 移动类（F6，G12）：桩用正则做个简化版 —— 目标包是桩里 .java 文件所在的那些目录（包名取文件里的 package 行），要改的是这个文件的 package 行
 * 和 `import 旧包.类名;` 的那几处（真的是 jdtls 算，还会给同包里不带 import 直接用它的文件补 import）
 */
function pkgOf(text: string) {
  return /^package\s+([\w.]+);/m.exec(text)?.[1] ?? "";
}
function moveDests(root: string, path: string) {
  const seen = new Map<string, { name: string; uri: string; project: string; here: boolean }>();
  for (const p of Object.keys(FILES)) {
    if (!p.startsWith(`${root}/`) || !p.endsWith(".java") || p.includes("/src/test/")) continue;
    const dir = p.slice(0, p.lastIndexOf("/"));
    const name = pkgOf(FILES[p]);
    if (!name || seen.has(dir)) continue;
    seen.set(dir, { name, uri: `file://${dir}/`, project: p.slice(root.length + 1).split("/")[0], here: dir === path.slice(0, path.lastIndexOf("/")) });
  }
  return [...seen.values()].sort((a, b) => (a.project + a.name).localeCompare(b.project + b.name));
}
function movePlan(root: string, path: string, dest: string) {
  const d = moveDests(root, path).find((x) => x.uri === dest);
  if (!d) throw new Error("Cannot enable move operation.");
  const name = path.slice(path.lastIndexOf("/") + 1, -".java".length);
  const old = pkgOf(FILES[path] ?? "");
  const to = `${dest.replace(/^file:\/\//, "").replace(/\/$/, "")}/${name}.java`;
  if (FILES[to] !== undefined) throw new Error(`${name}.java 已经有了，不覆盖`);
  const edits = new Map<string, string>();
  edits.set(path, (FILES[path] ?? "").replace(`package ${old};`, `package ${d.name};`));
  for (const p of Object.keys(FILES)) {
    if (p !== path && p.startsWith(`${root}/`) && FILES[p].includes(`import ${old}.${name};`)) edits.set(p, FILES[p].replace(`import ${old}.${name};`, `import ${d.name}.${name};`));
  }
  return { edits, to, rel: (p: string) => p.slice(root.length + 1) };
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

/** 层级的桩：这个项目里的方法（声明在哪、方法体里调了谁） */
function mockMethods(root: string) {
  const out: { name: string; calls: { name: string; line: number; character: number }[]; node: Record<string, unknown> }[] = [];
  for (const [p, text] of Object.entries(FILES)) {
    if (!p.startsWith(`${root}/`) || !p.endsWith(".java") || typeof text !== "string") continue;
    const cls = /\b(?:class|interface|enum|record)\s+(\w+)/.exec(text)?.[1] ?? "";
    let cur: (typeof out)[number] | null = null;
    text.split("\n").forEach((l, i) => {
      const d = /^\s*(?:(?:public|private|protected|static|final|synchronized)\s+)*[\w<>\[\],.? ]+\s+(\w+)\s*\([^)]*\)\s*(?:throws [\w, .]+)?\{/.exec(l);
      if (d && !KEYWORDS.has(d[1])) {
        const at = { uri: `file://${p}`, path: p, label: p.slice(p.lastIndexOf("/") + 1), line: i, character: l.indexOf(d[1]), endLine: i, endCharacter: l.indexOf(d[1]) + d[1].length, text: "" };
        cur = { name: d[1], calls: [], node: { name: d[1], detail: cls, kind: 6, at, site: null, raw: JSON.stringify({ name: d[1] }) } };
        out.push(cur);
        return;
      }
      if (!cur) return;
      for (const m of l.matchAll(/\b(\w+)\s*\(/g)) if (!KEYWORDS.has(m[1])) cur.calls.push({ name: m[1], line: i, character: m.index! });
    });
  }
  return out;
}

/** 层级的桩：这个项目里的类型（extends / implements 了谁） */
function mockTypes(root: string) {
  const out: { name: string; supers: string[]; node: Record<string, unknown> }[] = [];
  for (const [p, text] of Object.entries(FILES)) {
    if (!p.startsWith(`${root}/`) || !p.endsWith(".java") || typeof text !== "string") continue;
    text.split("\n").forEach((l, i) => {
      const m = /\b(class|interface|enum|record)\s+(\w+)([^{]*)/.exec(l);
      if (!m) return;
      const supers = [...m[3].matchAll(/\b([A-Z]\w*)/g)].map((x) => x[1]).filter((x) => x !== "extends" && x !== "implements");
      const at = { uri: `file://${p}`, path: p, label: p.slice(p.lastIndexOf("/") + 1), line: i, character: l.indexOf(m[2]), endLine: i, endCharacter: l.indexOf(m[2]) + m[2].length, text: "" };
      out.push({ name: m[2], supers, node: { name: m[2], detail: /^package\s+([\w.]+);/m.exec(text)?.[1] ?? "", kind: m[1] === "interface" ? 11 : m[1] === "enum" ? 10 : 5, at, site: null, raw: JSON.stringify({ name: m[2] }) } });
    });
  }
  return out;
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
    case "java_gen_status": {
      // 生成：桩认「private 类型 名字;」那种字段，toString 之外的几种都给同一份字段；覆盖方法给 Object 的两个。真的是 jdtls 算的。故意等 120 ms
      await wait(120);
      const text = String(a.text ?? "");
      const what = String(a.what);
      const type = /\b(?:class|record)\s+(\w+)/.exec(text)?.[1] ?? "";
      const fields = [...text.matchAll(/^\s*(?:private|protected|public)?\s*(?:final\s+)?([\w<>.]+)\s+(\w+)\s*(?:=[^;]*)?;/gm)].filter((m) => !KEYWORDS.has(m[1]) || m[1] === "int" || m[1] === "long");
      const item = (label: string, detail: string, raw: Record<string, unknown>, checked = true) => ({ label, detail, checked, raw: JSON.stringify(raw) });
      if (what === "override")
        return { typeName: type, note: "", items: [item("toString()", "Object", { name: "toString", parameters: [] }, false), item("hashCode()", "Object", { name: "hashCode", parameters: [] }, false)] };
      const items = fields.map((m) => item(m[2], m[1], { name: m[2], type: m[1] }));
      return { typeName: type, note: what === "tostring" && /String toString\(\)/.test(text) ? "这个类已经有 toString() 了：生成会替换掉它" : "", items };
    }
    case "java_gen_apply": {
      // 插在类的最后一个 } 前面：每选一项一行（真的是 jdtls 的代码生成，会排版、会照代码风格）
      await wait(150);
      const text = String(a.text ?? "");
      const lines = text.split("\n");
      const last = lines.map((l, i) => (/^}/.test(l) ? i : -1)).filter((i) => i >= 0).pop() ?? lines.length - 1;
      const names = ((a.picked as unknown as string[]) ?? []).map((r) => JSON.parse(r).name as string);
      const what = String(a.what);
      const body =
        what === "tostring" ? `    @Override\n    public String toString() {\n        return "${names.join(", ")}";\n    }\n`
        : what === "constructor" ? `    public Generated(${names.join(", ")}) {\n    }\n`
        : names.map((n) => `    // ${what} ${n}\n`).join("");
      return { edits: [{ line: last, character: 0, endLine: last, endCharacter: 0, text: `\n${body}` }], outcome: null };
    }
    case "java_hier_prepare": {
      // 层级：桩不懂语义 —— 方法认「名字(…) {」的声明，类型认 class / interface 后面的名字；调用方 = 别的方法体里出现了「名字(」。
      // 真的是 jdtls 算的（重载、继承、跨模块都认得）。故意等 60 ms
      await wait(60);
      const text = docs.get(path) ?? String(FILES[path] ?? "");
      const l = text.split("\n")[Number(a.line)] ?? "";
      const ch = Number(a.character);
      let s = ch, e = ch;
      while (s > 0 && /\w/.test(l[s - 1])) s--;
      while (e < l.length && /\w/.test(l[e])) e++;
      const w = l.slice(s, e);
      if (!w) return [];
      const n = a.types ? mockTypes(root).find((t) => t.name === w) : mockMethods(root).find((m) => m.name === w);
      return n ? [n.node] : [];
    }
    case "java_hier_children": {
      await wait(80);
      const item = JSON.parse(String(a.raw)) as { name: string };
      const dir = String(a.dir);
      if (dir === "in") {
        const out = [];
        for (const m of mockMethods(root)) for (const c of m.calls) if (c.name === item.name) out.push({ ...m.node, site: { ...(m.node.at as Record<string, unknown>), line: c.line, character: c.character, endLine: c.line, endCharacter: c.character } });
        return { items: out, total: out.length };
      }
      if (dir === "out") {
        const me = mockMethods(root).find((m) => m.name === item.name);
        const all = mockMethods(root);
        const out = (me?.calls ?? []).map((c) => all.find((m) => m.name === c.name)?.node).filter((x) => !!x);
        return { items: out, total: out.length };
      }
      const ts = mockTypes(root);
      const me = ts.find((t) => t.name === item.name);
      const out = dir === "super" ? ts.filter((t) => me?.supers.includes(t.name)).map((t) => t.node) : ts.filter((t) => t.supers.includes(item.name)).map((t) => t.node);
      return { items: out, total: out.length };
    }
    case "java_inlay_hints": {
      // 参数名提示：桩只认「这个文件里声明过的方法、调用时传的是字面量」—— 照 jdtls 的默认（literals）。真的是语义的（重载、别的类的方法都认得）。故意等 40 ms
      await wait(40);
      const text = docs.get(path) ?? String(FILES[path] ?? "");
      const lines = text.split("\n");
      const params = new Map<string, string[]>();
      for (const m of text.matchAll(/\b(\w+)\s*\(([^()]*)\)\s*(?:throws\s+[\w, ]+)?\{/g)) {
        const names = m[2].split(",").map((p) => p.trim().split(/\s+/).pop() ?? "").filter(Boolean);
        if (names.length && !KEYWORDS.has(m[1])) params.set(m[1], names);
      }
      const out: { line: number; character: number; label: string; param: boolean }[] = [];
      // 照真实现：终点不是文件里真有的位置就整段回空（jdtls 换不成偏移）—— 不照的话「问到下一行」那个错在桩上永远看不出来
      const endLine = Number(a.endLine);
      if (endLine >= lines.length || Number(a.endChar) > lines[endLine].length) return [];
      for (let i = Number(a.from); i <= endLine; i++) {
        for (const m of lines[i].matchAll(/\b(\w+)\(([^()]*)\)/g)) {
          const names = params.get(m[1]);
          if (!names || /\{\s*$/.test(lines[i].slice(m.index! + m[0].length))) continue;
          let at = m.index! + m[1].length + 1;
          m[2].split(",").forEach((arg, k) => {
            const lead = arg.length - arg.trimStart().length;
            if (names[k] && /^\s*(\d[\d_]*[lLdDfF]?|"[^"]*"|true|false|null)\s*$/.test(arg)) out.push({ line: i, character: at + lead, label: `${names[k]}:`, param: true });
            at += arg.length + 1;
          });
        }
      }
      return out;
    }
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
    case "java_move_destinations":
      await wait(300);
      return moveDests(root, path);
    case "java_move_preview": {
      // 第一次要搜全部索引，真实现会慢：桩也等一会儿，「正在算要改哪些」那一句看得见
      await wait(700);
      const m = movePlan(root, path, String(a.dest));
      return { files: [...m.edits.keys()].map(m.rel).sort(), from: m.rel(path), to: m.rel(m.to) };
    }
    case "java_move": {
      await wait(500);
      const m = movePlan(root, path, String(a.dest));
      if (JSON.stringify([...m.edits.keys()].map(m.rel).sort()) !== JSON.stringify(a.expect)) throw new Error("预览之后有文件变了，要改的和刚才列的不一样 —— 再预览一次");
      const entries = [];
      const changed = [];
      for (const [p, out] of m.edits) {
        const before = FILES[p];
        FILES[p] = out;
        bump(p);
        entries.push({ path: p, rel: m.rel(p), before, after: out });
        changed.push({ rel: m.rel(p), path: p, count: 1, edits: [{ from: 0, to: before.length, insert: out }], onDisk: true });
      }
      FILES[m.to] = FILES[path];
      delete FILES[path];
      bump(m.to);
      recordRename(entries, [{ from: path, to: m.to }]);
      return { changed, skipped: [], undo: true, renamed: [{ from: path, to: m.to }] };
    }
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
    case "java_organize_imports": {
      // 桩：把挨在一起的 import 行按 IDEA 默认的次序排（其余、javax、java、static），不删不补（真的是 jdtls 算，还会删没用的、补缺的）。等 200 ms
      await wait(200);
      const text = String(a.text ?? "");
      docs.set(path, text);
      const lines = text.split("\n");
      const idx = lines.map((l, i) => (/^import\s/.test(l) ? i : -1)).filter((i) => i >= 0);
      if (idx.length < 2) return [];
      const [first, last] = [idx[0], idx[idx.length - 1]];
      const rank = (l: string) => (/^import static /.test(l) ? 3 : /^import java\./.test(l) ? 2 : /^import javax\./.test(l) ? 1 : 0);
      const sorted = lines.slice(first, last + 1).filter((l) => l.trim()).sort((x, y) => rank(x) - rank(y) || x.localeCompare(y));
      const out: string[] = [];
      sorted.forEach((l, i) => {
        if (i > 0 && rank(l) !== rank(sorted[i - 1]) && !(rank(l) === 2 && rank(sorted[i - 1]) === 1)) out.push("");
        out.push(l);
      });
      const next = out.join("\n");
      if (next === lines.slice(first, last + 1).join("\n")) return [];
      return [{ line: first, character: 0, endLine: last, endCharacter: lines[last].length, text: next }];
    }
    case "java_code_actions": {
      // 桩：光标那行有一个文件里没 import 过的 JDK 类名（List / Map / Optional）就给几条 import 候选（排序照真实现：java.* 在前、JDK 内部的最后），
      // 外加一条只改这一行的「Add final modifier」。真的修复是 jdtls 算的。故意等 250 ms（实测 190–280）
      await wait(250);
      const text = String(a.text ?? "");
      docs.set(path, text);
      // 重构的快捷键（docs/TAKEOVER.md G12）：桩只演「抽成局部变量」—— 选中的那段换成 `extracted`、上一行加声明（真的名字、类型、替换几处是 jdtls 算的）
      if (a.only) {
        const r = a.range as unknown as { line: number; character: number; endLine: number; endCharacter: number };
        if (a.only !== "refactor.extract.variable" || r.line !== r.endLine || r.character === r.endCharacter) return [];
        const l = text.split("\n")[r.line] ?? "";
        const ind = l.slice(0, l.length - l.trimStart().length);
        const expr = l.slice(r.character, r.endCharacter);
        const edits = [
          { line: r.line, character: 0, endLine: r.line, endCharacter: 0, text: `${ind}var extracted = ${expr};\n` },
          { line: r.line, character: r.character, endLine: r.line, endCharacter: r.endCharacter, text: "extracted" },
        ];
        return [
          { title: "Extract to local variable (replace all occurrences)", kind: "refactor.extract.variable", raw: JSON.stringify({ path, edits }) },
          { title: "Extract to local variable", kind: "refactor.extract.variable", raw: JSON.stringify({ path, edits }) },
        ];
      }
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
      const it = JSON.parse(String(a.raw)) as { path: string; import?: string; finalAt?: number; edits?: unknown[] };
      if (it.edits) return { edits: it.edits, outcome: null };
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
