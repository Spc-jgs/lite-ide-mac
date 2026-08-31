/**
 * ⌘P 模糊匹配的**刻画测试**。
 *
 * 存在的理由：`fuzzy.ts` 是个打分函数，改它极容易在不报错的情况下
 * 把排序质量弄差 —— 界面照样出结果，只是「想找的那个不在第一条了」，
 * 而这种退化肉眼要盯很久才看得出来。
 *
 * 所以这里钉死的是**相对顺序**，不是绝对分数：分数怎么调都行，
 * 只要「这个候选该排在那个前面」这层关系不变。写死具体分值会让
 * 每次调权重都得重写测试，那种测试没人会认真维护。
 *
 * 覆盖 fuzzy.ts 注释里那四条偏好，外加 rank 的稳定性和 segments 的切分。
 */
import { fuzzyMatch, rank, segments } from "../src/lib/search/fuzzy.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) {
    pass++;
  } else {
    fail++;
    console.error("  ✗ " + m);
  }
};

/** 在 files 里搜 query，返回按分数排好的路径 */
const order = (files: string[], query: string) =>
  rank(files, query, (f) => f, files.length).map((r) => r.item);

/** a 必须排在 b 前面 */
const before = (files: string[], query: string, a: string, b: string) => {
  const o = order(files, query);
  const ia = o.indexOf(a),
    ib = o.indexOf(b);
  ok(ia >= 0 && ib >= 0, `「${query}」应同时命中 ${a} 和 ${b}，实得 ${JSON.stringify(o)}`);
  ok(ia >= 0 && ib >= 0 && ia < ib, `「${query}」应把 ${a} 排在 ${b} 前，实得 ${JSON.stringify(o)}`);
};

// ── 1. 子序列匹配的基本约定 ──
ok(fuzzyMatch("src/App.svelte", "xyz") === null, "匹配不上要返回 null");
ok(fuzzyMatch("abc", "cb") === null, "顺序不对不算匹配（是子序列不是子集）");
const empty = fuzzyMatch("anything", "");
ok(empty !== null && empty.score === 0 && empty.positions.length === 0, "空 query 是 0 分空命中");
const m = fuzzyMatch("src/App.svelte", "app");
ok(m !== null && m.positions.length === 3, "命中位置数应等于 query 长度");
ok(
  m !== null && m.positions.every((p, i) => i === 0 || p > m.positions[i - 1]),
  "命中位置必须严格递增",
);
ok(
  m !== null && m.positions.map((p) => "src/App.svelte"[p].toLowerCase()).join("") === "app",
  "命中位置上的字符要真的等于 query",
);
ok(fuzzyMatch("SRC/APP.SVELTE", "app") !== null, "匹配不分大小写");
ok(fuzzyMatch("ab", "abc") === null, "query 比文本长必须是 null");
ok(fuzzyMatch("", "a") === null, "空文本配不上非空 query");
{
  const r = fuzzyMatch("src/OrderService.java", "os");
  ok(
    r !== null && r.positions.every((p) => p >= 0 && p < "src/OrderService.java".length),
    "命中位置必须落在串内（越界会让 segments 切出 undefined）",
  );
}

/*
 * ── 1b. 子串一定要能被找到 ──
 *
 * 这几条全是真实踩过的：⌘P 里打 `readme` 搜不到 `README.md`、
 * 打 `notify` 搜不到 `notify.svelte.ts`，界面直接显示「没有匹配」。
 *
 * 根因是「词首优先」的贪心会跳到一个更靠右的词首上，跳过去之后
 * 剩下的 query 就配不完了，而它没有回头路 —— 于是**明明存在的匹配被判成不匹配**。
 * 排序差一点只是烦，找不到是这个功能直接不能用。
 */
for (const [t, q] of [
  ["README.md", "readme"],
  ["src/lib/state/notify.svelte.ts", "notify"],
  ["service/order/repo.java", "order"],
  ["a/aa/aaa/target.ts", "aaat"],
]) {
  ok(fuzzyMatch(t, q) !== null, `「${q}」是 ${t} 的子序列，必须命中`);
}
// 反向：确实不是子序列的，还是要 null（别为了修上面那条把匹配放宽）
for (const [t, q] of [
  ["README.md", "readmex"],
  ["service/order/repo.java", "orderz"],
]) {
  ok(fuzzyMatch(t, q) === null, `「${q}」不是 ${t} 的子序列，必须是 null`);
}

