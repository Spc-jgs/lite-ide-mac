/**
 * 运行窗的动作（#48，docs/TASKS.md）：起、重跑上一个、停、关、在编辑区打开日志。懒加载 —— ⌃R / ⌃⌥R / ⌘F2 / 运行窗
 * 第一次用到才拉（同 `replace-ops.ts`），入口包里只有 `runs.svelte.ts` 那几个字段。
 */
import { runs, type RunTab } from "./runs.svelte";
import { exitText } from "../run/exit-text";
import { layout } from "./layout.svelte";
import { project } from "./project.svelte";
import { notify } from "./notify.svelte";
import { progress } from "./progress.svelte";
import { overlay } from "./overlay.svelte";
import { listenHere, type TaskExit, type TaskLinger } from "../ipc/commands";
import { buildProblems, coverageReport, runCompoundItems, servicesList, taskClose, taskFreePort, taskNewFile, taskRun, taskStale, taskStaleResolve, taskStop, taskStuck, testResults } from "../ipc/tasks";

let listening: Promise<unknown> | null = null;

/**
 * 每个项目最近一次跑的任务名：⌃R 跑它。重启就忘（同终端不恢复的理由）。
 * 放这儿不放 `runs.svelte.ts`：没有界面画它，用不着响应式，也用不着进入口包（第 3 步量过，挪走省下的那几百字节在 JOURNAL 里）
 */
export const last: Record<string, string> = {};

/** 听「任务退出了」。第一次跑任务时挂上，之后一直挂着（一个窗口一份，Rust 只发给起它的窗口） */
function listen() {
  listening ??= Promise.all([listenHere<TaskExit>("task-exit", (e) => exited(e.payload)), listenHere<TaskLinger>("task-linger", (e) => lingered(e.payload))]);
  return listening;
}

export function exited(x: TaskExit) {
  // 按 id 现找（frontend.md「`$state` 数组里的元素」）：拿到的是代理，改了界面才跟着变
  const r = runs.list.find((t) => t.id === x.id);
  if (!r) return;
  r.status = x.stopped ? "stopped" : x.failed ? "failed" : "done";
  r.code = x.code;
  r.holder = x.port;
  r.lingering = false;
  // 端口被占有自己的卡片（说清是谁、给出路），不再另弹一句「退出了」。跑单测的失败由结果那句说（「3 通过 · 1 没过」比「退出码 1」有用）
  if (isTestRun(r.name)) void loadTests(r.id);
  else if (isBuildRun(r.name)) void loadProblems(r.id);
  else if (r.status === "failed" && !x.port) notify.fail(exitText(r.name, x.code));
}

/** 构建的两个任务名（同 `taskdefs::BUILD` / `REBUILD`） */
const BUILD = "构建项目";
const REBUILD = "重新构建项目";
const isBuildRun = (name: string) => name === BUILD || name === REBUILD;

/** 构建项目（⌘F9）/ 重新构建（docs/IDEA.md 第 22 节）：每个 Maven reactor 跑一遍 `test-compile`，走任务那套（运行窗、⌃R 重跑、⌘F2 停） */
export async function buildProject(clean: boolean) {
  const root = project.root;
  if (!root) {
    notify.hint("请先打开项目：构建针对整个项目");
    return;
  }
  await runTask(clean ? REBUILD : BUILD, root);
}

/**
 * 构建结束了：从这次的输出里认出编译错误，交给运行窗左边的错误树，说一句结果（同 IDEA 状态栏那句「构建完成，用了 N 秒」）。
 * 失败了却一条都没认出来 —— 不是编译错误（依赖没下下来、pom 写错），照实说
 */
async function loadProblems(id: number) {
  const r = runs.list.find((t) => t.id === id);
  if (!r) return;
  const secs = ((Date.now() - r.startedAt) / 1000).toFixed(1);
  try {
    const res = await buildProblems(r.log);
    const now = runs.list.find((t) => t.id === id);
    if (!now) return;
    now.problems = res;
    const errs = res.items.filter((p) => p.error).length;
    if (now.status === "stopped") return;
    if (errs) notify.fail(`构建失败：${res.total > res.items.length ? `共 ${res.total} 条，显示 ${res.items.length} 条，` : ""}${errs} 个错误`);
    else if (now.status === "failed") notify.fail("构建失败（非编译错误，可能是依赖、pom 或插件问题），请查看输出", 8000);
    else if (debugging(now.root)) await hotswap(`构建完成（${secs} 秒）`);
    else notify.ok(`构建完成，耗时 ${secs} 秒`);
  } catch (e) {
    notify.fail(`无法读取构建结果：${e instanceof Error ? e.message : String(e)}`);
  }
}

