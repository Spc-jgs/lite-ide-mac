/**
 * 运行窗的动作（#48，docs/TASKS.md）：起、重跑上一个、停、关、在编辑区打开日志。懒加载 —— ⌃R / ⌃⌥R / ⌘F2 / 运行窗
 * 第一次用到才拉（同 `replace-ops.ts`），入口包里只有 `runs.svelte.ts` 那几个字段。
 */
import { runs, type RunTab } from "./runs.svelte";
import { exitText } from "../run/exit-text";
import { layout } from "./layout.svelte";
import { project } from "./project.svelte";
import { notify } from "./notify.svelte";
import { overlay } from "./overlay.svelte";
import { listenHere, type TaskExit } from "../ipc/commands";
import { servicesList, taskClose, taskFreePort, taskNewFile, taskRun, taskStale, taskStaleResolve, taskStop, testResults } from "../ipc/tasks";

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
  // 端口被占有自己的卡片（说清是谁、给出路），不再另弹一句「退出了」。跑单测的失败由结果那句说（「3 通过 · 1 没过」比「退出码 1」有用）
  if (isTestRun(r.name)) void loadTests(r.id);
  else if (r.status === "failed" && !x.port) notify.fail(exitText(r.name, x.code));
}

/**
 * 「模块 [test …]」：同 java-tests.ts 的 parseTestTask，这里只要判断。**不引那个模块**：入口里那句 import("runs-ops") 要列出它静态依赖的每个块的文件名，
 * 多引一个就多一个（2026-10-19 量的：+41 B、越过入口包的告警线）
 */
const isTestRun = (name: string) => /^.+ \[test .+\]$/.test(name);

/**
 * 跑单测那次结束了：读这次写的 surefire 报告（docs/IDEA.md 第 17 节），交给运行窗的结果树和编辑器行号旁的 ✓ / ✗，说一句结果。
 * 一条都没读到 —— 多半是编译没过、测试一个都没跑起来：照实说，原因在输出里
 */
async function loadTests(id: number) {
  const r = runs.list.find((t) => t.id === id);
  if (!r) return;
  try {
    const res = await testResults(r.root, r.name, r.startedAt - 1000);
    const now = runs.list.find((t) => t.id === id);
    if (!now) return;
    now.tests = res;
    const { noteOutcomes } = await import("../run/test-outcomes");
    noteOutcomes(res.cases);
    const bad = res.cases.filter((c) => c.outcome === "failed" || c.outcome === "error").length;
    const good = res.cases.filter((c) => c.outcome === "passed").length;
    if (!res.cases.length) notify.fail(`「${r.name}」一个测试结果都没有（编译没过、或者没找到这个测试），看输出`, 8000);
    else if (bad) notify.fail(`${good} 个通过 · ${bad} 个没过`);
    else notify.ok(`${good} 个测试全部通过`);
  } catch (e) {
    notify.fail(`读不到测试结果：${e instanceof Error ? e.message : String(e)}`);
  }
}

/** 跑起来之后亮出它所在的那个工具窗：Spring Boot 服务在 Services，别的在运行窗 */
function show(service: boolean) {
  layout.panelView = service ? "services" : "run";
  layout.panel = true;
}

/**
 * 跑一个任务。同名的还在跑：Rust 那边先停它、等进程组真没了再起（端口要先空出来）——
 * 这段可能要几秒（软停的宽限期），所以先把旧的那一格标成「停止中」，人看得见在等什么
 */
export async function runTask(name: string, root: string | null = project.root, debug = false) {
  if (!root) return;
  // 同一个正在起（重跑要等旧的软停，最多 5 秒，人看着没动静会再按一次 ⌃R）：第二下不再发。
  // Rust 那边也挡（`begin_run` 的认领），这里先挡住是为了不弹那句「正在起」—— 人只是按急了（code review 2026-10-10）
  const key = `${root}\n${name}`;
  if (starting.has(key)) return;
  starting.add(key);
  try {
    await start(name, root, debug);
  } finally {
    starting.delete(key);
  }
}

/** 正在起的（项目根 + 任务名）。不用响应式：没有界面画它 */
const starting = new Set<string>();

async function start(name: string, root: string, debug: boolean) {
  await listen();
  last[root] = name;
  lastDebug[root] = debug;
  const old = runs.list.find((t) => t.root === root && t.name === name);
  if (old && old.status === "running") old.status = "stopping";
  const startedAt = Date.now();
  // 先亮工具窗再等 Rust：同名的上一次要先软停（最多 5 秒），这段时间里人要看得见「在处理」。是不是服务认得出来的话先按它亮
  if (old) show(old.service);
  try {
    const r = await taskRun(root, name, debug);
    show(r.service);
    const tab: RunTab = {
      id: r.id, root, name, command: r.command, log: r.log, status: "running", code: null, holder: null, service: r.service, startedAt, tests: null,
      debug: debug ? { run: r.id, phase: "waiting", note: "", thread: null, reason: "", frames: [], totalFrames: 0, threads: [] } : null,
      http: null,
    };
    // 同名的那一格原地换掉（位置不跳），没有就加在最后
    const i = runs.list.findIndex((t) => t.root === root && t.name === name);
    if (i >= 0) runs.list[i] = tab;
    else runs.list = [...runs.list, tab];
    // 运行窗的「当前格」只指运行窗里的；服务的选中在 Services 工具窗自己那儿
    if (!r.service) runs.activeId = r.id;
    // 调试的状态事件可能比这一格先到（Rust 那边一起来就发）：交给调试那边补上（动态引：静态引会把它的块名带进入口，见 isTestRun 那条）
    if (debug) void import("../debug/debug-ops").then((m) => m.adopt(r.id));
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
  }
}