// ── 2. 偏好一：落在文件名上，胜过落在目录名上 ──
before(
  ["app/core/model.ts", "src/App.svelte"],
  "app",
  "src/App.svelte",
  "app/core/model.ts",
);
before(
  ["service/order/repo.java", "src/OrderService.java"],
  "order",
  "src/OrderService.java",
  "service/order/repo.java",
);

// ── 3. 偏好二：连续成段，胜过东一个西一个 ──
before(
  ["o.r.d.e.r.ts", "order.ts"],
  "order",
  "order.ts",
  "o.r.d.e.r.ts",
);
before(
  ["u/s/e/rmodel.ts", "user.ts"],
  "user",
  "user.ts",
  "u/s/e/rmodel.ts",
);

// ── 4. 偏好三：落在词首（分隔符之后、或驼峰的大写处） ──
before(
  ["src/tools.ts", "src/OrderService.java"],
  "os",
  "src/OrderService.java",
  "src/tools.ts",
);
// 注意两个候选都**不连续** —— 偏好二（连续）比偏好三（词首）优先级更高，
// 拿一个连续的去比词首，测的就不是词首了
before(
  ["src/xaxb.ts", "src/a_b.ts"],
  "ab",
  "src/a_b.ts",
  "src/xaxb.ts",
);

// ── 5. 偏好四：越靠前 / 路径越浅越好 ──
before(
  ["src/deep/nested/README.md", "README.md"],
  "readme",
  "README.md",
  "src/deep/nested/README.md",
);
before(
  ["xxxxxxxxxxconfig.ts", "config.ts"],
  "config",
  "config.ts",
  "xxxxxxxxxxconfig.ts",
);

// ── 6. 综合：注释里举的那种真实场景 ──
{
  const files = [
    "src/lib/apphandler/util.ts",
    "src/App.svelte",
    "packages/x/src/apply/handler.ts",
    "app/handler.ts",
  ];
  const top = order(files, "apphandler")[0];
  ok(top === "app/handler.ts", `「apphandler」的第一条应是 app/handler.ts，实得 ${top}`);
}

/*
 * ── 6b. 四条偏好各自单独钉一遍 ──
 *
 * 上面那些整组排序的用例，往往有两三个因素同时指向同一个答案 ——
 * 于是把其中一条权重清零，测试照样绿。这里每条用例只让**一个**因素起作用，
 * 其余（命中位置、串长、是否连续、是否在文件名里）全部构造成相同。
 */
const scoreOf = (t: string, q: string) => {
  const r = fuzzyMatch(t, q);
  ok(r !== null, `${q} 应该命中 ${t}`);
  return r ? r.score : NaN;
};
const stronger = (a: string, b: string, q: string, why: string) =>
  ok(scoreOf(a, q) > scoreOf(b, q), `${why}：${a} 应该比 ${b} 分高（「${q}」）`);

// 词首 —— 五个分隔符逐个来。命中位置(2)、串长(6)、文件名加分全一样，
// 只差 b 前面那个字符。（不能写成 `a${sep}b` 比 `axb`：`/` 会把 a 推到
// 目录侧，丢掉 12 分的文件名加分，测的就不是词首了）
for (const sep of ["/", "-", "_", ".", " "]) {
  stronger(`x${sep}b.ts`, "xzb.ts", "b", `分隔符「${sep}」后面算词首`);
}
// 词首 —— 驼峰。同样只差大小写
stronger("aB", "ab", "ab", "小写接大写算词首");

// 越靠前越好：两串等长、命中都不在词首、都不连续，只差命中位置（2 vs 4）
stronger("xxqxx.ts", "xxxxq.ts", "q", "命中越靠前越好");

// 路径越短越好：命中都在 0、都是词首，只差串长
stronger("q.ts", "qxxxxxxx.ts", "q", "同样匹配时偏向短路径");

