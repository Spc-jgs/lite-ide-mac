/**
 * 构建（⌘F9，docs/IDEA.md 第 22 节）前端那半的状态层：走任务那套起、跑完从输出里认出编译错误、交给运行窗的错误树、说一句结果。
 * 跑在裸 node 里，IPC 走浏览器桩（mock/tasks.ts 的 buildOutput / parseProblems，格式照真 mvn 撞出来的样子）。认错误的规则本身在 javac.rs 的测试里。
 *
 * 每一条是一种「说谎」的形状：
 * - 同一条错误 Maven 打两遍：列成两条，人以为有两个错
 * - 失败了错误树却空着、提示说「构建完成」：人以为编过了
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { project } = await import("../src/lib/state/project.svelte");
const { runs } = await import("../src/lib/state/runs.svelte");
const { notify } = await import("../src/lib/state/notify.svelte");
const { FILES } = await import("../src/lib/dev/mock/data");
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
/** 胶囊里此刻那句（做成了 / 没做成） */
const said = () => `${notify.toast?.kind}:${notify.toast?.text}`;

const ROOT = "/proj";
project.root = ROOT;
const r = () => runs.list.find((t) => t.name === "构建项目");

// ── 干净的项目：成功、没有错误、说「构建完成」 ──
{
  await ops.buildProject(false);
  const id = r()?.id;
  await ops.buildProject(false);
  await ops.buildProject(true);
  ok(r()?.id === id && !runs.list.some((t) => t.name === "重新构建项目"), "构建中重复构建或重新构建：不停止重起，也不并行 clean");
  ok(r()?.command === "mvn -B -ntp test-compile", `命令是 test-compile（连测试一起编）：${r()?.command}`);
  ok(await until(() => r()?.status === "done" && !!r()?.problems), `跑完、读了结果：${r()?.status}`);
  ok(r()?.problems?.items.length === 0, "没有错误");
  ok(/^ok:构建完成，耗时 [\d.]+ 秒$/.test(said()), `说了构建完成（做成了的颜色）：${said()}`);
}

// ── 写错了一个变量名：失败，错误树里正好一条（输出里那条出现两遍），行、列、补充都对 ──
{
  const F = "/proj/moduleB/src/main/java/com/demo/core/OrderClient.java";
  const before = FILES[F] as string;
  const lines = before.split("\n");
  lines.splice(3, 0, "    int x = nosuch;");
  FILES[F] = lines.join("\n");
  await ops.buildProject(false);
  ok(await until(() => r()?.status === "failed" && !!r()?.problems), `失败、读了结果：${r()?.status}`);
  const p = r()?.problems?.items ?? [];
  ok(p.length === 1, `同一条只列一次（Maven 打了两遍）：${p.length} 条`);
  ok(p[0]?.path === F && p[0]?.line === 4 && p[0]?.col === 13 && p[0]?.error, `位置对：${JSON.stringify(p[0])}`);
  ok(p[0]?.message === "找不到符号\n符号:   变量 nosuch\n位置: 类 OrderClient", `补充接在后面、只接一遍：${JSON.stringify(p[0]?.message)}`);
  ok(said() === "fail:构建失败：1 个错误", `说了几个错误（没做成的颜色）：${said()}`);
  FILES[F] = before;
}

// ── 重新构建：先 clean ──
{
  await ops.buildProject(true);
  const rr = () => runs.list.find((t) => t.name === "重新构建项目");
  ok(rr()?.command === "mvn -B -ntp clean test-compile", `重新构建先 clean：${rr()?.command}`);
  await until(() => rr()?.status !== "running");
}

console.log(`${fail === 0 ? "✅" : "❌"} 构建（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