/** 最近那次是不是调试着起的：⌃R 照原样再来一次（IDEA 的 Rerun 也是：调试起的重跑还是调试） */
const lastDebug: Record<string, boolean> = {};

/**
 * ⌃R：再跑这个项目最近跑过的那个（调试起的还是调试）；一个都没跑过 → 打开任务列表让人选。
 * ⌃D（`debug`）：调试最近跑过的那个（IDEA 的 Debug）。没跑过就说一句 —— 任务列表里的大多不是 Java 程序，调不了
 */
export function rerunLast(debug = false) {
  const root = project.root;
  if (!root) return;
  const name = last[root];
  if (debug) {
    if (name) void import("../debug/debug-ops").then((m) => m.debugTask(name, root));
    else notify.hint("先在 Services 里调试一个 Spring Boot 服务（工具栏上的虫子），之后 ⌃D 调试同一个");
  } else if (name) void runTask(name, root, lastDebug[root] ?? false);
  else overlay.taskPicker = true;
}

/** Services 工具窗里选中的那个服务的这次运行（ServicesView 报上来）：⌘F2 在 Services 开着时停它 */
let serviceFocus: number | null = null;
export function focusService(id: number | null) {
  serviceFocus = id;
}

const isAlive = (t: RunTab) => t.status === "running" || t.status === "stopping";

/**
 * ⌘F2 停哪个：Services 开着就是那边选中的服务，否则运行窗的当前格。那一个没在跑、而整个窗口只有一个在跑的，就停那一个 ——
 * 服务不进运行窗的标签，不这样的话 Services 收起时 ⌘F2 永远停不到它（IDEA 也是：只有一个在跑时直接停）
 */
function stopTarget(): number | null {
  const id = layout.panel && layout.panelView === "services" ? serviceFocus : runs.activeId;
  const r = runs.list.find((t) => t.id === id);
  if (r && isAlive(r)) return id;
  const alive = runs.list.filter(isAlive);
  return alive.length === 1 ? alive[0].id : id;
}

/** ⌘F2 / 停止按钮：第一下软停，正在软停时再按一下立刻强杀（Rust 那边按状态判） */
export async function stopRun(id: number | null = stopTarget()) {
  const r = runs.list.find((t) => t.id === id);
  if (!r || (r.status !== "running" && r.status !== "stopping")) return;
  // HTTP 请求不是任务：停 = 取消（docs/IDEA.md 第 19 节）
  if (r.http) return void import("../http/http-ops").then((m) => m.cancel(r.id));
  const got = await taskStop(r.id);
  const now = runs.list.find((t) => t.id === id);
  // 只在它还「在跑」时标：退得快的（vite 几毫秒）退出事件比这一条的回包先到，已经是「已停止」了，
  // 再标回「正在停」就再也没有事件来改它（tasks-real.sh 在真 .app 上撞见的）
  if (now && got === "stopping" && now.status === "running") now.status = "stopping";
}

/** 关掉一格：还在跑就软停（Rust 那边在后台等宽限期）。关掉最后一格，运行窗跟着收起（同终端，`terms.close`） */
export function closeRun(id: number) {
  if (!runs.list.some((t) => t.id === id)) return;
  // 下标按运行窗里的那几格数：运行窗的当前格只在它们里挑（服务不在运行窗的标签上）
  const i = runs.plain().findIndex((t) => t.id === id);
  const r = runs.list.find((t) => t.id === id);
  if (r?.http) void import("../http/http-ops").then((m) => m.cancel(id));
  else void taskClose(id);
  runs.list = runs.list.filter((t) => t.id !== id);
  const plain = runs.plain();
  if (runs.activeId === id) runs.activeId = plain[Math.min(i, plain.length - 1)]?.id ?? null;
  if (plain.length === 0 && layout.panelView === "run") {
    layout.panelView = "term";
    layout.panel = false;
  }
}

/** 运行窗的重跑按钮：任务照名字再跑（调试起的还是调试），HTTP 请求照文件此刻的样子重发 */
export function rerun(id: number) {
  const r = runs.list.find((t) => t.id === id);
  if (!r) return;
  if (r.http) void import("../http/http-ops").then((m) => m.resend(id));
  else void runTask(r.name, r.root, !!r.debug);
}

/** 这一格的输出在编辑区开成一个日志标签（大屏看、和别的日志并排） */
export async function openRunLog(id: number | null = runs.activeId) {
  const r = runs.list.find((t) => t.id === id);
  // HTTP 请求还在发 / 没发成：还没有响应体可开
  if (!r?.log) return;
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

/** 启动后、项目根定了：问一次这个项目有没有上次没停干净的任务，顺带认一次有没有 Spring Boot 服务（同一个时机，入口里少一处调用） */
export async function checkStale(root: string) {
  void detectServices(root);
  try {
    const list = await taskStale(root);
    if (list.length && project.root === root) runs.stale = { root, list };
  } catch {
    /* 问不到就算了：不影响别的功能，下次启动再问（同替换的中断卡片） */
  }
}

/**
 * 这个项目有没有 Spring Boot 服务：导轨上的 Services 按钮有服务才出现（ui.md 第十条）。不是 Maven 项目 Rust 那边读一眼项目根就回空的；
 * 认入口要翻 Java 源码，所以只在开项目时问一次 —— Services 工具窗开着时它自己会再认
 */
export async function detectServices(root: string) {
  runs.services = 0;
  try {
    const list = await servicesList(root);
    if (project.root === root) runs.services = list.length;
  } catch {
    /* 问不到就当没有：按钮不出，别的不受影响 */
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

