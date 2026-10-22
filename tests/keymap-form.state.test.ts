/**
 * 设置里「键位」那一页的纯逻辑（src/lib/settings/keymap-form.ts，docs/TAKEOVER.md R3）：
 * 改的怎么合进 keymap.json、哪些行不能改、录到的键收不收、怎么搜。
 */
import { KEYS } from "../src/lib/state/keymap.ts";
import { judge, lockedWhy, matches, staged } from "../src/lib/settings/keymap-form.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);
const def = (id: string) => KEYS.find((k) => k.id === id)!;

// 合进覆盖表：改的叠上去；改回默认 = 删掉那条（keymap.json 只存改过的）；null = 回默认；"" = 不要键位
eq(staged({}, { save: "⌥⌘S" }), { save: "⌥⌘S" }, "改一条");
eq(staged({ save: "⌥⌘S" }, { save: "⌘S" }), {}, "改回和默认一样：那条删掉，不存「⌘S = ⌘S」");
eq(staged({ save: "⌥⌘S", "git-fetch": "⌃⌘F9" }, { save: null }), { "git-fetch": "⌃⌘F9" }, "null = 回默认，别的不动");
eq(staged({}, { "git-push": "" }), { "git-push": "" }, "\"\" = 不要键位，要存下来（和默认不一样）");
eq(staged({}, { "git-fetch": "" }), {}, "本来就没键位的改成「不要」：和默认一样，不存");
eq(staged({}, { "没有这个": "⌘9" }), {}, "不认识的 id 不进文件");

// 不能改的说清为什么
ok(lockedWhy(def("save")) === null && lockedWhy(def("find-usages")) === null, "菜单、编辑器的能改");
ok(lockedWhy(def("run-last"))?.includes("shell") === true, `keydown 的：${lockedWhy(def("run-last"))}`);
ok(lockedWhy(def("quick-all"))?.includes("手势") === true && lockedWhy(def("term-find"))?.includes("终端") === true, "手势、终端");
ok(lockedWhy(def("cm-multi-cursor"))?.includes("鼠标") === true, "⌘Click");

// 录到的键
eq(judge("⌥⌘L"), { accel: "⌥⌘L" }, "正常的收");
eq(judge("F5"), { accel: "F5" }, "功能键不带修饰也收");
ok("why" in judge("L") && "why" in judge("⇧L"), "裸字母、只带 ⇧ 的字母不收（打字用的）");
ok("why" in judge("⌘C") && (judge("⌘C") as { why: string }).why.includes("复制"), "⌘C 是系统的复制");

// 搜
ok(matches(def("save"), {}, "保存", null) && matches(def("save"), {}, "⌘s", null) && matches(def("save"), {}, "save", null), "名字、键位（不分大小写）、id");
ok(!matches(def("save"), { save: "⌥⌘S" }, "", "⌘S") && matches(def("save"), { save: "⌥⌘S" }, "", "⌥⌘S"), "按键找：按生效的键位");
ok(matches(def("nav-back"), {}, "", "⌘["), "按键找也认别名");

console.log(fail === 0 ? `✅ 键位页：${pass} 通过，0 失败` : `❌ 键位页：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
