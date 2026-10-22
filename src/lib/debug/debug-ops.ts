/**
 * 调试的动作（docs/IDEA.md 第 18 节）：调试着起、听会话的状态、停下时把编辑器带到那一行、继续 / 单步 / 暂停。懒加载 ——
 * 第一次调试、或者按 F7 / F8 / F9 才拉（菜单动作和 runs-ops 都是动态引它，入口里不多一个字节）。
 *
 * 状态的主人是 Rust（`debug.rs`）：每次变了整份推过来（"debug-state"），这里只是放进那一格运行（`RunTab.debug`）。
 * 停下时做的三件事同 IDEA：标出执行点、在编辑区打开那个文件跳到那一行、亮出调试那一格所在的工具窗。
 */
import { runs, type RunTab } from "../state/runs.svelte";
import { layout } from "../state/layout.svelte";
import { notify } from "../state/notify.svelte";
import { project } from "../state/project.svelte";
import { nav } from "../state/nav.svelte";
import { listenHere, type DebugFrame, type DebugState } from "../ipc/commands";
import { debugRunTo, debugState, debugStep, debugVars, type DebugAction } from "../ipc/debug";
import { tabs } from "../state/tabs.svelte";
import { docs } from "../state/docs.svelte";
import { settings } from "../state/settings.svelte";
import { countOf, syncAll } from "./breakpoints";
import { execPoint, setExecPoint } from "./exec-point";

let listening: Promise<unknown> | null = null;
/** 那一格还没建好时先到的状态（Rust 一起来就发，比 `task_run` 的回包早） */
const early = new Map<number, DebugState>();
/** 最近一次停下的那次运行：F8 们作用在它上面（Services 和运行窗里都没选中调试的那一格时） */
let lastPaused: number | null = null;
/** 执行点是哪次运行的：它接着跑了 / 结束了才清（别的会话的状态不该把它清掉） */
let execOwner: number | null = null;

function listen() {
  if (!listening) hotswapOnSave();
  listening ??= listenHere<DebugState>("debug-state", (e) => apply(e.payload));
  return listening;
}

/**
 * 调试时存盘就自动构建、热替换（docs/TAKEOVER.md I6；设置 `java.hotswapOnSave`，默认开）：存了 .java、这个项目在调试，停手 0.8 秒构建（⌘F9 那条路）——
 * 构建成功本来就会自动热替换、说一句换了哪几个类（`runs-ops` 的 `loadProblems`）。IDEA 是存完问一句「要重新加载吗」，这里不问：不打断。
 * 等 0.8 秒是为了连着存好几个文件（⌥⌘S 全部保存、改名改了几个文件）只构建一次；已经在构建就不再起一个（那一次会把这次的改动一起编进去）。
 * 挂在保存钩子后面而不是写在保存里：保存在入口包里，这段跟着调试的懒块走，第一次调试时才装
 */
let hotTimer: ReturnType<typeof setTimeout> | null = null;
function hotswapOnSave() {
  const prev = docs.hooks.afterSave;
  docs.hooks.afterSave = (path) => {
    prev?.(path);
    const root = project.root;
    if (!path?.endsWith(".java") || !root || !settings.v.javaHotswapOnSave) return;
    if (!runs.list.some((r) => r.root === root && (r.debug?.phase === "running" || r.debug?.phase === "paused"))) return;
    if (hotTimer) clearTimeout(hotTimer);
    hotTimer = setTimeout(() => {
      hotTimer = null;
      if (runs.list.some((r) => r.root === root && (r.name === "构建项目" || r.name === "重新构建项目") && r.status === "running")) return;
      void import("../state/runs-ops").then((m) => m.buildProject(false));
    }, 800);
  };
}

const tab = (id: number) => runs.list.find((t) => t.id === id);

/** 调试着起一个任务（Services 的虫子、⌃D）。先把这个项目的断点交给 Rust —— 重启之后它那边是空的 */
export async function debugTask(name: string, root: string | null = project.root) {
  if (!root) return;
  await listen();
  await syncAll(root);
  if (!countOf(root)) notify.hint("还没有断点：点 Java 文件的行号下一个（⌘F8），调试着的时候也能下");
  const { runTask } = await import("../state/runs-ops");
  await runTask(name, root, true);
}

/** runs-ops 建好那一格之后叫：先到的状态补上；没先到的问一次（事件有可能在挂上监听之前就发了） */
export async function adopt(id: number) {
  await listen();
  const got = early.get(id) ?? (await debugState(id).catch(() => null));
  early.delete(id);
  if (got) apply(got);
}

