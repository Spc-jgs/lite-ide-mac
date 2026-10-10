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
import { type A, bump, FILES, NOT_MINE } from "./data";

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

type Def = { name: string; command: string; cwd: string; source: "file" | "package" };

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
  return { tasks: defs, file, problems };
}

type Run = { root: string; name: string; status: "running" | "stopping" | "gone"; timer?: ReturnType<typeof setTimeout> };
const RUNS = new Map<number, Run>();
let nextRun = 1;
/** `task-exit` 的监听回调（`plugin:event|listen` 时记下的编号）。精确地发给它，不像 `__mockMenu` 那样广播 */
const exitCbs = new Set<number>();

function emitExit(id: number, x: { code: number | null; stopped: boolean }) {
  const r = RUNS.get(id);
  if (!r || r.status === "gone") return;
  r.status = "gone";
  clearTimeout(r.timer);
  const payload = { id, code: x.code, signal: x.stopped ? 2 : null, stopped: x.stopped, failed: !x.stopped && x.code !== 0 };
  for (const cb of exitCbs) {
    const f = (window as unknown as Record<string, unknown>)[`_cb${cb}`];
    if (typeof f === "function") (f as (e: unknown) => void)({ event: "task-exit", id: 0, payload });
  }
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
    case "plugin:event|listen":
      if ((a as unknown as { event: string }).event !== "task-exit") return NOT_MINE;
      exitCbs.add(Number((a as unknown as { handler: number }).handler));
      return Number((a as unknown as { handler: number }).handler);
    case "task_run": {
      const root = String(a.root);
      const name = String(a.name);
      const def = list(root).tasks.find((d) => d.name === name);
      if (!def) throw new Error(`没有叫「${name}」的任务了（tasks.json 改过？）`);
      // 同真实现：同名的还在跑先停掉
      for (const [id, r] of RUNS) if (r.root === root && r.name === name && r.status !== "gone") emitExit(id, { code: 130, stopped: true });
      const id = nextRun++;
      const log = `/mock-runs/${name.replace(/[^\w.-]/g, "_")}.log`;
      const fail = name.includes("fail");
      FILES[log] = fakeOutput(name, def.command, fail);
      bump(log);
      const run: Run = { root, name, status: "running" };
      RUNS.set(id, run);
      if (fail) run.timer = setTimeout(() => emitExit(id, { code: 1, stopped: false }), 900);
      return { id, name, command: def.command, log };
    }
    case "task_stop": {
      const id = Number(a.id);
      const r = RUNS.get(id);
      if (!r || r.status === "gone") return "gone";
      if (r.status === "stopping") {
        emitExit(id, { code: null, stopped: true });
        return "killed";
      }
      r.status = "stopping";
      r.timer = setTimeout(() => emitExit(id, { code: 130, stopped: true }), 600);
      return "stopping";
    }
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
