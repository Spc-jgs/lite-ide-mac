/**
 * 设置对话框的纯逻辑（`settings/settings-form.ts`，docs/SETTINGS.md 第 13 节）：交给 Rust 的那一批只放真变了的 ——
 * 点开看一眼、改回原样、对没写的键点恢复默认，settings.json 都不该被改写
 */
import { catOf, changes, isSet, matches, parseIntField, shown } from "../src/lib/settings/settings-form.ts";
import type { SettingDef, SettingsPage } from "../src/lib/ipc/commands.ts";

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

const D = (key: string, kind: SettingDef["kind"], def: string, min: number | null = null, max: number | null = null): SettingDef => ({
  key,
  label: key,
  kind,
  default: def,
  doc: `说明 ${key}`,
  min,
  max,
});
const size = D("editor.fontSize", "integer", "13", 9, 28);
const home = D("maven.home", "string", '""');
const wrap = D("maven.useWrapper", "boolean", "true");
const page: SettingsPage = {
  defs: [size, home, wrap],
  values: { "editor.fontSize": 14, "maven.home": "", "maven.useWrapper": true },
  written: ["editor.fontSize"],
  path: "/x/settings.json",
  broken: null,
  jdks: [],
  badJdks: [],
  project: null,
};

ok(catOf("editor.fontSize") === "appearance" && catOf("maven.opts") === "maven" && catOf("java.jdks") === "java" && catOf("terminal.shell") === "terminal", "按前缀分类");
ok(matches(home, "MAVEN") && matches(home, "说明") && !matches(home, "字号") && matches(home, " "), "搜索：键名不分大小写、说明里的字也算、空的都算");

// 显示什么
ok(shown(page, {}, size) === 14, "没改：生效的");
ok(shown(page, { "editor.fontSize": 16 }, size) === 16, "改了没应用：改的");
ok(shown(page, { "editor.fontSize": null }, size) === 13, "点了恢复默认：默认值");
ok(isSet(page, {}, "editor.fontSize") && !isSet(page, {}, "maven.home"), "改过的标记：文件里写了的");
ok(!isSet(page, { "editor.fontSize": null }, "editor.fontSize") && isSet(page, { "maven.home": "/m" }, "maven.home"), "这次恢复默认的不算、这次改了的算");

// 交给 Rust 的那一批
ok(eq(changes(page, {}), {}), "什么都没改：空的");
ok(eq(changes(page, { "editor.fontSize": 14 }), {}), "改回原样：不算");
ok(eq(changes(page, { "maven.useWrapper": true }), {}), "填了一遍默认值（文件里本来没写）：不写");
ok(eq(changes(page, { "maven.home": null }), {}), "对没写的键点恢复默认：不算");
ok(eq(changes(page, { "editor.fontSize": null }), { "editor.fontSize": null }), "写了的恢复默认：交 null（Rust 删掉那一行）");
ok(eq(changes(page, { "editor.fontSize": 16, "maven.home": "/opt/maven" }), { "editor.fontSize": 16, "maven.home": "/opt/maven" }), "真改了的都交");

// 数字
ok(eq(parseIntField(size, " 15 "), { value: 15 }), "整数");
ok("error" in parseIntField(size, "15.5") && "error" in parseIntField(size, "") && "error" in parseIntField(size, "abc"), "不是整数：错误");
ok(eq(parseIntField(size, "40"), { error: "最大 28" }) && eq(parseIntField(size, "3"), { error: "最小 9" }), "越界：说范围，不替人夹");

console.log(`${fail === 0 ? "✅" : "❌"} 设置对话框的纯逻辑：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
