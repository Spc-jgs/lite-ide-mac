/**
 * 单测边跑边出（docs/TAKEOVER.md G15）：两个测试类，第一个跑完、第二个还在跑的时候，结果树里已经有第一个的结果（挂了的已经标出来）；
 * 跑完是全部的；**跑完之后不能被晚到的那次轮询拿半截结果盖回去**。
 * 跑在裸 node 里，IPC 走浏览器桩（mock/tasks.ts：一个类 1.2 秒，跑着时 test_results 只给跑完了的类，同 surefire 一个类写一份报告）
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { project } = await import("../src/lib/state/project.svelte");
const { runs } = await import("../src/lib/state/runs.svelte");
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
async function until(f: () => boolean, ms = 8000) {
  const end = Date.now() + ms;
  while (!f() && Date.now() < end) await sleep(20);
  return f();
}

project.root = "/proj";
// 第一个类里有一个挂了的（桩里名字带 fail 的方法算没过），第二个全过
FILES["/proj/moduleA/src/test/java/com/demo/api/FirstTest.java"] = "package com.demo.api;\nclass FirstTest {\n  @Test void adds() {}\n  @Test void failsTotal() {}\n}\n";
FILES["/proj/moduleA/src/test/java/com/demo/api/SecondTest.java"] = "package com.demo.api;\nclass SecondTest {\n  @Test void one() {}\n  @Test void two() {}\n}\n";
const NAME = "moduleA [test com.demo.api.FirstTest,com.demo.api.SecondTest]";
const t0 = Date.now();
await ops.runTask(NAME, "/proj");
const run = () => runs.list.find((r) => r.name === NAME);
ok(run()?.status === "running" && run()?.tests === null, "刚起来：还没有结果");

await until(() => (run()?.tests?.cases.length ?? 0) > 0);
const mid = run();
ok(mid?.status === "running", `第一个类的结果出来时还在跑（${Date.now() - t0} ms）`);
ok(mid?.tests?.cases.map((c) => `${c.class.split(".").pop()}#${c.name}:${c.outcome}`).join(",") === "FirstTest#adds:passed,FirstTest#failsTotal:failed", `跑着的时候只有跑完了的那个类，挂了的已经标出来：${JSON.stringify(mid?.tests?.cases.map((c) => c.name))}`);

await until(() => run()?.status !== "running");
await until(() => (run()?.tests?.cases.length ?? 0) === 4, 3000);
ok(run()?.tests?.cases.length === 4, `跑完：两个类 4 条都在（${run()?.tests?.cases.length}）`);
// 再等过一次轮询的间隔：跑完之后不该再被改
await sleep(1500);
ok(run()?.tests?.cases.length === 4, `跑完之后没被晚到的轮询盖回半截（${run()?.tests?.cases.length}）`);

console.log(`${fail === 0 ? "✅" : "❌"} 单测边跑边出（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