// 文件名优先：命中位置相同、串长相同、都是词首，只差 / 在命中的哪一边
stronger("ab/q.ts", "abq/.ts", "q", "落在文件名上胜过落在目录名上");

// 连续成段：命中位置起点相同、串长相同，只差第二个字符连不连着
stronger("qq__x.ts", "q__qx.ts", "qq", "连续成段胜过分散命中");

// ── 7. rank 的契约 ──
{
  const files = ["a1.ts", "a2.ts", "a3.ts", "a4.ts", "b.ts"];
  ok(rank(files, "a", (f) => f, 2).length === 2, "limit 要生效");
  ok(rank(files, "zzz", (f) => f, 10).length === 0, "全不匹配返回空");
  ok(rank([], "a", (f: string) => f, 10).length === 0, "空候选集返回空");
  /*
   * 同分保持输入顺序。⌘P 的候选是按目录序来的，同分时打乱它，
   * 表现为「同一个 query 两次结果不一样」—— 而人是靠肌肉记忆
   * 按第几条的。V8 的 Array.sort 是稳定的，换成朴素 top-K 堆就会丢掉这条。
   */
  const tie = ["dir/qq1.ts", "dir/qq2.ts", "dir/qq3.ts"];
  const scored = rank(tie, "qq", (f) => f, 10);
  ok(
    scored.length === 3 && scored[0].score === scored[1].score && scored[1].score === scored[2].score,
    "构造的同分样例应该真的同分",
  );
  ok(
    scored.map((r) => r.item).join(",") === tie.join(","),
    `同分要保持输入顺序，实得 ${scored.map((r) => r.item).join(",")}`,
  );
  /*
   * 同一条稳定性，走**有界插入**那条路再来一遍。
   *
   * 上面那组只有 3 条候选、limit 10，走的是「全量 sort」分支 ——
   * 有界插入的代码一行都没跑到。而恰恰是有界插入这条路容易把同分打乱
   * （插入点用 > 还是 >= 只差一个字符，结果就反了）。
   * 12 条候选 + limit 2 才满足 limit * 4 < items.length。
   */
  const many = Array.from({ length: 12 }, (_, i) => `dir/q${i}.ts`);
  const top2 = rank(many, "q", (f) => f, 2);
  ok(
    top2.map((r) => r.item).join(",") === "dir/q0.ts,dir/q1.ts",
    `有界插入下同分也要保持输入顺序，实得 ${top2.map((r) => r.item).join(",")}`,
  );
  // 两条路必须给出同一个答案，否则 limit 一变结果就变
  ok(
    rank(many, "q", (f) => f, 100).slice(0, 2).map((r) => r.item).join(",") ===
      top2.map((r) => r.item).join(","),
    "有界插入和全量排序的结果必须一致",
  );

  const objs = [{ p: "src/main.ts" }, { p: "src/util.ts" }];
  ok(rank(objs, "main", (o) => o.p, 5)[0].item.p === "src/main.ts", "key 取值要用在打分上");
}

// ── 8. segments：高亮切分 ──
{
  const s = segments("src/App.svelte", [4, 5, 6]);
  ok(s.map((x) => x.t).join("") === "src/App.svelte", "切分后拼回来必须是原串");
  ok(
    s.filter((x) => x.hit).map((x) => x.t).join("") === "App",
    `命中段应该是 App，实得 ${JSON.stringify(s.filter((x) => x.hit).map((x) => x.t))}`,
  );
  ok(segments("abc", []).length === 1 && !segments("abc", [])[0].hit, "没有命中就是一整段");
  ok(segments("abc", [0, 1, 2]).length === 1 && segments("abc", [0, 1, 2])[0].hit, "全命中合成一段");
}

// ── 9. 非 ASCII 不能崩，也不能乱匹配 ──
ok(fuzzyMatch("文档/说明.md", "说明") !== null, "中文能匹配");
ok(fuzzyMatch("文档/说明.md", "xyz") === null, "中文串里搜不到就是 null");
{
  const t = fuzzyMatch("İstanbul.ts", "i");
  ok(t === null || t.positions.length === 1, "土耳其点 I 之类的特殊大小写不能崩");
}

console.log(`\n${fail === 0 ? "✅" : "❌"} 模糊匹配排序：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
