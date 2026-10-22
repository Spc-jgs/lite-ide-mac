/**
 * 层级工具窗的状态（`state/hierarchy.svelte.ts`，docs/TAKEOVER.md G10）：打开就问第一层；递归的节点不让展开；
 * 换方向时还在路上的那份答案扔掉；展开 / 收起之后看得见的行对得上
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { hierarchy } = await import("../src/lib/state/hierarchy.svelte");
import type { JavaHierNode } from "../src/lib/ipc/commands";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const node = (name: string, line: number): JavaHierNode => ({
  name,
  detail: "Demo",
  kind: 6,
  at: { uri: "file:///p/Demo.java", path: "/p/Demo.java", label: "Demo.java", line, character: 4, endLine: line, endCharacter: 8, text: "" },
  site: null,
  raw: JSON.stringify({ name }),
});
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// 调用关系：a ← b、a ← c；b ← a（a 和 b 互相调用）
const callers: Record<string, JavaHierNode[]> = { a: [node("b", 10), node("c", 20)], b: [node("a", 1)], c: [] };
let delay = 0;
const asked: string[] = [];
hierarchy.fetch = async (_root, raw, dir) => {
  const n = JSON.parse(raw).name as string;
  asked.push(`${dir}:${n}`);
  await sleep(delay);
  const items = dir === "in" ? (callers[n] ?? []) : [node(`${n}-callee`, 30)];
  return { items, total: items.length };
};

hierarchy.open("/p", node("a", 1), false);
await sleep(5);
ok(hierarchy.dir === "in" && hierarchy.rows.map((r) => r.node.name).join() === "a,b,c", `打开：默认看「谁调了它」、第一层就问了：${hierarchy.rows.map((r) => r.node.name).join()}`);

const b = hierarchy.rows[1];
await hierarchy.expand(b);
const a2 = hierarchy.rows[2];
ok(a2.node.name === "a" && a2.cycle, "b 的调用方里又出现了 a（根本身）：标成递归");
const before = asked.length;
await hierarchy.expand(a2);
ok(asked.length === before && !a2.open, "递归的节点点不开、不再问 —— 不然点几下就是无底洞");
ok(hierarchy.rows.map((r) => `${r.depth}${r.node.name}`).join() === "0a,1b,2a,1c", `展开之后的行：${hierarchy.rows.map((r) => `${r.depth}${r.node.name}`).join()}`);
hierarchy.collapse(hierarchy.rows[1]);
ok(hierarchy.rows.length === 3, "收起 b：它的孩子不在看得见的行里");
await hierarchy.expand(hierarchy.rows[1]);
ok(asked.filter((x) => x === "in:b").length === 1, "再展开不重问（问过的留着）");

// 换方向时第一层还在路上：那份答案对不上新树了，扔掉
delay = 30;
hierarchy.setDir("out");
hierarchy.setDir("in");
await sleep(80);
ok(hierarchy.rows.map((r) => r.node.name).join() === "a,b,c", `连换两次方向，晚到的「它调了谁」没混进来：${hierarchy.rows.map((r) => r.node.name).join()}`);
ok(!hierarchy.rows[0].loading, "最后那次的环停了");

hierarchy.open("/p", node("PriceRule", 2), true);
ok(hierarchy.types && hierarchy.dir === "sub", "类型层级默认看子类型（接口上按 ⌃H 最常见的是找实现）");

console.log(`${fail === 0 ? "✅" : "❌"} 层级工具窗：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
