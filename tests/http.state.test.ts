/**
 * HTTP 请求（docs/IDEA.md 第 19 节）前端那半的状态层：选环境、替换变量、运行窗里那一格的一生（发 → 回来 / 没发成 / 取消）、重发。
 * 跑在裸 node 里，IPC 走浏览器桩（mock/http.ts：发请求故意等 600ms）。curl 那半在 crates/httpsvc 的测试里，真 .app 在 scripts/accept/http.sh。
 *
 * 每一条是一种「说谎」的形状：
 * - 有没定义的变量还发出去：服务器回一个莫名其妙的 400，人往服务端去查
 * - 同一个请求再发一次多出一格（应该原地换掉），或者换掉时旧的还在发、回来盖掉新的
 * - 取消了还标成「在跑」；重发发的是上次那份、不是改过的
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { FILES } = await import("../src/lib/dev/mock/data");
const { runs } = await import("../src/lib/state/runs.svelte");
const { layout } = await import("../src/lib/state/layout.svelte");
const m = await import("../src/lib/http/http-ops");
const ops = await import("../src/lib/state/runs-ops");

let pass = 0;
let fail = 0;
const ok = (c: unknown, msg: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + msg);
  }
};
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until(f: () => boolean, ms = 4000) {
  const end = Date.now() + ms;
  while (!f() && Date.now() < end) await sleep(20);
  return f();
}

const ROOT = "/proj";
const FILE = "/proj/api/orders.http";
const text = FILES[FILE];
const tab = (name: string) => runs.list.find((r) => r.name === name);

// ── 没选环境：{{host}} 没定义 → 不发、不开格子 ──
m.setEnv(ROOT, null);
await m.sendAt(ROOT, FILE, text, 4);
ok(runs.list.length === 0, `有没定义的变量就不发：${JSON.stringify(runs.list.map((r) => r.name))}`);

// ── 选 dev：发出去、运行窗亮出来、正在发 → 回来 ──
m.setEnv(ROOT, "dev");
ok(m.envOf(ROOT) === "dev" && JSON.parse(localStorage.getItem("lite-ide.http-env") ?? "{}")[ROOT] === "dev", "环境按项目记进 localStorage");
const p1 = m.sendAt(ROOT, FILE, text, 9);
await sleep(30);
ok(tab("下单")?.status === "running" && tab("下单")?.log === "", `发出去还没回来：在跑、还没有响应体：${tab("下单")?.status}`);
ok(layout.panel && layout.panelView === "run" && runs.activeId === tab("下单")?.id, "运行窗亮出来、这一格是当前格");
await p1;
const t = tab("下单")!;
ok(t.status === "done" && t.code === 201 && t.http?.resp?.status === 201 && t.log.startsWith("/mock-http/"), `回来了：${t.status} ${t.code} ${t.log}`);
ok(t.command === "POST http://localhost:8080/api/orders", `发出去的是替换好的：${t.command}`);
const echo = JSON.parse(FILES[t.log]).echo;
ok(/"trace": "[0-9a-f-]{36}"/.test(echo.body), `体里的 {{$uuid}} 换成了真的：${echo.body}`);
ok(t.http?.env === "dev", "记下用的哪个环境");

// ── 同一个请求再发一次：原地换掉（不多一格），还在发的那个先取消、回来也不盖掉新的 ──
const before = runs.list.length;
const a = m.sendAt(ROOT, FILE, text, 4);
await sleep(30);
const firstId = tab("查订单")!.id;
const b = m.sendAt(ROOT, FILE, text, 4);
await Promise.all([a, b]);
ok(runs.list.filter((r) => r.name === "查订单").length === 1 && runs.list.length === before + 1, `同名的原地换掉：${runs.list.map((r) => r.name).join(",")}`);
ok(tab("查订单")!.id !== firstId && tab("查订单")!.status === "done", `留下的是新的那次、而且回来了：${tab("查订单")?.status}`);

// ── 连不上 / 500：红点，原因或状态码 ──
await m.sendAt(ROOT, FILE, text, 21);
ok(tab("连不上")?.status === "failed" && tab("连不上")!.http!.error.startsWith("连不上"), `连不上：${tab("连不上")?.http?.error}`);
await m.sendAt(ROOT, FILE, text, 24);
ok(tab("500")?.status === "failed" && tab("500")?.code === 500, "500 是红点");

// ── < 文件：按 .http 所在目录解析成绝对路径交给 Rust ──
await m.sendAt(ROOT, FILE, text, 15);
ok(JSON.parse(FILES[tab("从文件读请求体")!.log]).echo.bodyFile === "/proj/api/body.json", "< ./body.json → /proj/api/body.json");
ok(m.absPath("/p/a/b.http", "../x/./y.json") === "/p/x/y.json" && m.absPath("/p/a/b.http", "/abs.json") === "/abs.json", "相对路径里的 .. 和 .");

// ── 取消：运行窗的停止（stopRun）走取消，不当成任务 ──
const pending = m.sendAt(ROOT, FILE, text, 4);
await sleep(50);
await ops.stopRun(tab("查订单")!.id);
await pending;
ok(tab("查订单")?.status === "stopped" && tab("查订单")?.http?.error === "已取消", `取消了标成「已停止」：${tab("查订单")?.status}`);

// ── 重发：照文件此刻的样子（改了请求，重发的是改过的） ──
FILES[FILE] = text.replace("GET {{base}}/fail", "GET {{base}}/fail?again=1");
ops.rerun(tab("500")!.id);
ok(await until(() => tab("500")?.command.endsWith("/fail?again=1") === true && tab("500")?.status === "failed"), `重发的是改过的那份：${tab("500")?.command}`);

// ── 关掉一格：还在发就取消，不去碰任务表 ──
const p2 = m.sendAt(ROOT, FILE, text, 9);
await sleep(30);
const id = tab("下单")!.id;
ops.closeRun(id);
await p2;
ok(!runs.list.some((r) => r.id === id), "关掉了");

console.log(`${fail === 0 ? "✅" : "❌"} HTTP 请求（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
