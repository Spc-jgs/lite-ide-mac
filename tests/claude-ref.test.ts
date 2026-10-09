// 「发送到终端」写进去的 @ 引用（src/lib/terminal/claude-ref.ts，issue #45）
import { lineSpan, mention, refPath } from "../src/lib/terminal/claude-ref.ts";

let pass = 0,
  fail = 0;
const eq = (a: unknown, b: unknown, m: string) => {
  if (JSON.stringify(a) === JSON.stringify(b)) pass++;
  else {
    fail++;
    console.error(`  ✗ ${m}：${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
  }
};

/*
 * 拿 Claude Code 自己的正则反解一遍（2.1.293 安装包里读出来的那几条）：只比字符串的话，
 * 测的是「我们以为的格式」；反解测的是「claude 拿到这串会读哪个文件的哪几行」。
 * 引号那条在安装包里是断开的（`@(?:"([^"\n]{1,` 后面看不全），这里按能看到的部分补成 `[^"\n]+`。
 */
const QUOTED = /(?:^|\s)@"([^"\n]+)"/g;
const BARE = /(?:^|\s)@((?:[^\s\\]|\\ )+)/g;
const RANGE = /^([^#]+)(?:#L(\d+)(?:-(\d+))?)?(?:#[^#]*)?$/;
function parse(s: string) {
  const out: { path: string; from?: number; to?: number }[] = [];
  const take = (raw: string) => {
    const m = RANGE.exec(raw);
    if (!m) return;
    const from = m[2] ? Number(m[2]) : undefined;
    out.push({ path: m[1], ...(from ? { from, to: m[3] ? Number(m[3]) : from } : {}) });
  };
  const rest = s.replace(QUOTED, (_, p: string) => (take(p), " "));
  for (const m of rest.matchAll(BARE)) take(m[1].replace(/\\ /g, " "));
  return out;
}

// ── 行范围 ──
eq(lineSpan(10, 20, 5), { from: 10, to: 20 }, "普通选区");
eq(lineSpan(10, 21, 0), { from: 10, to: 20 }, "终点停在下一行行首：那一行不算（⇧↓ 选整行）");
eq(lineSpan(10, 10, 0), { from: 10, to: 10 }, "同一行里从行首选起：不能减成第 9 行");
eq(lineSpan(10, 11, 3), { from: 10, to: 11 }, "终点在下一行中间：那一行算");

// ── 相对谁 ──
eq(refPath("/p/src/a.ts", "/p"), "src/a.ts", "在前台进程目录底下：相对");
eq(refPath("/p/src/a.ts", "/p/"), "src/a.ts", "目录带尾斜杠");
eq(refPath("/p/src/a.ts", "/p/src"), "a.ts", "cd 进子目录起的 claude");
eq(refPath("/p/src/a.ts", "/p/test"), "/p/src/a.ts", "不在底下：绝对路径");
eq(refPath("/pp/a.ts", "/p"), "/pp/a.ts", "前缀相同但不是子目录");
eq(refPath("/p/a.ts", null), "/p/a.ts", "拿不到目录：绝对路径");

// ── 拼出来的字 ──
eq(mention("src/a.ts", [{ from: 10, to: 20 }]), " @src/a.ts#L10-20 ", "一段");
eq(mention("src/a.ts", [{ from: 7, to: 7 }]), " @src/a.ts#L7 ", "单行不写成 7-7");
eq(mention("src/a.ts", []), " @src/a.ts ", "没选：整个文件");
eq(
  mention("a.ts", [{ from: 30, to: 31 }, { from: 2, to: 3 }, { from: 2, to: 3 }]),
  " @a.ts#L2-3 @a.ts#L30-31 ",
  "多段：按行排、重复的只给一次",
);
eq(mention("my dir/a b.ts", [{ from: 1, to: 2 }]), ' @"my dir/a b.ts#L1-2" ', "带空格：引号");

// ── claude 读出来的是不是那个文件那几行 ──
const cases: [string, { from: number; to: number }[]][] = [
  ["src/a.ts", [{ from: 10, to: 20 }]],
  ["src/a.ts", [{ from: 7, to: 7 }]],
  ["src/a.ts", []],
  ["my dir/a b.ts", [{ from: 1, to: 2 }]],
  ['odd "q" name.ts', [{ from: 4, to: 9 }]],
  ["/abs/x (1).ts", [{ from: 3, to: 3 }]],
  ["中文/文件.md", [{ from: 1, to: 5 }]],
];
for (const [p, spans] of cases) {
  const want = spans.length ? spans.map((s) => ({ path: p, ...s })) : [{ path: p }];
  eq(parse(`看一下${mention(p, spans)}这里`), want, `claude 反解：${p}`);
}
eq(parse(`看一下@src/a.ts#L1`), [], "反例：@ 前面没空白，claude 不认 —— 所以 mention 开头要留空格");

// ── 路径里有 #：没法 @，退成纯路径 ──
eq(mention("docs/c#.md", [{ from: 3, to: 4 }]), " docs/c#.md:3-4 ", "有 #：不带 @");
eq(parse(mention("docs/c#.md", [{ from: 3, to: 4 }])), [], "有 #：claude 不会当成引用去读别的文件");
eq(mention("my docs/c#.md", []), ' "my docs/c#.md" ', "有 # 又有空格：套引号");

console.log(`${fail === 0 ? "✅" : "❌"} 发送到终端的 @ 引用：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
