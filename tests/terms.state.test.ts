/**
 * 终端的左右拆分（`state/terms.svelte.ts`，docs/TAKEOVER.md G20 / #34 后半）：在当前那个的目录里新开一个、并排；关掉其中一个就不拆了
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { terms } = await import("../src/lib/state/terms.svelte");

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

terms.split();
ok(terms.pair === null && terms.list.length === 0, "一个终端都没有：拆不了，什么都不做");

const a = terms.open("/proj/web");
terms.split();
const b = terms.list[terms.list.length - 1];
ok(terms.pair?.[0] === a.id && terms.pair?.[1] === b.id, `拆开：[原来那个, 新的]（${JSON.stringify(terms.pair)}）`);
ok(b.cwd === "/proj/web", "新的那个开在原来那个的目录里，不是项目根");
ok(terms.activeId === b.id, "新开的那个是当前的");

terms.open("/proj");
ok(terms.pair !== null, "开第三个：那一对还在（切回去照样并排）");
terms.close(a.id);
ok(terms.pair === null, "关掉那一对里的一个：不拆了");

terms.closeAll();
console.log(`${fail === 0 ? "✅" : "❌"} 终端拆分：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