/** 这个项目有没有正在调试的程序（挂上了的：在跑或停着） */
const debugging = (root: string) => runs.list.some((r) => r.root === root && (r.debug?.phase === "running" || r.debug?.phase === "paused"));

/** 类全名只说最后一段，内部类算进外面那个（`com.demo.Money$1` → `Money`），多了说「等 N 个」 */
function classNames(all: string[]): string {
  const names = [...new Set(all.map((c) => c.slice(c.lastIndexOf(".") + 1).split("$")[0]))];
  return names.length > 3 ? `${names.slice(0, 3).join("、")} 等 ${names.length} 个` : names.join("、");
}

/**
 * 热替换（docs/IDEA.md 第 26 节）：把和调试开始时比变了的 class 换进正在调试的 JVM。构建（⌘F9）成功后自动做（`head` 是「构建完成」那半句，
 * 合成一句说：连着弹两句，第一句来不及读就被顶掉），菜单「重新加载改过的类」手动做。
 * 自动那次「没变化」就只说构建完成 —— 构建的多半是没改过的模块
 */
export async function hotswap(head = "") {
  const root = project.root;
  if (!root) return;
  const pre = head ? `${head}，` : "";
  try {
    const { debugHotswap } = await import("../ipc/debug");
    const r = await debugHotswap(root);
    const paused = runs.list.some((x) => x.root === root && x.debug?.phase === "paused");
    switch (r.kind) {
      case "done":
        // 停着的那一帧已经在旧方法里了，JVM 让它把旧的跑完 —— 不说的话，人 F8 一步看到的还是旧代码，以为没换上
        // 行号变了的类，这次调试里它的断点下不了（照旧表下会下错地方）：不说的话，人只会觉得「断点怎么不停了」
        if (r.lost.length) {
          const files = r.lost.map((p) => p.slice(p.lastIndexOf("/") + 1)).join("、");
          return notify.ok(`${pre}已热替换 ${classNames(r.classes)}。${files} 的行号已变化，其中的断点在本次调试中不再生效，重新调试后恢复`, 10000);
        }
        return notify.ok(`${pre}已热替换 ${classNames(r.classes)}${paused ? "（当前暂停的栈帧仍执行旧代码，下次调用起生效）" : ""}`);
      case "unchanged":
        if (head) return notify.ok(`${head}，没有需要热替换的类`);
        return notify.hint("没有修改过的类（修改后请先构建 ⌘F9）");
      case "failed":
        return notify.fail(`${pre}但热替换失败：${r.message}（${classNames(r.classes)}）。需重新调试才能生效`, 10000);
      case "noreply":
        return notify.fail(`${pre}但热替换未响应：程序整体暂停时无法热替换，请继续运行后选择「运行 › 重新加载已更改的类」`, 10000);
      case "noagent":
        return notify.hint(`${pre}当前构建不支持热替换（开发构建），需重新调试才能生效`);
      case "nosession":
        if (head) return notify.ok(head);
        return notify.hint("热替换仅适用于正在调试的程序");
    }
  } catch (e) {
    notify.fail(`热替换失败：${e instanceof Error ? e.message : String(e)}`);
  }
}

/**
 * 「模块 [test …]」：同 java-tests.ts 的 parseTestTask，这里只要判断。**不引那个模块**：入口里那句 import("runs-ops") 要列出它静态依赖的每个块的文件名，
 * 多引一个就多一个（2026-10-19 量的：+41 B、越过入口包的告警线）
 */
