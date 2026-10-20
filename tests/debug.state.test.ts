/**
 * 断点调试（docs/IDEA.md 第 18 节）前端那半的状态层：断点跟着行走、调试会话的状态进那一格运行、停下时标出执行点、单步 / 继续、结束。
 * 跑在裸 node 里，IPC 走浏览器桩（mock/debug.ts，次序和时长照真实现）。DAP 那半在 dapsvc 的测试和 debug.rs 里，真 JVM 在 spring.sh。
 *
 * 每一条是一种「丢东西 / 说谎」的形状：
 * - 断点不跟着行走：上面插了几行，断点还停在原来的行号上，停的就是别的语句
 * - 状态事件比那一格先到（Rust 一起来就发）：先到的被扔掉，那一格一直「正在挂上」
 * - 执行点不清：接着跑了，编辑器里还标着一行，看着像还停着
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { EditorState } = await import("@codemirror/state");
const { lineNumberMarkers } = await import("@codemirror/view");
const { project } = await import("../src/lib/state/project.svelte");
const { runs } = await import("../src/lib/state/runs.svelte");
const { FILES } = await import("../src/lib/dev/mock/data");
const bp = await import("../src/lib/debug/breakpoints");
const { execPoint } = await import("../src/lib/debug/exec-point");
const { javaDebugGutter } = await import("../src/lib/editor/java-debug-gutter");
const dbg = await import("../src/lib/debug/debug-ops");
const ops = await import("../src/lib/state/runs-ops");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until(f: () => boolean, ms = 6000) {
  const end = Date.now() + ms;
  while (!f() && Date.now() < end) await sleep(20);
  return f();
}

const ROOT = "/proj";
project.root = ROOT;
const FILE = "/proj/moduleB/src/main/java/com/demo/core/OrderServiceApplication.java";

// ── 断点跟着行走：上面插三行，断点跟着下移三行；表里换了（另一个分屏点的），编辑器照表重画 ──
{
  bp.setLines(ROOT, FILE, [8, 4]);
  ok(bp.linesOf(ROOT, FILE).join() === "4,8", `排好序、去重：${bp.linesOf(ROOT, FILE)}`);
  let st = EditorState.create({ doc: FILES[FILE], extensions: [javaDebugGutter(ROOT, FILE)] });
  const lines = (s: typeof st) => {
    const out: number[] = [];
    for (const set of s.facet(lineNumberMarkers)) for (let it = set.iter(); it.value; it.next()) out.push(s.doc.lineAt(it.from).number);
    return out.sort((a, b) => a - b);
  };
  ok(lines(st).join() === "4,8", `打开时照表画：${lines(st)}`);
  st = st.update({ changes: { from: 0, insert: "// a\n// b\n// c\n" } }).state;
  ok(lines(st).join() === "7,11", `上面插三行，断点跟着下移三行：${lines(st)}`);
  // 在断点那一行的行尾敲字：断点留在这一行（不跑到下一行去）
  const l7 = st.doc.line(7);
  st = st.update({ changes: { from: l7.to, insert: " // 注释" } }).state;
  ok(lines(st).join() === "7,11", `行尾敲字断点不动：${lines(st)}`);
  ok(JSON.parse(localStorage.getItem("lite-ide.breakpoints") ?? "{}")[ROOT]?.[FILE]?.join() === "4,8", "存进 localStorage（重启还在）");
  bp.toggle(ROOT, FILE, 4);
  ok(bp.linesOf(ROOT, FILE).join() === "8", `再点一次就取消：${bp.linesOf(ROOT, FILE)}`);
  bp.setLines(ROOT, FILE, []);
  ok(!JSON.parse(localStorage.getItem("lite-ide.breakpoints") ?? "{}")[ROOT]?.[FILE], "一个都没有了就从表里摘掉，不留空数组");
}

// ── 条件断点：条件挂在标记上跟着行走；清空就变回普通断点（存回数字，第一版读得懂）；只给行号时条件不丢 ──
{
  bp.setLines(ROOT, FILE, [4]);
  bp.setCond(ROOT, FILE, 8, "  cents == 1 ");
  ok(JSON.stringify(bp.bpsOf(ROOT, FILE)) === '[{"line":4},{"line":8,"cond":"cents == 1"}]', `右键填了条件、顺手下了断点、两头空白去掉：${JSON.stringify(bp.bpsOf(ROOT, FILE))}`);
  const stored = () => JSON.stringify(JSON.parse(localStorage.getItem("lite-ide.breakpoints") ?? "{}")[ROOT]?.[FILE]);
  ok(stored() === '[4,{"line":8,"cond":"cents == 1"}]', `普通的存数字、带条件的存对象：${stored()}`);
  let st = EditorState.create({ doc: FILES[FILE], extensions: [javaDebugGutter(ROOT, FILE)] });
  const marks = (s: typeof st) => {
    const out: string[] = [];
    for (const set of s.facet(lineNumberMarkers))
      for (let it = set.iter(); it.value; it.next()) out.push(`${s.doc.lineAt(it.from).number}${(it.value as unknown as { cond: string }).cond ? ":" + (it.value as unknown as { cond: string }).cond : ""}`);
    return out.join();
  };
  ok(marks(st) === "4,8:cents == 1", `编辑器里的标记带着条件：${marks(st)}`);
  st = st.update({ changes: { from: 0, insert: "// a\n// b\n" } }).state;
  ok(marks(st) === "6,10:cents == 1", `上面插两行，条件跟着那个断点走：${marks(st)}`);
  bp.setLines(ROOT, FILE, [8, 9]);
  ok(JSON.stringify(bp.bpsOf(ROOT, FILE)) === '[{"line":8,"cond":"cents == 1"},{"line":9}]', `只给行号（测试通道、点行号）：还在的那行条件照旧：${JSON.stringify(bp.bpsOf(ROOT, FILE))}`);
  bp.setCond(ROOT, FILE, 8, " ");
  ok(stored() === "[8,9]", `条件清空就是普通断点，存回数字：${stored()}`);
  bp.setLines(ROOT, FILE, []);
}

// ── 调试着起：状态比那一格先到也不丢、停在断点上标出执行点、单步、继续、停 ──
{
  bp.setLines(ROOT, FILE, [9]);
  await dbg.debugTask("OrderServiceApplication", ROOT);
  const r = () => runs.list.find((t) => t.name === "OrderServiceApplication");
  ok(r()?.debug, "调试着起的那一格带着调试状态");
  ok(r()?.command.includes("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0"), `命令里插了调试代理：${r()?.command}`);
  // 桩一起来就发 waiting（task_run 回包之前）——这一格建好时它已经发过了；adopt 要把它补上，不能停在建格时那个空的
  // 只认第一句：0.8 秒后桩会发第二句「等 Java 语言服务器」，等太久的话没补上的那版也会被它改绿（验红时撞见的）
  ok(await until(() => r()?.debug?.note === "等它编译完、开调试端口…", 300), `先到的「等编译」补上了：${JSON.stringify(r()?.debug)}`);
  ok(await until(() => r()?.debug?.phase === "running"), `挂上之后在跑：${r()?.debug?.phase}`);
  ok(await until(() => r()?.debug?.phase === "paused"), `撞上断点停下：${r()?.debug?.phase}`);
  const ep = execPoint();
  ok(ep?.path === FILE && ep.line === 9 && ep.top, `执行点标在断点那一行：${JSON.stringify(ep)}`);
  ok(r()?.debug?.frames.length === 4 && r()?.debug?.frames[2].path.startsWith("jdt://"), "调用栈带 jar 里的帧（jdt 地址）");

  await dbg.step("next");
  ok(await until(() => r()?.debug?.phase === "paused" && execPoint()?.line === 10), `单步跳过之后停在下一行：${JSON.stringify(execPoint())}`);
  // 没停着时按单步：什么都不做（F8 按惯了的，按到不是错）
  await dbg.step("continue");
  ok(await until(() => r()?.debug?.phase === "running"), "继续之后在跑");
  ok(execPoint() === null, `接着跑了执行点就清掉：${JSON.stringify(execPoint())}`);
  await dbg.step("next");
  ok(r()?.debug?.phase === "running", "在跑的时候按单步不动");

  await ops.stopRun(r()!.id);
  ok(await until(() => r()?.debug?.phase === "ended"), `任务停了调试跟着结束：${r()?.debug?.phase}`);
}

// ── ⌃D：调试最近跑过的那个（rerunLast(true)）；⌃R 重跑调试起的那个，还是调试 ──
{
  ops.rerunLast(false);
  const r = () => runs.list.find((t) => t.name === "OrderServiceApplication");
  ok(await until(() => r()?.status === "running" && !!r()?.debug), `⌃R 重跑调试起的还是调试：${JSON.stringify(r()?.debug?.phase)}`);
  await ops.stopRun(r()!.id);
  await until(() => r()?.debug?.phase === "ended");
}

// ── 调试开着时改条件，当场下到「JVM」里：条件为假的不停（桩：条件写 `false` 的不下进去 —— 验的是条件一路带到了 debug_breakpoints）。
// 条件在调试起来**之后**才填：之前填的话调试开始前的 syncAll 自己会带上，「改了条件没告诉正在跑的会话」那条路就验不到（验红时撞见的）
{
  bp.setLines(ROOT, FILE, [9]);
  await dbg.debugTask("OrderServiceApplication", ROOT);
  bp.setCond(ROOT, FILE, 9, "false");
  const r = () => runs.list.find((t) => t.name === "OrderServiceApplication");
  ok(await until(() => r()?.debug?.phase === "running"), `挂上了：${r()?.debug?.phase}`);
  await sleep(1500);
  ok(r()?.debug?.phase === "running" && execPoint() === null, `条件为假，跑过那一行不停：${r()?.debug?.phase}`);
  await ops.stopRun(r()!.id);
  await until(() => r()?.debug?.phase === "ended");
  bp.setLines(ROOT, FILE, []);
}

// ── 条件算不了（变量名打错）：照样停下，那一次停下带着一句提醒（桩：带 nosuch 的；真插件的原话在 debug.rs 的 cond_warning） ──
{
  bp.setCond(ROOT, FILE, 9, "nosuch == 1");
  await dbg.debugTask("OrderServiceApplication", ROOT);
  const r = () => runs.list.find((t) => t.name === "OrderServiceApplication");
  ok(await until(() => r()?.debug?.phase === "paused"), `照样停下：${r()?.debug?.phase}`);
  ok(r()?.debug?.note.startsWith("条件 nosuch == 1 算不了，照样停下了"), `停下时带着提醒：${r()?.debug?.note}`);
  await ops.stopRun(r()!.id);
  await until(() => r()?.debug?.phase === "ended");
  bp.setLines(ROOT, FILE, []);
}

// ── 调试单测（⌃⇧D / ▶ 菜单里的「调试」）：JDWP 交给 surefire；停在测试里的断点上时任务不退，继续之后跑完、结果树照常出来、会话正常结束 ──
{
  const T = "/proj/moduleA/src/test/java/com/demo/api/AdminControllerTest.java";
  const NAME = "moduleA [test com.demo.api.AdminControllerTest#listsAdmins]";
  bp.setLines(ROOT, T, [9]);
  await dbg.debugTask(NAME, ROOT);
  const r = () => runs.list.find((t) => t.name === NAME);
  ok(r()?.command.startsWith("mvn '-Dmaven.surefire.debug=-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0' "), `JDWP 交给 surefire fork 的 JVM：${r()?.command}`);
  ok(await until(() => r()?.debug?.phase === "paused"), `停在测试里的断点上：${r()?.debug?.phase}`);
  ok(execPoint()?.path === T && execPoint()?.line === 9, `执行点在测试文件第 9 行：${JSON.stringify(execPoint())}`);
  await sleep(1500);
  ok(r()?.status === "running", `停着的时候任务不退（被调试的 JVM 停着）：${r()?.status}`);
  await dbg.step("continue");
  ok(await until(() => r()?.status !== "running"), `继续之后跑完：${r()?.status}`);
  ok(await until(() => !!r()?.tests), "结果树照常出来");
  ok(r()?.tests?.cases.map((c) => c.name + ":" + c.outcome).join() === "listsAdmins:passed", `只跑了那一个、过了：${JSON.stringify(r()?.tests?.cases)}`);
  ok(await until(() => r()?.debug?.phase === "ended" && !r()?.debug?.note), `会话正常结束、不带报错：${JSON.stringify(r()?.debug)}`);
  bp.setLines(ROOT, T, []);
}

// ── 不是 Spring Boot 应用的调试不了：照实说，不起一个不带调试的 ──
{
  FILES["/proj/.lite-ide/tasks.json"] = `[{ "name": "前端", "command": "pnpm dev" }]`;
  const before = runs.list.length;
  await dbg.debugTask("前端", ROOT);
  ok(runs.list.length === before, "调不了就不起");
}

console.log(`${fail === 0 ? "✅" : "❌"} 断点调试（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
