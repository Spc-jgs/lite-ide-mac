/**
 * 桩：任务（#48），对应 `src-tauri/src/commands/tasks.rs`（规则在 `src-tauri/src/taskdefs.rs`）。
 *
 * 照真实现的规则走，从 `FILES` 里读而不是写死一份列表：`.lite-ide/tasks.json`（注释、末尾逗号照样认）在前，
 * 然后根目录和一层子目录的 package.json scripts（文件里的顺序；生命周期钩子不列；`packageManager` 声明 → 锁文件 → npm），
 * 同名的以 tasks.json 为准。不模拟的：「cwd 跑出项目」那几条校验的措辞（只报一句「跳过了」）；建出来的 tasks.json 不进文件树的目录清单。
 *
 * 跑：不起进程，往 `FILES` 里写一份假输出（`open_log` 从 `FILES` 读），名字里带 `fail` 的 0.9 秒后「失败退出」（退出码 1），
 * 别的一直「在跑」直到停。停：故意在「停止中」停 600ms 再报退出 —— 真实现软停要等进程收尾，围着「停止中」做的界面
 * 在 0ms 返回的桩上看不见（frontend.md「桩要能停在中间一会儿」）；停止中再停一次立刻报退出（强杀）。
 * 输出不会长（桩的日志源是打开那一刻的快照），跟随在桩上验不到 —— 要在真 .app 上看。
 */
import { type A, bump, FILES, NOT_MINE, sleep } from "./data";
import { mockEmit } from "./events";
import { mockDebugEnd, mockDebugStart } from "./debug";
import { mockUserConfigs } from "./settings";

const FILE = ".lite-ide/tasks.json";
const TEMPLATE = `// lite-ide 的任务（docs/USAGE.md「任务」）。⌃⌥R 选一个跑，⌃R 再跑上一个，⌘F2 停。
// 每个任务：name（列表里显示的名字）、command（一整行，交给你的登录 shell，和在终端里敲一样）、
// cwd（相对项目根，可以不写）、env（额外的环境变量，可以不写）。
// package.json 里的 scripts 不用写在这儿，会自动列出来；同名的以这里为准。
[
  // { "name": "后端", "command": "mvn spring-boot:run", "cwd": "admin" },
  // { "name": "前端", "command": "pnpm dev", "cwd": "web" },
  // { "name": "脚本", "command": "python3 main.py", "env": { "APP_ENV": "dev" } },
]
`;
const LIFECYCLE = new Set(["preinstall", "install", "postinstall", "prepublish", "preprepare", "prepare", "postprepare", "prepublishOnly", "prepack", "postpack", "publish", "postpublish", "preversion", "version", "postversion", "dependencies"]);

type Def = { name: string; command: string; cwd: string; source: "file" | "package" | "spring" | "goal" | "user"; module?: string; kind?: string; items?: string[] };
/** Lifecycle 底下的阶段（同 Rust 的 maven::PHASES） */
const PHASES = ["clean", "validate", "compile", "test", "package", "verify", "install", "site", "deploy"];
/** 能放进 mvn 命令行的词（同 Rust 的 maven::safe_token） */
const SAFE = /^[A-Za-z0-9_\-.:=,@/+%]+$/;
const MVN_PREFS = new Map<string, { offline: boolean; skipTests: boolean; profiles: string[] }>();

/** 和 Rust 的 settings::strip 同一件事的简化版：去掉 // 注释和末尾逗号（不管字符串里的 //，桩里的数据没有那种） */
const jsonc = (t: string) => JSON.parse(t.replace(/\/\/[^\n]*/g, "").replace(/,(\s*[\]}])/g, "$1") || "[]");

