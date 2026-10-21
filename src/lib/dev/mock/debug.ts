/**
 * 桩：断点调试（docs/IDEA.md 第 18 节），对应 `src-tauri/src/commands/debug.rs`（规则在 `src-tauri/src/debug.rs`）。
 *
 * 照真实现的次序走、**故意走一阵**（frontend.md「桩要能停在中间一会儿」）：等编译开端口 0.8 秒 → 等语言服务器 0.6 秒 → 在跑；
 * 这个项目有断点的话 1 秒后停在第一个断点上（真的要等请求进来 / 启动代码跑到那儿）。单步 150ms 后停在下一行，继续之后「在跑」，
 * 任务停了会话跟着结束（`mockDebugEnd`，tasks.ts 的 emitExit 叫它）。
 *
 * 不模拟的：真的语义。调用栈是三帧（断点那一帧、入口类的 `run`、一帧 jar 里的 SpringApplication），变量是固定的几个。
 */
import { type A, FILES, NOT_MINE } from "./data";
import { mockEmit } from "./events";

type Phase = "waiting" | "running" | "paused" | "ended";
interface Sess {
  run: number;
  root: string;
  main: string;
  phase: Phase;
  note: string;
  at: { path: string; line: number } | null;
  timers: ReturnType<typeof setTimeout>[];
  /** 程序跑完了（单测：没撞上断点、或者停过之后按了继续）—— 任务那边报退出。Spring 应用不给：它一直跑到你停 */
  done?: () => void;
}
const SESS = new Map<number, Sess>();
/** 项目根 → 文件 → 行 */
const BPS = new Map<string, Map<string, number[]>>();
/** 条件算不了的断点（`路径:行` → 停下时那句提醒），同 Rust 的 `cond_warning` */
const WARN = new Map<string, string>();

const JAR = "jdt://contents/spring-boot-4.0.1.jar/org.springframework.boot/SpringApplication.class?=mock";

function state(s: Sess) {
  const paused = s.phase === "paused" && s.at;
  const cls = (p: string) => p.slice(p.lastIndexOf("/") + 1).replace(/\.java$/, "");
  const frames = paused
    ? [
        { id: 1, name: `${cls(s.at!.path)}.show()`, path: s.at!.path, label: "", line: s.at!.line },
        { id: 2, name: `${cls(s.main)}.run(String[])`, path: s.main, label: "", line: 12 },
        { id: 3, name: "SpringApplication.callRunner(Runner,ApplicationArguments)", path: JAR, label: "org.springframework.boot.SpringApplication", line: 792 },
        { id: 4, name: "0x000000b001233c50.accept(Object)", path: "", label: "", line: 0 },
      ]
    : [];
  return {
    run: s.run,
    phase: s.phase,
    note: s.note,
    thread: paused ? 1 : null,
    reason: paused ? "breakpoint" : "",
    frames,
    totalFrames: paused ? 24 : 0,
    threads: paused ? [{ id: 1, name: "main" }] : [],
  };
}

function set(s: Sess, phase: Phase, note = "") {
  s.phase = phase;
  s.note = note;
  mockEmit("debug-state", state(s));
}

function later(s: Sess, ms: number, f: () => void) {
  s.timers.push(setTimeout(f, ms));
}

/** 这个项目里的第一个断点（按路径排） */
function firstBp(root: string): { path: string; line: number } | null {
  const m = BPS.get(root);
  if (!m) return null;
  const [path, lines] = [...m].filter(([, l]) => l.length).sort(([a], [b]) => a.localeCompare(b))[0] ?? [];
  return path ? { path, line: Math.min(...lines!) } : null;
}

/** task_run 带了 debug（Spring Boot 应用，或者单测 —— 单测给 `done`：跑完了任务那边报退出）：起会话 */
export function mockDebugStart(run: number, root: string, mainClass: string, done?: () => void) {
  const main = Object.keys(FILES).find((p) => p.startsWith(`${root}/`) && p.endsWith(`/${mainClass.replace(/\./g, "/")}.java`)) ?? `${root}/${mainClass}.java`;
  const s: Sess = { run, root, main, phase: "waiting", note: "等它编译完、开调试端口…", at: null, timers: [], done };
  SESS.set(run, s);
  set(s, "waiting", s.note);
  later(s, 800, () => set(s, "waiting", "等 Java 语言服务器导入完…"));
  later(s, 1400, () => {
    set(s, "running");
    const bp = firstBp(root);
    if (bp) later(s, 1000, () => pauseAt(s, bp));
    else if (s.done) later(s, 1000, s.done);
  });
}

function pauseAt(s: Sess, at: { path: string; line: number }) {
  if (s.phase === "ended") return;
  s.at = at;
  // 真插件条件算不了时照样停、先发一句提醒，Rust 把它挂在这次停下的 note 上
  set(s, "paused", WARN.get(`${at.path}:${at.line}`) ?? "");
}

