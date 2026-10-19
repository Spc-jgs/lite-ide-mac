/**
 * 最近一次跑单测的结果，按「类#方法」和「类」记（docs/IDEA.md 第 17 节）：编辑器行号旁的 ▶ 跑完换成 ✓ / ✗（同 IDEA）。
 * 写的是运行那边（runs-ops 读完报告），读的是编辑器那边（java-test-gutter）—— 两边都是懒加载的块，这张表单独一个小文件，谁都不用把对方拽进来。
 * 只在这次运行里（不进会话快照）：上次的结果在代码改过之后不一定还对
 */
export type Outcome = "passed" | "failed";

const map = new Map<string, Outcome>();
const subs = new Set<() => void>();

/** 记下这次跑出来的（同一个类 / 方法的旧结果被盖掉）。类的结果：有一个方法没过就算没过 */
export function noteOutcomes(cases: { class: string; name: string; outcome: string }[]) {
  const classes = new Map<string, Outcome>();
  for (const c of cases) {
    if (c.outcome === "skipped") continue;
    const o: Outcome = c.outcome === "passed" ? "passed" : "failed";
    map.set(`${c.class}#${c.name.replace(/[(\[].*$/, "")}`, o);
    if (classes.get(c.class) !== "failed") classes.set(c.class, o);
  }
  for (const [k, o] of classes) map.set(k, o);
  for (const f of subs) f();
}

export const outcomeOf = (cls: string, method: string | null): Outcome | undefined => map.get(method ? `${cls}#${method}` : cls);

export function onOutcomes(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}
