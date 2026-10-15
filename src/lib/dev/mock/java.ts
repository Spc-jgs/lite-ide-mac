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
import { type A, FILES, NOT_MINE } from "./data";
import { mockEmit } from "./events";

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
  return { root, phase, message: phase === "importing" ? "32% Project 'proj'" : "", detail: "JDK 21（桩）· Lombok lombok-1.18.46.jar" };
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
