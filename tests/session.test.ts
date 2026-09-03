import {
  parse,
  serialize,
  toLayout,
  DEFAULT_LAYOUT,
  VERSION,
  MAX_TABS,
  MAX_DRAFT_CHARS,
  MAX_DRAFTS_CHARS,
  type Session,
} from "../src/lib/state/session.ts";

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

const base: Session = {
  root: "/proj",
  tabs: [{ path: "/proj/a.ts", line: 12 }, { path: "/proj/b.md" }],
  active: 1,
  layout: { ...DEFAULT_LAYOUT, sidebarWidth: 300, panel: true },
};

// ── 1. 存进去能原样读回来 ──
const round = parse(serialize(base));
ok(round !== null, "序列化再解析不该是 null");
ok(round?.root === "/proj", "项目根要还原");
ok(round?.tabs.length === 2, "标签数要还原");
ok(round?.tabs[0].line === 12, "光标行要还原");
ok(round?.tabs[1].line === undefined, "没记光标的标签不该凭空多出一个行号");
ok(round?.active === 1, "活动标签下标要还原");
ok(round?.layout.sidebarWidth === 300 && round?.layout.panel === true, "布局要还原");

// ── 2. 坏数据一律返回 null，绝不抛 ──
// 这段跑在启动路径上：抛一次应用就打不开，而用户没法清掉那份坏数据
for (const bad of [
  null,
  undefined,
  "",
  "{",
  "not json",
  "[]",
  "null",
  '"just a string"',
  "123",
]) {
  let threw = false;
  let r: Session | null = null;
  try {
    r = parse(bad as string);
  } catch {
    threw = true;
  }
  ok(!threw, `parse(${JSON.stringify(bad)}) 不该抛`);
  ok(r === null, `parse(${JSON.stringify(bad)}) 该返回 null`);
}

// ── 3. 版本不认就整份丢 ──
const old = JSON.stringify({ v: VERSION - 1, root: "/x", tabs: [{ path: "/x/a" }], active: 0 });
ok(parse(old) === null, "旧版本快照要整份丢弃，不能将就着读");
const noVer = JSON.stringify({ root: "/x", tabs: [], active: 0 });
ok(parse(noVer) === null, "没有版本号的快照也要丢");

// ── 4. 尺寸要夹回可用区间 ──
// 一个 4000px 的侧边栏会把内容区挤没，而拖动手柄本身就在屏幕外，拉不回来
const huge = toLayout({ sidebarWidth: 4000, panelHeight: 99999 });
ok(huge.sidebarWidth <= 640, `侧边栏宽 ${huge.sidebarWidth} 应被夹到 640 以内`);
ok(huge.panelHeight <= 900, `面板高 ${huge.panelHeight} 应被夹到 900 以内`);
const tiny = toLayout({ sidebarWidth: -50, panelHeight: 0 });
ok(tiny.sidebarWidth >= 160, `侧边栏宽 ${tiny.sidebarWidth} 应被夹到 160 以上`);
ok(tiny.panelHeight >= 80, `面板高 ${tiny.panelHeight} 应被夹到 80 以上`);
const nan = toLayout({ sidebarWidth: NaN, panelHeight: Infinity });
ok(nan.sidebarWidth === DEFAULT_LAYOUT.sidebarWidth, "NaN 宽度要回默认值");
ok(nan.panelHeight === DEFAULT_LAYOUT.panelHeight, "Infinity 高度要回默认值");

// ── 5. 枚举字段只认已知值 ──
ok(toLayout({ sideView: "外星视图" }).sideView === "files", "不认识的 sideView 回 files");
ok(toLayout({ panelView: "外星视图" }).panelView === "term", "不认识的 panelView 回 term");
ok(toLayout({}).sidebar === true, "缺字段用默认值");
ok(toLayout(null).panelHeight === 260, "整个 layout 缺失也要给一份默认");

// ── 6. 标签列表的清洗 ──
const dirty = JSON.stringify({
  v: VERSION,
  root: "/p",
  tabs: [
    { path: "/p/a" },
    { path: "/p/a" }, // 重复
    { path: "" }, // 空路径
    { line: 3 }, // 没有路径
    null,
    { path: "/p/b", line: -5 }, // 非法行号
    { path: "/p/c", line: 2.7 }, // 小数行号
  ],
  active: 0,
  layout: DEFAULT_LAYOUT,
});
const cleaned = parse(dirty);
ok(cleaned?.tabs.length === 3, `脏数据清洗后应剩 3 个，实际 ${cleaned?.tabs.length}`);
ok(cleaned?.tabs.map((t) => t.path).join(",") === "/p/a,/p/b,/p/c", "去重且保持顺序");
ok(cleaned?.tabs[1].line === undefined, "负数行号要丢掉，不能变成负行");
ok(cleaned?.tabs[2].line === 2, "小数行号要取整");

