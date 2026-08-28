import {
  BLOCK_LINES,
  MAX_BLOCKS,
  MAX_CHARS,
  MIN_BLOCKS,
  overBudget,
} from "../src/lib/logview/cache-budget.ts";

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

// 1. 空缓存与常规水位不驱逐
ok(!overBudget(0, 0), "空缓存不该驱逐");
ok(!overBudget(MAX_BLOCKS, MAX_CHARS), "正好压线不该驱逐");

// 2. 块数超了就驱逐，跟字节多少无关
ok(overBudget(MAX_BLOCKS + 1, 0), "块数超上限必须驱逐");

// 3. 普通日志（一行约 120 字符）塞满块数上限也撞不到字节线 ——
//    这条保证「加了字节预算之后短行文件的行为一个字都没变」
const 常规字符数 = MAX_BLOCKS * BLOCK_LINES * 120;
ok(
  常规字符数 < MAX_CHARS,
  `常规日志塞满 ${MAX_BLOCKS} 块只有 ${常规字符数.toLocaleString()} 字符，` +
    `不该超过 ${MAX_CHARS.toLocaleString()} —— 否则短行文件的缓存被无谓地砍小了`,
);
ok(!overBudget(MAX_BLOCKS, 常规字符数), "常规日志满载不该被字节预算赶");

// 4. 长行文件（一行 10KB，正是日志模式的触发条件之一）字节超标要驱逐
const 长行每块 = BLOCK_LINES * 10_000;
ok(overBudget(20, 20 * 长行每块), "长行文件字节超标必须驱逐");

// 5. 但不能赶到没法滚动：保底 MIN_BLOCKS 块
ok(
  !overBudget(MIN_BLOCKS, MIN_BLOCKS * 长行每块),
  `字节再超也要留够 ${MIN_BLOCKS} 块，否则装不下一屏上下文，滚动会来回抖`,
);
ok(overBudget(MIN_BLOCKS + 1, (MIN_BLOCKS + 1) * 长行每块), "比保底多一块时还该继续赶");

// 6. 最坏占用有上限：长行文件稳定在 MIN_BLOCKS 块
const 最坏字符 = MIN_BLOCKS * 长行每块;
ok(
  最坏字符 < 25_000_000,
  `10KB/行的最坏占用 ${(最坏字符 / 1e6).toFixed(1)}M 字符（约 ${(
    (最坏字符 * 2) /
    1e6
  ).toFixed(0)}MB）应远在 200MB 预算之内`,
);

console.log(`${fail === 0 ? "✅" : "❌"} 行缓存预算：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
