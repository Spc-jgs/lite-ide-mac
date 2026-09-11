import {
  invariant,
  setInvariantSink,
  resetInvariants,
  invariantCount,
  shouldReport,
  tabFaults,
  tabsFaults,
  audit,
  type TabLike,
} from "../src/lib/state/invariant.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

/** 装一个假通道，返回收到的那些行 */
function 收集(): string[] {
  const got: string[] = [];
  setInvariantSink((m) => got.push(m));
  resetInvariants();
  return got;
}

// ── 不抛、不打断 ───────────────────────────────────────────────────

/*
 * 这是这个模块**唯一的硬要求**：自检失败绝不能把应用带下去。
 * 它比下面所有具体的不变量都重要 —— 自检是新代码，它比被它检查的
 * 那些东西更可能出错。
 */
{
  const got = 收集();
  ok(invariant(true, "成立的那条") === true, "成立时返回 true");
  ok(got.length === 0, "成立时一个字都不写");
  ok(invariant(false, "不成立的那条") === false, "不成立时返回 false，而不是抛");
  ok(got.length === 1, "不成立时写一行");
}

/*
 * 没装通道时必须是彻底的空操作。裸 node 跑别的测试、或者 `main.ts`
 * 还没接上之前，都不该有落点 —— 也不该因此炸掉。
 */
{
  resetInvariants();
  setInvariantSink(() => {
    throw new Error("通道自己炸了");
  });
  let 炸了 = false;
  try {
    invariant(false, "通道坏掉的时候");
  } catch {
    炸了 = true;
  }
  ok(!炸了, "通道自己抛异常时，invariant 不能跟着往上抛 —— 那就是「点了标签没反应」");
  ok(invariantCount("通道坏掉的时候") === 1, "写不出去也要照常计数");
  setInvariantSink(() => {});
}

// ── 收敛：2 的幂 ───────────────────────────────────────────────────

ok(
  [1, 2, 4, 8, 16, 1024].every(shouldReport),
  "2 的幂要写",
);
ok(
  [3, 5, 6, 7, 9, 100, 1023].every((n) => !shouldReport(n)),
  "不是 2 的幂就闭嘴",
);
ok(!shouldReport(0), "0 次不算失败，不该写");

{
  const got = 收集();
  for (let i = 0; i < 100; i++) invariant(false, "一直不成立的那条");
  ok(invariantCount("一直不成立的那条") === 100, "计数要数满 100 次");
  ok(got.length === 7, `100 次只写 7 行（1/2/4/8/16/32/64），实得 ${got.length}`);
  ok(got[0] === "一直不成立的那条", "第一次不带次数 —— 那时它还只是「发生了」");
  ok(got[6].includes("第 64 次"), "后面每一行都要自带次数，量级要直接读得出来");
}

/*
 * **收敛的单位是 key，不是整条消息。**
 * 把路径拼进 key 的话，二十个标签就是二十个计数器，收敛等于没有 ——
 * 而「同一条不变量在二十个标签上都不成立」恰恰是最该只说一次的情况。
 */
{
  const got = 收集();
  for (let i = 0; i < 20; i++) invariant(false, "同一条", `第 ${i} 个标签`);
  ok(invariantCount("同一条") === 20, "detail 不同也算同一条");
  ok(got.length === 5, `20 次写 5 行（1/2/4/8/16），实得 ${got.length}`);
  ok(got[0].includes("第 0 个标签"), "detail 要带上，否则不知道是哪个标签");
}

// ── 标签自己的不变量 ───────────────────────────────────────────────

const 干净的编辑标签: TabLike = {
  id: 1, path: "/p/a.ts", mode: "edit", dirty: false, content: "x",
};
const 正常的日志标签: TabLike = {
  id: 2, path: "/p/a.log", mode: "log", dirty: false, handle: 7,
};

ok(tabFaults(干净的编辑标签, false).length === 0, "干净的编辑标签没有问题");
ok(tabFaults(正常的日志标签, false).length === 0, "正常的日志标签没有问题");

/*
 * 日志模式没句柄 = 空白面板。这不是假想的状态：`doSwitch` 的 catch 分支
 * 真能走到 —— closeLog 成功之后 readText 抛了，mode 还停在 log，句柄已经没了。
 */
{
  const 没句柄 = { ...正常的日志标签, handle: undefined };
  const f = tabFaults(没句柄, false);
  ok(f.length === 1 && f[0][0].includes("没有引擎句柄"), "日志模式必须有句柄");
  ok(f[0][1].includes("a.log"), "要说清是哪个标签");
}

// 反方向是 mmap 句柄泄漏：文件一直被映射着，进程内存不降
{
  const f = tabFaults({ ...干净的编辑标签, handle: 9 }, false);
  ok(f.length === 1 && f[0][0].includes("还拿着引擎句柄"), "编辑模式不该有句柄");
}