function apply(s: DebugState) {
  const r = tab(s.run);
  if (!r) {
    early.set(s.run, s);
    return;
  }
  const was = r.debug?.phase;
  r.debug = s;
  if (s.phase === "paused") {
    lastPaused = s.run;
    showFrame(r, s.frames[0] ?? null, true);
    // 亮出它所在的工具窗（同 IDEA：撞上断点时调试窗自己跳到前面）
    layout.panelView = r.service ? "services" : "run";
    layout.panel = true;
    if (!r.service) runs.activeId = r.id;
    return;
  }
  if (execOwner === s.run) {
    setExecPoint(null);
    execOwner = null;
  }
  if (lastPaused === s.run) lastPaused = null;
  // 挂不上 / 半路断了：原因说出来（正常结束是空的；程序自己退了的话任务那边会说退出码）
  if (s.phase === "ended" && s.note && was !== "ended") notify.fail(`调试：${s.note}`, 8000);
}

/**
 * 看这一帧：标出执行点、在编辑区打开它。`top`：是停在这儿的那一帧（选了下面的帧时淡一档）。
 * jar 里的类开成只读标签（⌘B 进 jar 用的同一条路），没有源码的帧（lambda 生成的类）只说一句
 */
export function showFrame(r: RunTab, f: DebugFrame | null, top: boolean) {
  if (!f || !f.path) {
    if (f) notify.hint(`${f.name} 没有源码（运行时生成的类）`);
    return;
  }
  execOwner = r.id;
  const p = { path: f.path, line: f.line, top };
  setExecPoint(p);
  // 行内变量值：这一帧的局部变量问回来补上（问的这一会儿换了帧 / 接着跑了就不补）
  void debugVars(r.id, { frame: f.id })
    .then((v) => {
      const now = execPoint();
      if (now && now.path === p.path && now.line === p.line && now.top === p.top) setExecPoint({ ...p, vals: v.vars.map((x) => ({ name: x.name, value: x.value })) });
    })
    .catch(() => {});
  if (f.path.startsWith("jdt://")) {
    void import("../state/tabflow-ops").then(async (m) => {
      if (await m.openClass(f.path, f.label || f.name)) nav.goto(f.line);
    });
  } else void nav.openAt(f.path, f.line, true);
}

/** F8 们作用在哪次运行上：Services / 运行窗里正看着的那一格是调试着的就是它，否则最近停下的那次 */
function target(): RunTab | undefined {
  const looking = layout.panel && layout.panelView === "run" ? tab(runs.activeId ?? -1) : undefined;
  if (looking?.debug && looking.debug.phase !== "ended") return looking;
  if (lastPaused !== null) return tab(lastPaused);
  return runs.list.find((t) => t.debug && (t.debug.phase === "running" || t.debug.phase === "paused"));
}

/**
 * 继续（F9）/ 单步跳过（F8）/ 单步进入（F7）/ 单步跳出（⇧F8）/ 暂停。没停着时按单步什么都不做 ——
 * 不弹提示：F8 是按惯了的，在没调试的时候按到也不是错
 */
export async function step(action: DebugAction, id?: number) {
  const r = id !== undefined ? tab(id) : target();
  if (!r?.debug) return;
  if (action === "pause" ? r.debug.phase !== "running" : r.debug.phase !== "paused") return;
  try {
    await debugStep(r.id, action);
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
  }
}

/**
 * 运行到光标（⌥F9，docs/TAKEOVER.md G14）：停着的那次调试跑到当前标签光标那一行再停。Rust 那边是临时断点 + 继续（`debug::Debugs::run_to` 写了为什么）。
 * 光标取编辑器此刻的（`docs.viewOf` 问活着的编辑器），不取上次换行时记的
 */
export async function runToCursor() {
  const r = target();
  const t = tabs.active;
  if (!r?.debug || r.debug.phase !== "paused") return void notify.hint("没有停着的调试：运行到光标要先停下来（撞上断点、或者暂停）");
  if (!t || t.mode !== "edit" || !t.path.endsWith(".java")) return void notify.hint("光标要在一个 Java 文件里");
  const line = docs.viewOf(t)?.line;
  if (!line) return;
  try {
    await debugRunTo(r.id, t.path, line);
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
  }
}

/** 执行点现在是不是这一格的（调试面板里「回到执行点」用） */
export const ownsExec = (id: number) => execOwner === id && !!execPoint();
