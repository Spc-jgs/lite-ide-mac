import {
  parse,
  serialize,
  toLayout,
  DEFAULT_LAYOUT,
  VERSION,
  MAX_TABS,
  MAX_DRAFT_CHARS,
  MAX_DRAFTS_CHARS,
  RECENT_MAX,
  withoutTabs,
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
  recent: ["/proj", "/other"],
  recentFiles: ["/proj/a.ts", "/Users/me/x.log"],
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
ok(round?.recent.length === 2 && round?.recent[0] === "/proj", "最近打开要还原");
ok(round?.recentFiles?.length === 2 && round?.recentFiles?.[0] === "/proj/a.ts", "最近文件（⌘E）要还原，顺序不变");
ok(
  parse(JSON.stringify({ v: VERSION, root: null, tabs: [], active: 0, layout: DEFAULT_LAYOUT, recent: [] }))?.recentFiles?.length === 0,
  "没有 recentFiles 字段（老快照）要退成空列表",
);
ok(
  parse(JSON.stringify({ v: VERSION, root: null, tabs: [], active: 0, layout: DEFAULT_LAYOUT, recent: [], recentFiles: ["/a", 3, "", "/a", "/b"] }))?.recentFiles?.join() === "/a,/b",
  "最近文件里的坏数据和重复要清掉",
);

// ── 最近打开：坏数据一律当它不存在 ──

/*
 * 这几条和别的字段同一条判据 —— 这段在启动路径上，抛一次应用就打不开。
 * 值可能来自上一个版本、也可能被人手改过。
 */
{
  const bad = (recent: unknown) =>
    parse(JSON.stringify({ v: VERSION, root: null, tabs: [], active: 0, layout: DEFAULT_LAYOUT, recent }));
  ok(bad(undefined)?.recent.length === 0, "没有 recent 字段（旧版本存的）要退成空列表");
  ok(bad("不是数组")?.recent.length === 0, "不是数组就当空的");
  ok(bad([1, null, {}])?.recent.length === 0, "元素不是字符串就跳过");
  ok(bad(["/a", "/a", "/b"])?.recent.length === 2, "重复的要去掉");
  ok(bad(["/a", ""])?.recent.length === 1, "空串不算一个项目");
  const many = Array.from({ length: 20 }, (_, i) => `/p${i}`);
  ok(bad(many)?.recent.length === RECENT_MAX, `超过 ${RECENT_MAX} 条要截断`);
  ok(bad(many)?.recent[0] === "/p0", "截断保留的是最前面那些（最新的排最前）");
}

// serialize 少给字段也不能抛 —— 它和 parse 一样贴着保存路径
{
  let threw = false;
  try {
    // @ts-expect-error 故意少给 recent，模拟调用方漏传
    serialize({ root: null, tabs: [], active: 0, layout: DEFAULT_LAYOUT });
  } catch {
    threw = true;
  }
  ok(!threw, "serialize 少一个字段不该抛 —— 抛一次这一轮的现场就没了");
}

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
ok(toLayout({ panelView: "git", gitTab: "外星" }).gitTab === "log", "不认识的 gitTab 回 log");

// ── 5b. v1.0.0 以前的三档 panelView 要无损映射进两层（#31）──
// 老的 `log` = 提交历史工具窗，`git` = Git 控制台工具窗；现在都是 Git 窗里的标签
{
  const oldLog = toLayout({ panelView: "log" });
  ok(oldLog.panelView === "git" && oldLog.gitTab === "log", "老 log → Git 窗 + Log 标签");
  const oldCon = toLayout({ panelView: "git" });
  ok(oldCon.panelView === "git" && oldCon.gitTab === "console", "老 git → Git 窗 + Console 标签");
  // 新格式一定带 gitTab，这时 panelView 的 git 只是「Git 窗」，标签听 gitTab 的
  const fresh = toLayout({ panelView: "git", gitTab: "log" });
  ok(fresh.panelView === "git" && fresh.gitTab === "log", "新格式 git + log 不能被当成老的 console");
  ok(toLayout({ panelView: "term" }).gitTab === "log", "终端上的老快照，Git 标签默认 Log");
}
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

// ── 预览标签（issue #33 ⑯）：只存 true、只认 true ──

/*
 * 旧快照没有这个字段。「没有」和 false 是同一个意思，所以 VERSION 不动 ——
 * 但这也意味着解析必须对一切非 true 的值装作没看见，包括看着像真的 "true"。
 */
{
  const withPreview = JSON.stringify({
    v: VERSION,
    root: "/proj",
    tabs: [{ path: "/proj/a" }, { path: "/proj/b", preview: true }],
    active: 0,
    layout: DEFAULT_LAYOUT,
  });
  const got = parse(withPreview);
  ok(got?.tabs[0].preview === undefined, "没有 preview 字段的标签就是没有");
  ok(got?.tabs[1].preview === true, "preview: true 要读回来");
  const back = parse(serialize(got!));
  ok(back?.tabs[1].preview === true && back?.tabs[0].preview === undefined, "预览标记要经得起一来一回");

  const 假的 = JSON.stringify({
    v: VERSION,
    root: "/proj",
    tabs: [{ path: "/a", preview: "true" }, { path: "/b", preview: 1 }, { path: "/c", preview: false }],
    active: 0,
    layout: DEFAULT_LAYOUT,
  });
  const g2 = parse(假的);
  ok(g2?.tabs.every((t) => t.preview === undefined), "非 true 的 preview 一律当没有");

  // 钉住（issue #33 ⑰）同一套规矩
  const pinned = parse(JSON.stringify({
    v: VERSION, root: "/proj", active: 0, layout: DEFAULT_LAYOUT,
    tabs: [{ path: "/a", pinned: true }, { path: "/b", pinned: "true" }, { path: "/c" }],
  }));
  ok(pinned?.tabs[0].pinned === true && pinned?.tabs[1].pinned === undefined && pinned?.tabs[2].pinned === undefined, "pinned 只认 true");
  ok(parse(serialize(pinned!))?.tabs[0].pinned === true, "钉住要经得起一来一回");
}

// ── withoutTabs：项目那份快照里不能有草稿（issue #40） ──
{
  const S = "/scratches";
  const isScratch = (p: string) => p.startsWith(`${S}/`);
  const mixed: Session = {
    ...base,
    tabs: [{ path: "/proj/a.ts" }, { path: `${S}/1.md`, draft: "x" }, { path: "/proj/b.md" }, { path: `${S}/2.md` }],
    active: 2,
  };
  const r = withoutTabs(mixed, isScratch);
  ok(r.tabs.length === 2, "两份草稿要滤掉");
  ok(r.tabs.every((t) => !isScratch(t.path)), "剩下的全是项目文件");
  ok(r.active === 1, "活动标签下标要跟着重算（原来指 b.md，滤完它在第 1 位）");
  ok(mixed.tabs.length === 4, "不改原对象");

  const activeIsScratch = withoutTabs({ ...mixed, active: 1 }, isScratch);
  ok(activeIsScratch.active === 0, "活动标签本身是草稿（左边只有 a.ts）时退到 a.ts");
  const activeIsLastScratch = withoutTabs({ ...mixed, active: 3 }, isScratch);
  ok(activeIsLastScratch.active === 1, "活动的是最后那份草稿时退到它左边最近的 b.md，不是 0");

  const none = withoutTabs({ ...base, tabs: [], active: 0 }, isScratch);
  ok(none.tabs.length === 0 && none.active === 0, "空的照样空");

  // 老快照的 sideView 认不出就回 files；新值 scratch 要认
  ok(toLayout({ sideView: "scratch" }).sideView === "scratch", "sideView 认 scratch");
  ok(toLayout({ sideView: "bogus" }).sideView === "files", "sideView 认不出回 files");
}

// ── 视口：列 / 顶行 / 行内偏移（2026-09-17）──
{
  const s = parse(serialize({ ...base, tabs: [{ path: "/p/a", line: 300, col: 8, top: 280, toff: 12 }, { path: "/p/b", line: 5 }] }));
  const t = s?.tabs[0];
  ok(t?.line === 300 && t?.col === 8 && t?.top === 280 && t?.toff === 12, "四个字段原样读回");
  const u = s?.tabs[1];
  ok(u?.line === 5 && u?.col === undefined && u?.top === undefined && u?.toff === undefined, "只记了行的不该凭空多出列和顶行");
  const bad = parse(JSON.stringify({
    v: VERSION, root: "/p", active: 0, layout: DEFAULT_LAYOUT,
    tabs: [
      { path: "/p/a", line: 3, col: 0, top: -1, toff: 5000 }, // 三个都坏
      { path: "/p/b", line: 3, col: 2.9, top: 7, toff: 3.4 }, // 小数
      { path: "/p/c", line: 3, col: "8", top: null, toff: NaN }, // 类型不对
    ],
  }));
  const a = bad?.tabs[0], b = bad?.tabs[1], c = bad?.tabs[2];
  ok(a?.line === 3 && a?.col === undefined && a?.top === undefined && a?.toff === undefined, "列 0 / 顶行 -1 / 偏移 5000 各自丢掉，行号不连坐");
  ok(b?.col === 2 && b?.top === 7 && b?.toff === 3, "小数列取整、偏移四舍五入");
  ok(c?.line === 3 && c?.col === undefined && c?.top === undefined && c?.toff === undefined, "字符串 / null / NaN 一律当没有");
}

console.log(`${fail === 0 ? "✅" : "❌"} 会话快照：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
