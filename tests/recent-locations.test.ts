/**
 * 最近位置（`shell/recent-locations.ts`，docs/TAKEOVER.md G6）：跳转历史新的在前、再接最近文件停在的那一行；同一处只留一条；没记过位置的文件不列
 */
import { spotsOf } from "../src/lib/shell/recent-locations.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const eq = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

const back = [{ path: "/p/A.java", line: 10 }, { path: "/p/B.java", line: 3 }, { path: "/p/A.java", line: 42 }];
const pos: Record<string, number> = { "/p/B.java": 3, "/p/C.java": 7 };
const got = spotsOf(back, ["/p/B.java", "/p/C.java", "/p/D.java"], (p) => pos[p]);
ok(eq(got.map((s) => `${s.path.slice(3)}:${s.line}`), ["A.java:42", "B.java:3", "A.java:10", "C.java:7"]), `跳转历史新的在前、B:3 只留一条、D 没位置不列：${JSON.stringify(got)}`);
ok(spotsOf(Array.from({ length: 50 }, (_, i) => ({ path: "/p/X.java", line: i + 1 })), [], () => undefined, 30).length === 30, "最多 30 条");
ok(eq(spotsOf([], [], () => 1), []), "什么都没有：空的");

console.log(`${fail === 0 ? "✅" : "❌"} 最近位置：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