const word = (s: string) => (/^[A-Za-z0-9_\-:./@+=,%]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`);

function pmOf(dir: string, root: string, declared?: string): string {
  const d = declared?.split("@")[0];
  if (d && ["pnpm", "yarn", "npm", "bun"].includes(d)) return d;
  for (let cur = dir; ; cur = cur.slice(0, cur.lastIndexOf("/"))) {
    for (const [lock, pm] of [["pnpm-lock.yaml", "pnpm"], ["yarn.lock", "yarn"], ["bun.lock", "bun"], ["bun.lockb", "bun"], ["package-lock.json", "npm"]]) {
      if (`${cur}/${lock}` in FILES) return pm;
    }
    if (cur === root || !cur.includes("/")) return "npm";
  }
}

/** 运行配置的桩（mock/settings.ts）也要这份服务列表 */
export const mockTaskList = (root: string) => list(root);

function list(root: string) {
  const defs: Def[] = [];
  const problems: string[] = [];
  const file = `${root}/${FILE}` in FILES;
  if (file) {
    try {
      const arr = jsonc(FILES[`${root}/${FILE}`]);
      if (!Array.isArray(arr)) problems.push(`${FILE} 最外层要是 [ … ]（任务的列表）`);
      else
        arr.forEach((t: { name?: string; command?: string; cwd?: string }, i: number) => {
          if (!t?.name?.trim() || !t?.command?.trim()) return void problems.push(`${FILE} 第 ${i + 1} 个任务缺 name 或 command（都要是非空的字符串），跳过了`);
          if (defs.some((d) => d.name === t.name)) return void problems.push(`${FILE} 里的「${t.name}」 重名了，用的是前面那个`);
          defs.push({ name: t.name.trim(), command: t.command.trim(), cwd: (t.cwd ?? "").replace(/^\.\/|\/$/g, ""), source: "file" });
        });
    } catch (e) {
      problems.push(`${FILE} 写错了：${(e as Error).message}`);
    }
  }
  const pkgs = Object.keys(FILES)
    .filter((p) => p === `${root}/package.json` || new RegExp(`^${root}/[^/.][^/]*/package\\.json$`).test(p))
    .filter((p) => !p.includes("/node_modules/"))
    .sort((a, b) => (a === `${root}/package.json` ? -1 : b === `${root}/package.json` ? 1 : a.localeCompare(b)));
  for (const p of pkgs) {
    const dir = p.slice(0, -"/package.json".length);
    const rel = dir === root ? "" : dir.slice(root.length + 1);
    let pj: { scripts?: Record<string, unknown>; packageManager?: string };
    try {
      pj = JSON.parse(FILES[p]);
    } catch (e) {
      problems.push(`${rel ? `${rel}/` : ""}package.json 解析不了：${(e as Error).message}`);
      continue;
    }
    // JS 对象按插入顺序遍历（非数字键）—— 和 Rust 那边特意保住的文件顺序一致
    const names = Object.keys(pj.scripts ?? {}).filter((k) => typeof pj.scripts![k] === "string");
    const hook = (k: string) => ["pre", "post"].some((pre) => k.startsWith(pre) && names.includes(k.slice(pre.length)));
    const pm = pmOf(dir, root, pj.packageManager);
    for (const s of names.filter((k) => !LIFECYCLE.has(k) && !hook(k))) {
      const name = rel ? `${rel}/${s}` : s;
      if (!defs.some((d) => d.name === name)) defs.push({ name, command: `${pm} run ${word(s)}`, cwd: rel, source: "package" });
    }
  }
  // Spring Boot 入口（真实现在 maven.rs）：项目根有 pom.xml 时，各模块 src/main/java 里带 @SpringBootApplication 又有 main 的类。
  // 桩不读 <modules>，按「某个目录底下的 src/main/java」认模块 —— 演示数据里够用；命令的样子照真实现
  if (FILES[`${root}/pom.xml`] !== undefined) {
    const apps = Object.keys(FILES)
      .filter((p) => p.startsWith(`${root}/`) && p.endsWith(".java") && p.includes("/src/main/java/"))
      .filter((p) => FILES[p].includes("@SpringBootApplication") && FILES[p].includes("static void main"))
      .sort();
    for (const p of apps) {
      const mod = p.slice(root.length + 1, p.indexOf("/src/main/java/"));
      const pkg = /^package\s+([\w.]+);/m.exec(FILES[p])?.[1];
      const cls = p.slice(p.lastIndexOf("/") + 1, -".java".length);
      const main = pkg ? `${pkg}.${cls}` : cls;
      if (defs.some((d) => d.name === cls)) continue;
      const cp = "target/.lite-ide-cp.txt";
      defs.push({
        name: cls,
        command: `mvn -B -ntp -pl ${mod} -am -DskipTests compile dependency:build-classpath -Dmdep.outputFile=${cp} && cd ${mod} && exec java -cp "target/classes:$(cat ${cp})" ${main}`,
        cwd: "",
        source: "spring",
        module: mod,
      });
    }
  }
  // 你建的运行配置（真实现在 taskdefs::user_defs）：拼法照真实现
  for (const u of mockUserConfigs(root)) {
    if (defs.some((d) => d.name === u.name)) {
      problems.push(`你建的运行配置「${u.name}」和别处的同名，没列 —— 改个名`);
      continue;
    }
    const mod = u.module;
    const cp = "target/.lite-ide-cp.txt";
    const command =
      u.kind === "application"
        ? `mvn -B -ntp${mod ? ` -pl ${mod} -am` : ""} -DskipTests compile dependency:build-classpath -Dmdep.outputFile=${cp} && ${mod ? `cd ${mod} && ` : ""}exec java${u.vmOptions ? ` ${u.vmOptions}` : ""} -cp "target/classes:$(cat ${cp})" ${u.mainClass}${u.programArgs ? ` ${u.programArgs}` : ""}`
        : u.kind === "maven"
          ? `mvn -B -ntp${mod ? ` -pl ${mod}` : ""} ${u.goals}`
          : u.kind === "shell"
            ? u.command
            : u.kind === "remote"
              ? `printf '挂到 %s:%s 的 JVM 上调试（⌘F2 断开，不会停掉那个 JVM）\\n' '${u.host}' '${u.port}'; exec tail -f /dev/null`
              : "";
    defs.push({ name: u.name, command, cwd: u.kind === "shell" ? u.cwd : "", source: "user", kind: u.kind, items: u.kind === "compound" ? u.items : [] });
  }
  return { tasks: defs.map((d) => ({ kind: "", items: [], ...d })), file, problems };
}

type Run = { root: string; name: string; status: "running" | "stopping" | "gone"; timer?: ReturnType<typeof setTimeout>; service?: boolean; at?: number };
const RUNS = new Map<number, Run>();
let nextRun = 1;

type Holder = { port: number; pid: number; command: string; ours: string | null };
/** 端口被谁占着（桩里的假占用者）。8080 一开始就被占着；`task_free_port` 清掉它，之后带 fail 的任务就能正常起来（同真实现） */
const HOLDERS = new Map<number, Holder>([[8080, { port: 8080, pid: 41237, command: "java", ours: null }]]);

/** 桩里一个服务在听的端口：跑起来 2.5 秒之后才有（见 services_list） */
function mockPorts(id: number): number[] {
  const r = RUNS.get(id);
  if (!r?.service || r.status === "gone" || Date.now() - (r.at ?? 0) < 2500) return [];
  const services = list(r.root).tasks.filter((d) => d.source === "spring");
  return [8080 + Math.max(0, services.findIndex((d) => d.name === r.name))];
}

/** 桩里跑一个测试类要多久 */
const CLASS_MS = 1200;
/** 跑过的单测：任务名 → 选择器（test_results 按它造结果） */
const TESTS = new Map<string, string>();

/** 桩的测试结果：选择器里的类（`A#x+y,B`）到 FILES 里找源码，认 @Test 方法（同 java-tests.ts 的认法，粗一点）；名字带 fail 的算没过 */
function testCases(root: string, selector: string) {
  const out: { class: string; name: string; ms: number; outcome: string; message: string; detail: string }[] = [];
  for (const part of selector.split(",")) {
    const [cls, ms] = part.split("#");
    const file = Object.keys(FILES).find((p) => p.startsWith(`${root}/`) && p.endsWith(`/src/test/java/${cls.replace(/\./g, "/")}.java`));
    if (!file) continue;
    const methods = [...FILES[file].matchAll(/@Test\s+(?:public\s+)?void\s+(\w+)\s*\(/g)].map((m) => m[1]).filter((m) => !ms || ms.split("+").includes(m));
    for (const m of methods) {
      const bad = /fail/i.test(m);
      out.push({
        class: cls,
        name: m,
        ms: bad ? 3 : 11,
        outcome: bad ? "failed" : "passed",
        message: bad ? "expected: <3> but was: <2>" : "",
        detail: bad ? `org.opentest4j.AssertionFailedError: expected: <3> but was: <2>\n\tat ${cls}.${m}(${cls.slice(cls.lastIndexOf(".") + 1)}.java:16)` : "",
      });
    }
  }
  return out;
}

function emitExit(id: number, x: { code: number | null; stopped: boolean; port?: Holder | null }) {
  const r = RUNS.get(id);
  if (!r || r.status === "gone") return;
  r.status = "gone";
  clearTimeout(r.timer);
  mockDebugEnd(id);
  const payload = { id, code: x.code, signal: x.stopped ? 2 : null, stopped: x.stopped, failed: !x.stopped && x.code !== 0, port: x.port ?? null };
  mockEmit("task-exit", payload);
}

/**
 * 构建（⌘F9，docs/IDEA.md 第 22 节）的假输出：项目里哪个 Java 文件写了 `nosuch`（照真实现撞出来的样子：中文的 javac、同一条出现两遍），
 * 那一行就报「找不到符号」；没有就是 BUILD SUCCESS。浏览器里把某个文件改出一个 nosuch、保存、⌘F9 就看得到错误树
 */
function buildOutput(root: string, command: string): { text: string; failed: boolean } {
  const bad: string[] = [];
  for (const [p, text] of Object.entries(FILES)) {
    if (!p.startsWith(`${root}/`) || !p.endsWith(".java") || typeof text !== "string") continue;
    text.split("\n").forEach((l, i) => {
      const c = l.indexOf("nosuch");
      if (c >= 0) bad.push(`[ERROR] ${p}:[${i + 1},${c + 1}] 找不到符号\n  符号:   变量 nosuch\n  位置: 类 ${p.slice(p.lastIndexOf("/") + 1, -5)}`);
    });
  }
  const head = `$ ${command}\n[INFO] Scanning for projects...\n[INFO] --- maven-compiler-plugin:3.14.1:compile (default-compile) ---\n`;
  if (!bad.length) return { text: `${head}[INFO] Nothing to compile - all classes are up to date.\n[INFO] BUILD SUCCESS\n`, failed: false };
  const again = bad.map((b) => b.replace(/\n {2}/g, "\n[ERROR]   ")).join("\n");
  return { text: `${head}[ERROR] COMPILATION ERROR : \n${bad.join("\n")}\n[INFO] BUILD FAILURE\n[ERROR] Failed to execute goal ...: Compilation failure\n${again}\n`, failed: true };
}

/** 同 `javac.rs` 的 parse：认 `[ERROR] /路径.java:[行,列] 消息`，缩进两格（或 `[ERROR]   `）的是补充；同一条出现两遍只算一条 */
function parseProblems(text: string) {
  const items: { path: string; line: number; col: number; error: boolean; message: string }[] = [];
  const seen = new Set<string>();
  let cur: (typeof items)[number] | null | undefined;
  for (const line of text.split("\n")) {
    const m = /^\[(ERROR|WARNING)\] (\/.+\.java):\[(\d+),(\d+)\] ?(.*)$/.exec(line);
    if (m) {
      const key = `${m[2]}|${m[3]}|${m[4]}|${m[5]}`;
      cur = seen.has(key) ? null : { path: m[2], line: +m[3], col: +m[4], error: m[1] === "ERROR", message: m[5].trim() };
      seen.add(key);
      if (cur) items.push(cur);
      continue;
    }
    const more = /^(?:\[(?:ERROR|WARNING)\] {3}| {2})(.+)$/.exec(line);
    if (more && cur !== undefined) {
      if (cur) cur.message += `\n${more[1].trim()}`;
    } else cur = undefined;
  }
  return { items, total: items.length };
}

function fakeOutput(name: string, command: string, fail: boolean): string {
  const t = (s: number) => `2026-10-10T09:18:${String(20 + s).padStart(2, "0")}.000+08:00`;
  const head = `$ ${command}\n`;
  if (fail)
    return `${head}${t(0)}  INFO 52267 --- [main] demo.App : Starting App\n${t(1)} ERROR 52267 --- [main] o.s.boot.SpringApplication : Application run failed\njava.lang.IllegalStateException: Port 8080 was already in use.\n\tat demo.App.main(App.java:12)\n`;
  return `${head}\n  VITE v6.4.3  ready in 305 ms\n\n  ➜  Local:   http://localhost:5173/\n${t(2)}  INFO 1 --- [main] ${name} : started\n${t(3)}  WARN 1 --- [main] ${name} : 示范 WARN 行\n`;
}

export async function tasksCmd(cmd: string, a: A): Promise<unknown> {
  switch (cmd) {
    /*
     * Maven 工具窗（真实现：commands/tasks.rs 的 maven_project / maven_resolve / maven_prefs_set，规则在 maven.rs / maven_run.rs）。
     * 桩：根模块 + 第一层带 pom.xml 的目录（parent 都是根）；profile 读各 pom 的 <profiles>（同 maven::pom_profiles 的简化版）。
     * 解析插件和依赖**故意等 1.5 秒**：真实现要跑一次 mvn（几秒到几十秒），围着「正在解析」做的那两个节点在 0ms 的桩上看不见
     * （frontend.md「桩要能停在中间一会儿」）。开关存在内存里（真实现在 maven-prefs.json），刷新页面就没了
     */
    case "maven_project": {
      const root = String(a.root);
      const prefs = MVN_PREFS.get(root) ?? { offline: false, skipTests: false, profiles: [] as string[] };
      const base = { phases: PHASES, offline: prefs.offline, skipTests: prefs.skipTests };
      if (FILES[`${root}/pom.xml`] === undefined) return { reactors: [], profiles: [], ...base };
      const artifact = (p: string) => /<artifactId>([^<]+)<\/artifactId>/.exec((FILES[p] ?? "").replace(/<parent>[\s\S]*?<\/parent>|<dependencies>[\s\S]*?<\/dependencies>/g, ""))?.[1] ?? "";
      const rels: string[] = [];
      for (const p of Object.keys(FILES)) {
        const m = new RegExp(`^${root}/([^/]+)/pom\\.xml$`).exec(p);
        if (m) rels.push(m[1]);
      }
      const modules = [
        { rel: "", artifactId: artifact(`${root}/pom.xml`), parent: null },
        ...rels.sort().map((rel) => ({ rel, artifactId: artifact(`${root}/${rel}/pom.xml`) || rel, parent: "" })),
      ];
      const profiles: { id: string; activeByDefault: boolean; on: boolean }[] = [];
      for (const m of modules) {
        const text = FILES[`${root}/${m.rel ? `${m.rel}/` : ""}pom.xml`] ?? "";
        for (const p of text.matchAll(/<profile>([\s\S]*?)<\/profile>/g)) {
          const id = /<id>([^<]+)<\/id>/.exec(p[1])?.[1];
          if (id && !profiles.some((x) => x.id === id)) profiles.push({ id, activeByDefault: p[1].includes("<activeByDefault>true"), on: prefs.profiles.includes(id) });
        }
      }
      return { reactors: [{ rel: "", modules }], profiles, ...base };
    }
    case "maven_resolve": {
      const root = String(a.root);
      await new Promise((r) => setTimeout(r, 1500));
      if (FILES[`${root}/pom.xml`] === undefined) return { reactors: [] };
      const plugins = [
        { groupId: "org.apache.maven.plugins", artifactId: "maven-compiler-plugin", version: "3.14.0", prefix: "compiler", goals: ["compile", "testCompile", "help"] },
        { groupId: "org.apache.maven.plugins", artifactId: "maven-surefire-plugin", version: "3.5.2", prefix: "surefire", goals: ["test", "help"] },
        { groupId: "org.springframework.boot", artifactId: "spring-boot-maven-plugin", version: "4.0.1", prefix: "spring-boot", goals: ["run", "repackage", "start", "stop", "build-image", "build-info"] },
      ];
      const deps = [
        { depth: 0, groupId: "org.springframework.boot", artifactId: "spring-boot-starter-web", version: "4.0.1", scope: "compile", optional: false },
        { depth: 1, groupId: "org.springframework.boot", artifactId: "spring-boot-starter", version: "4.0.1", scope: "compile", optional: false },
        { depth: 2, groupId: "org.springframework", artifactId: "spring-core", version: "7.0.1", scope: "compile", optional: false },
        { depth: 1, groupId: "org.springframework", artifactId: "spring-webmvc", version: "7.0.1", scope: "compile", optional: false },
        { depth: 0, groupId: "org.projectlombok", artifactId: "lombok", version: "1.18.42", scope: "provided", optional: true },
        { depth: 0, groupId: "org.springframework.boot", artifactId: "spring-boot-starter-test", version: "4.0.1", scope: "test", optional: false },
      ];
      const proj = (await tasksCmd("maven_project", { root } as unknown as A)) as { reactors: { rel: string; modules: { artifactId: string }[] }[] };
      return { reactors: proj.reactors.map((r) => ({ rel: r.rel, error: "", modules: r.modules.map((m, i) => {
            // 多模块时根模块（打包方式 pom）只有一两个插件、没有依赖，同真项目；只有根模块的项目给全套
            const full = i > 0 || r.modules.length === 1;
            return { artifactId: m.artifactId, plugins: full ? plugins : plugins.slice(0, 1), deps: full ? deps : [] };
          }) })) };
    }
    case "maven_prefs_set": {
      MVN_PREFS.set(String(a.root), { offline: !!a.offline, skipTests: !!a.skipTests, profiles: (a.profiles as unknown as string[]) ?? [] });
      return null;
    }
    case "task_run": {
      const root = String(a.root);
      const name = String(a.name);
      // Maven 工具窗的「artifactId [目标]」：不在列表里，按名字认（真实现在 taskdefs::maven_goal）。
      // 跑单测的「模块 [test 选择器]」同一个形状（第 17 节），选择器按 maven::safe_selector 的字符卡
      // 带覆盖率的「模块 [cover 选择器]」（G15）同一个形状：命令一样（真实现再挂 JaCoCo 的代理，桩里不挂）
      const test = /^(.+) \[(?:test|cover) ([\w.#$*+,]+)\]$/.exec(name);
      // 构建（⌘F9，第 22 节）：同 taskdefs::build，有 pom.xml 才认
      const build = (name === "构建项目" || name === "重新构建项目") && FILES[`${root}/pom.xml`] !== undefined;
      const goal = test ? null : /^(.+) \[([^\]]+)\]$/.exec(name);
      const def =
        list(root).tasks.find((d) => d.name === name) ??
        (goal && goal[2].split(/\s+/).every((t) => SAFE.test(t)) ? { name, command: `mvn -B -ntp ${goal[2]}`, cwd: "", source: "goal" as const } : undefined) ??
        (test ? { name, command: `mvn -B -ntp -Dtest=${test[2]} -Dsurefire.failIfNoSpecifiedTests=false -pl ${test[1]} -am test`, cwd: "", source: "goal" as const } : undefined) ??
        (build ? { name, command: `mvn -B -ntp ${name === "重新构建项目" ? "clean " : ""}test-compile`, cwd: "", source: "goal" as const } : undefined);
      if (!def) throw new Error(`没有叫「${name}」的任务了（tasks.json 改过？）`);
      // 远程 JVM 调试（G23）：同真实现，只能调试 —— ▶ 也是挂上去
      const remote = def.source === "user" && "kind" in def && def.kind === "remote";
      if (remote) a.debug = true as unknown as A[string];
      // 调试（第 18 节）：同真实现，只有认出来的 Spring Boot 应用、你建的应用配置、单测、远程
      if (a.debug && def.source !== "spring" && !test && !remote && !("kind" in def && def.kind === "application")) throw new Error(`「${name}」不是认出来的 Spring Boot 应用或单测，调试不了`);
      // 同 debug.rs 的 with_jdwp / with_surefire_debug：Spring 在 `exec java` 后面插调试代理，单测交给 surefire（运行窗的命令那一行照实显示）
      const JDWP = "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0";
      const command = !a.debug || remote ? def.command : test ? def.command.replace(/^mvn /, `mvn '-Dmaven.surefire.debug=${JDWP}' `) : def.command.replace("exec java ", `exec java ${JDWP} `);
      // 同真实现：同名的还在跑先停掉
      for (const [id, r] of RUNS) if (r.root === root && r.name === name && r.status !== "gone") emitExit(id, { code: 130, stopped: true });
      const id = nextRun++;
      // 形状同 `tasksvc::log_file`：换过字符的名字后面带一段原名的指纹（#56 第 2 条）。值不必和 Rust 算的一样，这里是 32 位 FNV-1a
      let h = 0x811c9dc5;
      for (const b of new TextEncoder().encode(name)) h = Math.imul(h ^ b, 0x01000193) >>> 0;
      const log = `/mock-runs/${name.replace(/[^\w.-]/g, "_")}-${h.toString(16).padStart(8, "0")}.log`;
      // 名字带 fail 的：8080 还被占着就 0.9 秒后失败退出（输出里那句 `Port 8080 was already in use`）。
      // 带 ours 的，占着的算「你之前跑的」（主按钮）；不带的是别的程序（danger）—— 两种卡片在浏览器里都看得到
      const fail = !test && name.includes("fail") && HOLDERS.has(8080);
      const built = build ? buildOutput(root, command) : null;
      FILES[log] = built ? built.text : fakeOutput(name, command, fail);
      bump(log);
      const run: Run = { root, name, status: "running", service: def.source === "spring", at: Date.now() };
      RUNS.set(id, run);
      if (fail) {
        const h = { ...HOLDERS.get(8080)!, ours: name.includes("ours") ? "后端" : null };
        run.timer = setTimeout(() => emitExit(id, { code: 1, stopped: false, port: h }), 900);
      }
      // 跑单测：每个测试类 1.2 秒（真的 mvn 要编译再跑，几秒到几十秒；结果树的「正在跑」、一个类跑完亮一个（G15）要看得见）；有名字带 fail 的方法就是没过（退出码 1）
      const testExit = () => emitExit(id, { code: test && testCases(root, test[2]).some((c) => c.outcome === "failed") ? 1 : 0, stopped: false });
      if (test) {
        TESTS.set(name, test[2]);
        // 调试着跑的：停在断点上那段时间不退（被调试的 JVM 停着），跑完由调试那边说
        if (!a.debug) run.timer = setTimeout(testExit, CLASS_MS * Math.max(1, new Set(testCases(root, test[2]).map((c) => c.class)).size));
      }
      // 构建：1.5 秒（真的 mvn 起来就要一两秒，「正在构建」要看得见）
      if (built) run.timer = setTimeout(() => emitExit(id, { code: built.failed ? 1 : 0, stopped: false }), 1500);
      if (a.debug && test) mockDebugStart(id, root, test[2].split(/[#+,]/)[0], testExit);
      else if (a.debug) mockDebugStart(id, root, /(\S+)$/.exec(def.command)?.[1] ?? name);
      return { id, name, command, log, service: def.source === "spring", debug: !!a.debug };
    }
    /*
     * Services（真实现：commands/tasks.rs 的 services_list / task_ports）。端口**故意 2.5 秒后才有**：Spring Boot 起来要几秒，
     * 「跑着、还没端口」那段在 0ms 的桩上看不见（frontend.md「桩要能停在中间一会儿」）。第几个服务听 8080 + 几
     */
    case "services_list": {
      const root = String(a.root);
      return list(root)
        .tasks.filter((d) => d.source === "spring")
        .map((d) => {
          const id = [...RUNS].find(([, r]) => r.root === root && r.name === d.name && r.status !== "gone")?.[0];
          return { task: d.name, mainClass: /(\S+)$/.exec(d.command)?.[1] ?? d.name, module: d.module ?? "", ports: id === undefined ? [] : mockPorts(id) };
        });
    }
    case "task_ports":
      return mockPorts(Number(a.id));
    // 堆（真实现读 JVM 的性能计数）：端口有了才有（JVM 起来了），用量锯齿形涨落 —— 涨到一截 GC 一次掉下来，曲线看得出 GC
    case "task_heap":
      return (a.ids as number[]).flatMap((id) => {
        const r = RUNS.get(id);
        if (!mockPorts(id).length || !r) return [];
        const s = (Date.now() - (r.at ?? 0)) / 1000;
        const MB = 1 << 20;
        return [{ id, used: Math.round((60 + ((s * 9) % 140)) * MB), committed: 256 * MB, max: 4096 * MB, gcCount: Math.floor((s * 9) / 140), gcMs: Math.floor((s * 9) / 140) * 6 }];
      });
    case "build_problems":
      return parseProblems(String(FILES[String(a.log)] ?? ""));
    /*
     * 覆盖率报告（G15，真实现：JaCoCo 的命令行出 XML、commands/tasks.rs 的 coverage_report）。桩：项目里每个 src/main/java 下的 .java，
     * 有代码的行一半「跑到了」、名字带 Mapper 的全没跑到、第一个方法那行「跑到一部分」—— 编辑器三种颜色、工具窗的排序都看得到。等 600 ms（真的要起一个 JVM）
     */
    case "coverage_report": {
      await sleep(600);
      const root = String(a.root);
      const files = Object.keys(FILES)
        .filter((p) => p.startsWith(`${root}/`) && p.includes("/src/main/java/") && p.endsWith(".java"))
        .map((path) => {
          const code = FILES[path].split("\n").map((l, i) => [i + 1, l.trim()] as const).filter(([, l]) => l && !/^(package|import|\/\/|\*|\/\*|@|\}|\{)/.test(l)).map(([n]) => n);
          const none = path.includes("Mapper");
          const covered = none ? [] : code.filter((_, i) => i % 2 === 0);
          const partial = none || !code.length ? [] : [code[1] ?? code[0]].filter((n) => !covered.includes(n));
          const missed = code.filter((n) => !covered.includes(n) && !partial.includes(n));
          return { path, covered, partial, missed };
        })
        .filter((f) => f.covered.length + f.partial.length + f.missed.length > 0);
      const pct = (f: { covered: number[]; partial: number[]; missed: number[] }) => (f.covered.length + f.partial.length) / (f.covered.length + f.partial.length + f.missed.length);
      files.sort((x, y) => pct(x) - pct(y) || x.path.localeCompare(y.path));
      return { files, total: files.length };
    }
    case "test_results": {
      // 真实现读 surefire 报告；桩按选择器从 FILES 里的测试类造（testCases）。还在跑的时候同 surefire：跑完一个类才有那个类的报告（G15），
      // 这里按开跑以来过了几个 CLASS_MS 给前几个类
      const sel = TESTS.get(String(a.name));
      let cases = sel ? testCases(String(a.root), sel) : [];
      const live = [...RUNS.values()].find((r) => r.root === String(a.root) && r.name === String(a.name) && r.status === "running");
      if (live) {
        const done = new Set([...new Set(cases.map((c) => c.class))].slice(0, Math.floor((Date.now() - (live.at ?? 0)) / CLASS_MS)));
        cases = cases.filter((c) => done.has(c.class));
      }
      return { cases, total: cases.length };
    }
    case "task_stop": {
      const id = Number(a.id);
      const r = RUNS.get(id);
      if (!r || r.status === "gone") return "gone";
      // 造一个「停不掉」（#68）：控制台里 `__mockStuck = true` 再停 —— 组里有 root 的进程，软停、强杀都送不到，一直「正在停」
      const stuck = (globalThis as { __mockStuck?: boolean }).__mockStuck === true;
      if (r.status === "stopping") {
        if (stuck) return "killed";
        emitExit(id, { code: null, stopped: true });
        return "killed";
      }
      r.status = "stopping";
      if (stuck) return "stopping";
      // 名字带 quick 的退得快（vite 收到 SIGINT 几毫秒就退）：退出事件比这一条的回包先到 —— 真 .app 上撞见过这个次序
      if (r.name.includes("quick")) {
        emitExit(id, { code: 130, stopped: true });
        return "stopping";
      }
      r.timer = setTimeout(() => emitExit(id, { code: 130, stopped: true }), 600);
      return "stopping";
    }
    case "run_compound_items": {
      const u = mockUserConfigs(String(a.root)).find((x) => x.name === String(a.name) && x.kind === "compound");
      return u ? [...u.items] : null;
    }
    case "task_stuck":
      // 真实现枚举组员、挑出不归我们的（tasksvc::foreign_members）
      return (globalThis as { __mockStuck?: boolean }).__mockStuck === true && RUNS.get(Number(a.id))?.status === "stopping"
        ? [{ pid: 48213, user: "root", command: "/usr/local/bin/nginx" }]
        : [];
    case "task_free_port": {
      // 真实现要等那个进程收尾、端口空出来 —— 等一会儿，「结束它并重跑」那一下的等待在浏览器里看得见
      await new Promise((r) => setTimeout(r, 400));
      HOLDERS.delete(Number(a.port));
      return null;
    }
    case "task_stale": {
      // 造一个「上次没停干净」：控制台里 `__mockStale = [{ pgid: 1, name: "后端", command: "mvn spring-boot:run" }]`，再刷新
      const all = (globalThis as { __mockStale?: { pgid: number; name: string; command: string }[] }).__mockStale ?? [];
      return String(a.root) === "/proj" ? all : [];
    }
    case "task_stale_resolve":
      (globalThis as { __mockStale?: unknown[] }).__mockStale = [];
      return null;
    case "task_close": {
      const id = Number(a.id);
      const r = RUNS.get(id);
      if (r) clearTimeout(r.timer);
      RUNS.delete(id);
      return null;
    }
    case "task_list":
      return list(String(a.root));
    case "task_new_file": {
      const p = `${String(a.root)}/${FILE}`;
      // 同真实现：已经有了就不动（create_new）
      if (!(p in FILES)) {
        FILES[p] = TEMPLATE;
        bump(p);
      }
      return p;
    }
    default:
      return NOT_MINE;
  }
}
