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
import { debugState, debugStep, type DebugAction } from "../ipc/debug";
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
  listening ??= listenHere<DebugState>("debug-state", (e) => apply(e.payload));
  return listening;
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
  setExecPoint({ path: f.path, line: f.line, top });
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

/** 执行点现在是不是这一格的（调试面板里「回到执行点」用） */
export const ownsExec = (id: number) => execOwner === id && !!execPoint();