const isTestRun = (name: string) => /^.+ \[(test|cover) .+\]$/.test(name);

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
    if (!res.cases.length) notify.fail(`「${r.name}」没有测试结果（编译失败或未找到测试），请查看输出`, 8000);
    else if (/ \[cover /.test(r.name)) void loadCoverage(r.root, r.name, good, bad);
    else if (bad) notify.fail(`${good} 个通过 · ${bad} 个失败`);
    else notify.ok(`${good} 个测试全部通过`);
  } catch (e) {
    notify.fail(`无法读取测试结果：${e instanceof Error ? e.message : String(e)}`);
  }
}

/**
 * 跑单测的过程中，结果一个类一个类地亮（docs/TAKEOVER.md G15，IDEA 的测试视图是边跑边出的）：surefire 每跑完一个测试类写一份 `TEST-类.xml`，
 * 这里每秒读一次这次写的报告（Rust 只认写完了的那几份，`junit::results`）。原来要等整个 `mvn test` 跑完才出结果树 —— 一个模块几百个测试、
 * 前几个类早就挂了，人还要干等几分钟才知道。
 *
 * **回包到的时候这次已经跑完了就不写**：跑完那一下 `loadTests` 读的是最终那份，晚到的一次轮询会拿半截的结果把它盖掉
 */
async function watchTests(id: number) {
  let seen = -1;
  for (;;) {
    await new Promise((ok) => setTimeout(ok, 1000));
    const r = runs.list.find((t) => t.id === id);
    if (!r || r.status !== "running") return;
    // 不往前放宽：跑着的时候，开跑之前那一秒里写的报告（上一次跑的）一定不是这次的
    const res = await testResults(r.root, r.name, r.startedAt).catch(() => null);
    const now = runs.list.find((t) => t.id === id);
    if (!res || !now || now.status !== "running" || res.total === seen) continue;
    seen = res.total;
    now.tests = res;
    const { noteOutcomes } = await import("../run/test-outcomes");
    noteOutcomes(res.cases);
  }
}

/**
 * 带覆盖率跑的那次（「模块 [cover …]」，docs/TAKEOVER.md G15）跑完：出报告（JaCoCo 的命令行，一两秒）、交给编辑器和覆盖率工具窗，
 * 胶囊上说一句测试结果和总的行覆盖率，带一个「看覆盖率」。报告和画它的都在懒块里（动态引，同 isTestRun 那条理由）
 */
