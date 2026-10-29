/** 响应头 → 本项目本机服务日志。端口相同不意味着远程地址或另一个项目就是这个服务。 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
import type { RunTab } from "../src/lib/state/runs.svelte";
import { parseQuery, matchLine } from "../src/lib/logview/query.ts";
installMockIpc();
const { runs } = await import("../src/lib/state/runs.svelte");
const { layout } = await import("../src/lib/state/layout.svelte");
const { logFocus } = await import("../src/lib/run/log-focus.svelte");
const { findInLogs } = await import("../src/lib/http/http-ops");
const inner = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
const orig = inner.invoke;
const ports = new Map<number, number[]>();
let checked: number[] = [];
let duringCheck: (() => void) | null = null;
inner.invoke = async (c, a) => {
  if (c !== "task_ports") return orig(c, a);
  checked.push(Number(a.id));
  duringCheck?.();
  return ports.get(Number(a.id)) ?? [];
};
const run = (id: number, root: string, service = true): RunTab => ({ id, root, service, name: `service-${id}`, command: "java Main", log: `/run-${id}.log`, status: "running", code: null, holder: null, startedAt: Date.now(), tests: null, debug: null, http: null });
let pass = 0, fail = 0;
const ok = (c: unknown, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

runs.list = [run(1, "/other"), { ...run(2, "/proj"), status: "stopped" }, run(3, "/proj")];
for (const id of [1, 2, 3]) ports.set(id, [8080]);
ok(await findInLogs("/proj", "http://localhost:8080/orders", "-req/7"), "找到本项目监听相同端口的运行");
ok(checked.join(",") === "3", "不查别的项目、已经停掉的运行");
ok(layout.panel && layout.panelView === "services" && logFocus.req?.id === 3, "打开 Services、选的是正确的运行");
const q = parseQuery(logFocus.req!.pattern);
ok(matchLine("INFO trace=-req/7", q, false) && !matchLine("INFO trace=other", q, false) && q.exclude.length === 0, "ID 以减号开头也按整个字面量查，不当排除条件或正则");
const nonce = logFocus.req!.nonce;
await findInLogs("/proj", "http://127.0.0.1:8080/orders", "second");
ok(logFocus.req!.nonce > nonce && logFocus.req!.pattern === '"second"', "再点同一个服务换新过滤条件");

for (const url of ["https://remote.example:8080/orders", "file:///tmp/x", "bad-url", "http://localhost:9999/orders"]) {
  const before = logFocus.req;
  ok(!(await findInLogs("/proj", url, "wrong")) && logFocus.req === before, `${url} 不误跳到本机同端口服务`);
}
runs.list = [run(4, "/proj", false)];
ports.set(4, [443]);
ok(await findInLogs("/proj", "https://[::1]/orders", "req-4"), "IPv6 回环、HTTPS 默认端口");
ok(layout.panelView === "run" && runs.activeId === 4, "普通任务在运行窗里选中");

runs.list = [run(5, "/proj")];
ports.set(5, [8080]);
duringCheck = () => { runs.list = []; };
ok(!(await findInLogs("/proj", "http://localhost:8080/orders", "gone")) && logFocus.req?.id === 4, "查端口期间被关闭的运行不再跳过去");
inner.invoke = orig;
console.log(`${fail === 0 ? "✅" : "❌"} HTTP 联日志：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