/*
 * **dirty 与 draft 同真同假这条，对正在被编辑的那个标签不成立。**
 *
 * 编辑器的 onChange 只改 dirty，draft 要等 onStash（换文件/销毁）才回写。
 * 也就是说活动标签本来就会「dirty 而无 draft」—— 把它算进去，
 * 这条会在**每一次敲键盘**时报假警，而那正是最不该有噪音的时候。
 */
{
  const 正在打字 = { ...干净的编辑标签, dirty: true };
  ok(tabFaults(正在打字, true).length === 0, "活动标签 dirty 而无 draft 是正常的");
  const f = tabFaults(正在打字, false);
  ok(f.length === 1 && f[0][0].includes("同真同假"), "非活动标签 dirty 就必须有草稿");
}
{
  // 反过来：有草稿却说不脏，⌘W 会当它干净直接关掉，改动就真没了
  const f = tabFaults({ ...干净的编辑标签, draft: "改过的" }, false);
  ok(f.length === 1 && f[0][0].includes("同真同假"), "有草稿就必须是脏的");
}

/*
 * `stashed` 的后置条件：草稿和盘上那份一样时它会把草稿丢掉。
 * 留着一份「和磁盘相同的草稿」，基线判断从此多一层拐弯 —— 而且它还得是脏的
 * （上一条），于是界面会说一个没改过的文件有未保存改动。
 */
{
  const f = tabFaults({ ...干净的编辑标签, draft: "x", dirty: true }, false);
  ok(f.length === 1 && f[0][0].includes("草稿和磁盘内容相同"), "和磁盘一样的草稿该被丢掉");
  // 活动标签也要检查这一条 —— 它和「还没回写」没有关系
  ok(tabFaults({ ...干净的编辑标签, draft: "x", dirty: true }, true).length === 1,
     "这一条对活动标签同样成立");
}

// ── 标签之间的不变量 ───────────────────────────────────────────────

ok(tabsFaults([干净的编辑标签, 正常的日志标签], 1).length === 0, "正常的一组没有问题");
ok(tabsFaults([], null).length === 0, "空界面没有问题");

{
  const f = tabsFaults([干净的编辑标签, { ...干净的编辑标签, id: 9 }], 1);
  ok(f.length === 1 && f[0][0].includes("两个标签"), "同一个文件不能开两个标签");
}

/*
 * 共用句柄：关掉任意一个，另一个当场变空白，而且第二次 close_log
 * 打在一个已经回收的句柄上。
 */
{
  const f = tabsFaults([正常的日志标签, { ...正常的日志标签, id: 3, path: "/p/b.log" }], 2);
  ok(f.length === 1 && f[0][0].includes("共用一个引擎句柄"), "句柄不能共用");
  ok(f[0][1].includes("#2") && f[0][1].includes("#3"), "要说清是哪两个");
}
// 两个标签都没句柄不算共用 —— undefined 不是一个句柄值
ok(tabsFaults([干净的编辑标签, { ...干净的编辑标签, id: 9, path: "/p/b.ts" }], 1).length === 0,
   "都没句柄不该被当成共用");

{
  const f = tabsFaults([干净的编辑标签], 99);
  ok(f.length === 1 && f[0][0].includes("不存在的标签"), "activeId 必须指得着");
}
{
  const f = tabsFaults([干净的编辑标签], null);
  ok(f.length === 1 && f[0][0].includes("有标签但没有活动标签"), "有标签就必须有活动的那个");
}

// ── audit：把两组一起跑，并且认得出哪个是活的 ──────────────────────

{
  const got = 收集();
  const 正在打字 = { ...干净的编辑标签, dirty: true };
  audit([正在打字], 1, 1, "切标签");
  ok(got.length === 0, "audit 要把 liveId 传下去，否则正在打字的标签会报假警");

  audit([正在打字], 1, null, "关标签");
  ok(got.length === 1, "同一组标签，没有活编辑器时就该报");
  ok(got[0].includes("@关标签"), "要带上转换点，不然不知道是在哪一步坏的");
}

/*
 * **转换点只进 detail，不进 key。** 同一条不变量在切标签和关标签上
 * 各报一次是同一个 bug，不该占两个计数器 —— 那样收敛会被稀释掉。
 */
{
  const got = 收集();
  const 坏的 = { ...干净的编辑标签, dirty: true };
  audit([坏的], 1, null, "切标签");
  audit([坏的], 1, null, "关标签");
  audit([坏的], 1, null, "切模式");
  ok(invariantCount("dirty 与 draft 不同真同假") === 3, "三个转换点算同一条");
  ok(got.length === 2, `三次只写 2 行（第 1、2 次），实得 ${got.length}`);
}

console.log(`${fail === 0 ? "✅" : "❌"} 运行时不变量：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