/** 任务退了（停了 / 自己退了）：会话跟着结束 */
export function mockDebugEnd(run: number) {
  const s = SESS.get(run);
  if (!s || s.phase === "ended") return;
  for (const t of s.timers) clearTimeout(t);
  s.at = null;
  set(s, "ended");
  SESS.delete(run);
}

const VARS: Record<number, { name: string; value: string; ty: string; reference: number; indexed: number }[]> = {
  // 第 1 帧的局部变量
  [-1]: [
    { name: "this", value: 'Money@21 "Money[cents=12444]"', ty: "Money", reference: 10, indexed: 0 },
    { name: "items", value: "ImmutableCollections$List12@33 size=2", ty: "List12", reference: 11, indexed: 2 },
    { name: "total", value: "12444", ty: "long", reference: 0, indexed: 0 },
    { name: "note", value: '"满 100 减 10"', ty: "String", reference: 0, indexed: 0 },
  ],
  [-2]: [{ name: "args", value: "String[0]@8", ty: "String[]", reference: 0, indexed: 0 }],
  10: [{ name: "cents", value: "12444", ty: "long", reference: 0, indexed: 0 }],
  11: [
    { name: "[0]", value: 'Money@41 "Money[cents=12345]"', ty: "Money", reference: 12, indexed: 0 },
    { name: "[1]", value: 'Money@42 "Money[cents=99]"', ty: "Money", reference: 13, indexed: 0 },
  ],
  12: [{ name: "cents", value: "12345", ty: "long", reference: 0, indexed: 0 }],
  13: [{ name: "cents", value: "99", ty: "long", reference: 0, indexed: 0 }],
};

export function debugCmd(cmd: string, a: A): unknown {
  switch (cmd) {
    case "debug_state": {
      const s = SESS.get(Number(a.run));
      return s ? state(s) : null;
    }
    case "debug_breakpoints": {
      const root = String(a.root);
      const lines = (a.lines as unknown as number[]) ?? [];
      const conds = (a.conds as unknown as string[]) ?? [];
      // 桩不会算 Java 表达式：条件写成 `false` 的那个永远不停（浏览器里看得出条件真的带下去了）；
      // 带 `nosuch` 的当成变量名打错 —— 照样停、带一句提醒（真插件就是这样）；别的条件当成真
      const live = lines.filter((_, i) => (conds[i] ?? "").trim() !== "false");
      lines.forEach((l, i) => {
        const c = (conds[i] ?? "").trim();
        if (c.includes("nosuch")) WARN.set(`${String(a.path)}:${l}`, `条件 ${c} 算不了，照样停下了：编译不过：nosuch cannot be resolved to a variable`);
        else WARN.delete(`${String(a.path)}:${l}`);
      });
      const m = BPS.get(root) ?? new Map<string, number[]>();
      if (live.length) m.set(String(a.path), live);
      else m.delete(String(a.path));
      BPS.set(root, m);
      return lines;
    }
    case "debug_step": {
      const s = SESS.get(Number(a.run));
      if (!s) throw new Error("这次运行没在调试");
      const action = String(a.action);
      if (action === "pause") {
        if (s.phase === "running") pauseAt(s, s.at ?? { path: s.main, line: 12 });
        return null;
      }
      if (s.phase !== "paused" || !s.at) return null;
      const at = s.at;
      set(s, "running");
      if (action !== "continue") later(s, 150, () => pauseAt(s, { path: at.path, line: at.line + (action === "stepOut" ? 3 : 1) }));
      else if (s.done) later(s, 300, s.done);
      return null;
    }
    case "debug_thread": {
      const s = SESS.get(Number(a.run));
      if (!s) throw new Error("这次运行没在调试");
      return state(s);
    }
    case "debug_vars": {
      if (!SESS.has(Number(a.run))) throw new Error("这次运行没在调试");
      const key = a.reference != null ? Number(a.reference) : -Number(a.frame);
      return { vars: VARS[key] ?? [], more: 0 };
    }
    // 桩里没有 JVM：有会话就当「一个都没变」，没有就是没在调试
    case "debug_hotswap":
      return { kind: SESS.size ? "unchanged" : "nosession", classes: [], message: "", lost: [] };
    case "debug_eval": {
      if (!SESS.has(Number(a.run))) throw new Error("这次运行没在调试");
      const expr = String(a.expr);
      if (expr.includes("lll")) throw new Error(`Cannot evaluate because of compilation error(s): ${expr} cannot be resolved.`);
      return { name: expr, value: "125", ty: "long", reference: 0, indexed: 0 };
    }
    default:
      return NOT_MINE;
  }
}