// ── 7. 标签数量有上限 ──
// localStorage 写满会抛，抛在启动路径上就是打不开
const many = JSON.stringify({
  v: VERSION,
  root: "/p",
  tabs: Array.from({ length: MAX_TABS + 50 }, (_, i) => ({ path: `/p/f${i}` })),
  active: 0,
  layout: DEFAULT_LAYOUT,
});
ok(parse(many)?.tabs.length === MAX_TABS, `超量标签要截到 ${MAX_TABS}`);
const big: Session = {
  ...base,
  tabs: Array.from({ length: MAX_TABS + 50 }, (_, i) => ({ path: `/p/f${i}` })),
  active: 60,
};
ok(JSON.parse(serialize(big)).tabs.length === MAX_TABS, "序列化时也要截");
ok(JSON.parse(serialize(big)).active === MAX_TABS - 1, "活动下标不能指到被截掉的位置");

// ── 8. 活动下标越界要收回来 ──
const oob = JSON.stringify({
  v: VERSION,
  root: "/p",
  tabs: [{ path: "/p/a" }],
  active: 99,
  layout: DEFAULT_LAYOUT,
});
ok(parse(oob)?.active === 0, "越界的活动下标要收回 0");
const empty = JSON.stringify({ v: VERSION, root: "/p", tabs: [], active: 5, layout: DEFAULT_LAYOUT });
ok(parse(empty)?.active === 0, "没有标签时活动下标是 0");

// ── 9. 空会话也要能表达 ──
const blank = parse(serialize({ root: null, tabs: [], active: 0, layout: DEFAULT_LAYOUT }));
ok(blank !== null && blank.root === null && blank.tabs.length === 0, "空会话要能存能读");

// ── 10. 草稿：存得住，也拦得住 ──

const 带草稿: Session = {
  root: "/p",
  tabs: [{ path: "/p/a.ts", line: 3, draft: "改了一半", stamp: { mtimeMs: 111, size: 22 } }],
  active: 0,
  layout: DEFAULT_LAYOUT,
};
const 回来 = parse(serialize(带草稿));
ok(回来?.tabs[0].draft === "改了一半", "草稿要能存能读");
ok(回来?.tabs[0].stamp?.mtimeMs === 111 && 回来?.tabs[0].stamp?.size === 22, "指纹跟着草稿一起存");
ok(回来?.tabs[0].line === 3, "草稿不影响原有字段");

// 单份超限：只丢这一份草稿，标签本身照存
const 超长: Session = {
  root: "/p",
  tabs: [{ path: "/p/big.ts", draft: "x".repeat(MAX_DRAFT_CHARS + 1) }, { path: "/p/ok.ts", draft: "小的" }],
  active: 0,
  layout: DEFAULT_LAYOUT,
};
const 超长回来 = parse(serialize(超长));
ok(超长回来?.tabs.length === 2, "草稿超限不能把标签也丢掉");
ok(超长回来?.tabs[0].draft === undefined, "超限的那份草稿要丢掉");
ok(超长回来?.tabs[1].draft === "小的", "别人的草稿不受牵连");

// 总额度：先来的存下，后面的丢掉
const 每份 = MAX_DRAFT_CHARS;
const 份数 = Math.floor(MAX_DRAFTS_CHARS / 每份) + 1;
const 一堆: Session = {
  root: "/p",
  tabs: Array.from({ length: 份数 }, (_, i) => ({ path: `/p/${i}.ts`, draft: "y".repeat(每份) })),
  active: 0,
  layout: DEFAULT_LAYOUT,
};
const 一堆回来 = parse(serialize(一堆));
const 存下的 = (一堆回来?.tabs ?? []).filter((t) => t.draft !== undefined).length;
ok(一堆回来?.tabs.length === 份数, "总额度用完也不能丢标签");
ok(存下的 === Math.floor(MAX_DRAFTS_CHARS / 每份), `总额度要卡住，实得 ${存下的} 份`);

// 退一步的那一档：写不下时把草稿全去掉重存
const 无草稿 = parse(serialize(带草稿, false));
ok(无草稿?.tabs[0].draft === undefined, "withDrafts=false 时不写草稿");
ok(无草稿?.tabs[0].path === "/p/a.ts" && 无草稿?.tabs[0].line === 3, "退一步也要保住路径和行号");

// 坏数据一律当没有，不能抛
const 坏草稿 = JSON.stringify({
  v: VERSION,
  root: "/p",
  tabs: [
    { path: "/p/a.ts", draft: 42 },
    { path: "/p/b.ts", draft: "有草稿没指纹" },
    { path: "/p/c.ts", draft: "指纹是坏的", stamp: { mtimeMs: "x" } },
    { path: "/p/d.ts", draft: "" },
  ],
  active: 0,
  layout: DEFAULT_LAYOUT,
});
const 坏的回来 = parse(坏草稿);
ok(坏的回来?.tabs[0].draft === undefined, "draft 不是字符串就当没有");
ok(坏的回来?.tabs[1].draft === "有草稿没指纹", "没有指纹的草稿照样收（恢复时按「盘上可能变过」处理）");
ok(坏的回来?.tabs[1].stamp === undefined, "缺指纹就是 undefined");
ok(坏的回来?.tabs[2].draft === "指纹是坏的" && 坏的回来?.tabs[2].stamp === undefined, "指纹坏了只丢指纹");
ok(坏的回来?.tabs[3].draft === undefined, "空串草稿当没有");
ok(坏的回来?.tabs.length === 4, "坏草稿不能连累标签");

console.log(`${fail === 0 ? "✅" : "❌"} 会话快照：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