async function loadCoverage(root: string, name: string, good: number, bad: number) {
  const t = progress.start("正在生成覆盖率报告");
  try {
    const [rep, cov] = await Promise.all([coverageReport(root, name), import("./coverage")]);
    cov.setCoverage(name, rep);
    const { pct, hit, all } = cov.totalPct(rep.files);
    const tests = bad ? `${good} 个通过 · ${bad} 个失败` : `${good} 个测试全部通过`;
    const msg = `${tests} · 行覆盖率 ${Math.round(pct * 100)}%（${hit} / ${all} 行）`;
    const act = { label: "查看覆盖率", run: () => layout.toggleTool("coverage") };
    if (bad) notify.fail(msg, undefined, act);
    else notify.ok(msg, undefined, act);
  } catch (e) {
    notify.fail(`无法生成覆盖率报告：${e instanceof Error ? e.message : String(e)}`, 8000);
  } finally {
    t.end();
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
/**
 * 起任务那条路上每一步的时刻（Unix 毫秒），最近 50 条：spring.sh「构建项目」间歇 240 秒才起（2026-10-28 抓到：Rust 那边真起进程是在验收等满 240 秒那一刻），
 * 卡在前端哪一步还是 Rust 哪一步，靠这个和 Rust 的诊断行（`@毫秒`）对。只是一张数组，测试通道读（`__lite.runTrace`）
 */
export const runTrace: [number, string][] = [];
const trace = (what: string) => {
  runTrace.push([Date.now(), what]);
  if (runTrace.length > 50) runTrace.shift();
};

export async function runTask(name: string, root: string | null = project.root, debug = false) {
  trace(`runTask 进来 ${name}`);
  if (!root) return;
  // 同一个正在起（重跑要等旧的软停，最多 5 秒，人看着没动静会再按一次 ⌃R）：第二下不再发。
  // Rust 那边也挡（`begin_run` 的认领），这里先挡住是为了不弹那句「正在起」—— 人只是按急了（code review 2026-10-10）
  const key = `${root}\n${name}`;
  if (starting.has(key)) return void trace(`runTask ${name}：同名的正在起，这一下不发`);
  starting.add(key);
  try {
    // 组合配置（docs/TAKEOVER.md R1）：没有自己的命令，一份份起；⌃R 记的是组合的名字，重跑还是整组
    const items = await runCompoundItems(root, name).catch(() => null);
    trace(`run_compound_items 回来 ${name}`);
    if (items) {
      if (!items.length) return void notify.hint(`组合配置「${name}」中没有可运行的配置，请在「编辑配置…」中添加`);
      for (const it of items) await runTask(it, root, debug);
      last[root] = name;
      lastDebug[root] = debug;
      return;
    }
    await start(name, root, debug);
  } finally {
    starting.delete(key);
  }
}

/** 正在起的（项目根 + 任务名）。不用响应式：没有界面画它 */
const starting = new Set<string>();

async function start(name: string, root: string, debug: boolean) {
  await listen();
  trace(`listen 完、要调 task_run ${name}`);
  last[root] = name;
  lastDebug[root] = debug;
  const old = runs.list.find((t) => t.root === root && t.name === name);
  if (old && old.status === "running") old.status = "stopping";
  const startedAt = Date.now();
  // 先亮工具窗再等 Rust：同名的上一次要先软停（最多 5 秒），这段时间里人要看得见「在处理」。是不是服务认得出来的话先按它亮
  if (old) show(old.service);
  try {
    const r = await taskRun(root, name, debug);
    trace(`task_run 回来 ${name}`);
    // 远程 JVM 调试那种只能调试：▶ 也是挂上去（Rust 说了算，docs/TAKEOVER.md G23）
    debug = r.debug;
    lastDebug[root] = debug;
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
    if (isTestRun(name)) void watchTests(r.id);
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
    else notify.hint("请先在 Services 中调试一个 Spring Boot 服务，之后可用 ⌃D 再次调试");
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
  // 停不掉要说为什么（#68）：软停的宽限期（tasksvc::GRACE，5 秒）过了、或者强杀之后一会儿，还在「正在停」就问一句组里剩的是谁的。
  // 只问一次、不轮询：剩下的是 root 的进程时我们什么都做不了，等它自己退（退了状态一变，卡片跟着没）
  if (got === "stopping" || got === "killed") setTimeout(() => void explainStuck(r.id), got === "stopping" ? STUCK_AFTER_MS : 1500);
}

/** 软停之后多久还在「正在停」就去问：宽限期 5 秒 + 1 秒余量（SIGKILL 送出去、等待线程回收组长要几百毫秒） */
const STUCK_AFTER_MS = 6000;

async function explainStuck(id: number) {
  const r = runs.list.find((t) => t.id === id);
  if (!r || r.status !== "stopping") return;
  const stuck = await taskStuck(id).catch(() => []);
  const now = runs.list.find((t) => t.id === id);
  if (now && now.status === "stopping") now.stuck = stuck;
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

/** 停完以后端口还被派生出去的进程占着（#59）：同一格摆一张卡片。那一格已经被重跑换掉了（id 不同）就不摆 —— 新的那次会自己撞上端口被占 */
export function lingered(x: TaskLinger) {
  const r = runs.list.find((t) => t.id === x.id);
  if (!r || r.status !== "stopped") return;
  r.holder = x.holder;
  r.lingering = true;
}

/** 「结束进程」（#59 那张卡片）：只结束占着端口的那个，不重新运行 —— 人点的是「停」 */
export async function freeLingering(id: number) {
  const r = runs.list.find((t) => t.id === id);
  if (!r?.holder) return;
  const { port, command } = r.holder;
  try {
    await taskFreePort(port);
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
    return;
  }
  const now = runs.list.find((t) => t.id === id);
  if (now) now.holder = null;
  notify.ok(`已结束占用端口 ${port} 的 ${command}`);
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
    if (kill) notify.ok(`已结束上次遗留的 ${s.list.length} 个任务`);
  } catch (e) {
    notify.fail(e instanceof Error ? e.message : String(e));
  }
}

