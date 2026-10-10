/**
 * 运行窗的动作（#48，docs/TASKS.md）：起、重跑上一个、停、关、在编辑区打开日志。懒加载 —— ⌃R / ⌃⌥R / ⌘F2 / 运行窗
 * 第一次用到才拉（同 `replace-ops.ts`），入口包里只有 `runs.svelte.ts` 那几个字段。
 */
import { runs, type RunTab } from "./runs.svelte";
import { layout } from "./layout.svelte";
import { project } from "./project.svelte";
import { notify } from "./notify.svelte";
import { overlay } from "./overlay.svelte";
import { listenHere, type TaskExit } from "../ipc/commands";
import { taskClose, taskFreePort, taskNewFile, taskRun, taskStale, taskStaleResolve, taskStop } from "../ipc/tasks";

let listening: Promise<unknown> | null = null;

/**
 * 每个项目最近一次跑的任务名：⌃R 跑它。重启就忘（同终端不恢复的理由）。
 * 放这儿不放 `runs.svelte.ts`：没有界面画它，用不着响应式，也用不着进入口包（第 3 步量过，挪走省下的那几百字节在 JOURNAL 里）
 */
export const last: Record<string, string> = {};

/** 听「任务退出了」。第一次跑任务时挂上，之后一直挂着（一个窗口一份，Rust 只发给起它的窗口） */
function listen() {
  listening ??= listenHere<TaskExit>("task-exit", (e) => exited(e.payload));
  return listening;
}

export function exited(x: TaskExit) {
  // 按 id 现找（frontend.md「`$state` 数组里的元素」）：拿到的是代理，改了界面才跟着变
  const r = runs.list.find((t) => t.id === x.id);
  if (!r) return;
  r.status = x.stopped ? "stopped" : x.failed ? "failed" : "done";
  r.code = x.code;
  r.holder = x.port;
  // 端口被占有自己的卡片（说清是谁、给出路），不再另弹一句「退出了」
  if (r.status === "failed" && !x.port) notify.fail(`「${r.name}」退出了${x.code !== null ? `（退出码 ${x.code}）` : ""}`);
}

/** 亮出运行窗 */
function show() {
  layout.panelView = "run";
  layout.panel = true;
}

/**
 * 跑一个任务。同名的还在跑：Rust 那边先停它、等进程组真没了再起（端口要先空出来）——
 * 这段可能要几秒（软停的宽限期），所以先把旧的那一格标成「停止中」，人看得见在等什么
 */
export async function runTask(name: string, root: string | null = project.root) {
  if (!root) return;
  await listen();
  last[root] = name;
  const old = runs.list.find((t) => t.root === root && t.name === name);
  if (old && old.status === "running") old.status = "stopping";
  show();
  try {
    const r = await taskRun(root, name);
    const tab: RunTab = { id: r.id, root, name, command: r.command, log: r.log, status: "running", code: null, holder: null };
    // 同名的那一格原地换掉（位置不跳），没有就加在最后
    const i = runs.list.findIndex((t) => t.root === root && t.name === name);
    if (i >= 0) runs.list[i] = tab;
    else runs.list = [...runs.list, tab];
    runs.activeId = r.id;
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
  }
}

/** ⌃R：再跑这个项目最近跑过的那个；一个都没跑过 → 打开任务列表让人选 */
export function rerunLast() {
  const root = project.root;
  if (!root) return;
  const name = last[root];
  if (name) void runTask(name, root);
  else overlay.taskPicker = true;
}

/** ⌘F2 / 停止按钮：第一下软停，正在软停时再按一下立刻强杀（Rust 那边按状态判） */
export async function stopRun(id: number | null = runs.activeId) {
  const r = runs.list.find((t) => t.id === id);
  if (!r || (r.status !== "running" && r.status !== "stopping")) return;
  const got = await taskStop(r.id);
  const now = runs.list.find((t) => t.id === id);
  if (now && got === "stopping") now.status = "stopping";
}

/** 关掉一格：还在跑就软停（Rust 那边在后台等宽限期）。关掉最后一格，运行窗跟着收起（同终端，`terms.close`） */
export function closeRun(id: number) {
  const i = runs.list.findIndex((t) => t.id === id);
  if (i < 0) return;
  void taskClose(id);
  runs.list = runs.list.filter((t) => t.id !== id);
  if (runs.activeId === id) runs.activeId = runs.list[Math.min(i, runs.list.length - 1)]?.id ?? null;
  if (runs.list.length === 0 && layout.panelView === "run") {
    layout.panelView = "term";
    layout.panel = false;
  }
}

/** 这一格的输出在编辑区开成一个日志标签（大屏看、和别的日志并排） */
export async function openRunLog(id: number | null = runs.activeId) {
  const r = runs.list.find((t) => t.id === id);
  if (!r) return;
  const { tabflow } = await import("./tabflow.svelte");
  await tabflow.openPath(r.log, { preview: false });
}

/** 打开 `.lite-ide/tasks.json`，没有就先建一份带例子的（`task_new_file` 不冲掉已有的） */
export async function openTasksFile(root: string | null = project.root) {
  if (!root) return;
  try {
    const p = await taskNewFile(root);
    const { tabflow } = await import("./tabflow.svelte");
    await tabflow.openPath(p, { preview: false });
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
  }
}

/**
 * 端口卡片上的「结束它并重跑」：结束现在占着那个端口的进程（Rust 那边重新查一次是谁，不信这里带过去的 pid），端口空了再重跑这个任务。
 * 结束不掉（别的用户的进程、没有权限）就把原因说出来，卡片留着
 */
export async function freePortAndRerun(id: number) {
  const r = runs.list.find((t) => t.id === id);
  if (!r?.holder) return;
  const { port } = r.holder;
  try {
    await taskFreePort(port);
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
    return;
  }
  const now = runs.list.find((t) => t.id === id);
  if (now) now.holder = null;
  await runTask(r.name, r.root);
}

/** 卡片上的「不管它」：只收起卡片，不动那个进程 */
export function dismissHolder(id: number) {
  const r = runs.list.find((t) => t.id === id);
  if (r) r.holder = null;
}

/** 启动后、项目根定了：问一次这个项目有没有上次没停干净的任务 */
export async function checkStale(root: string) {
  try {
    const list = await taskStale(root);
    if (list.length && project.root === root) runs.stale = { root, list };
  } catch {
    /* 问不到就算了：不影响别的功能，下次启动再问（同替换的中断卡片） */
  }
}

/** 卡片上的两个按钮：结束它们 / 留着（从账上划掉、不再提醒） */
export async function resolveStale(kill: boolean) {
  const s = runs.stale;
  if (!s) return;
  runs.stale = null;
  try {
    await taskStaleResolve(s.root, kill);
    if (kill) notify.ok(`上次没停干净的 ${s.list.length} 个任务已经结束`);
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
  }
}

