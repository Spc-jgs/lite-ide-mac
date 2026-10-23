import { readdirSync, readFileSync } from "node:fs";
import { KEYS, byId, toTauriAccel, accelOrderIsApple, accelFromEvent, accelOf, conflicts, editable, setOverrides, shortcuts, splitAccel, toCmKey } from "../src/lib/state/keymap.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// ── 表本身的完整性 ────────────────────────────────────────────────

/*
 * 这几条卡的是「四处手抄」那个病根。表一旦自相矛盾，
 * 菜单栏、速查表、随处搜索三处会各自显示一个版本，而且都不报错。
 */
{
  const ids = KEYS.map((k) => k.id);
  ok(new Set(ids).size === ids.length, "id 不许重复 —— 菜单项 id 靠它路由");

  /*
   * 同一个键位不许挂两条 —— 例外是「控件自己吃键」的那一类（cm6 / xterm）：
   * 它们按焦点分派，⌘F 在编辑器里归 CM6、在终端里归 xterm，不会双触发。
   * 例外只到这儿：两条里只要有一条是 menu / key，就还是重复。
   */
  const widget = new Set(["cm6", "xterm", "log"]);
  const withAccel = KEYS.filter((k) => k.accel);
  const dup = withAccel
    .filter((k, i) => withAccel.findIndex((x) => x.accel === k.accel) !== i)
    .filter((k) => !withAccel.filter((x) => x.accel === k.accel).every((x) => widget.has(x.owner)))
    .map((k) => `${k.id}=${k.accel}`);
  ok(dup.length === 0, `同一个键位不许挂两条：${dup.join(" ")}`);
  ok(withAccel.filter((k) => k.accel === "⌘F").map((k) => k.owner).sort().join() === "cm6,xterm", "⌘F 恰好两条：编辑器一条、终端一条");
}

/*
 * Apple 的修饰键次序是 ⌃⌥⇧⌘。老的 actions 表里写的是 `⌘⇧G`，
 * 而正确的是 `⇧⌘G` —— 这类错误肉眼扫不出来，只能让机器盯着。
 */
for (const k of KEYS) {
  if (!k.accel) continue;
  ok(accelOrderIsApple(k.accel), `${k.id} 的 ${k.accel} 修饰键次序不对（应为 ⌃⌥⇧⌘）`);
}
ok(!accelOrderIsApple("⌘⇧G"), "⌘⇧G 必须被判为错 —— 否则这条测试等于没写");
ok(accelOrderIsApple("⇧⌘G"), "⇧⌘G 是对的");
ok(accelOrderIsApple("⌃⇧`"), "⌃⇧` 是对的");

// ── owner 是判据，不是注释 ────────────────────────────────────────

/*
 * 菜单项带上 accelerator 之后 AppKit 会先把键吃掉，webview 收不到。
 * 所以 `menu` 的键位在 keydown 里就不该再有分支，`key` 的反过来。
 * 这里能测的是数据层面的自洽：
 */
for (const k of KEYS) {
  if (k.owner === "cm6" || k.owner === "xterm" || k.owner === "log") {
    // ⌘F 进了菜单就等于把 CM6 的查找抢没了，而且是静默的
    ok(k.accel !== undefined, `${k.id} 标了 ${k.owner} 却没写键位，那它凭什么占着这个位置`);
  }
  if (k.gesture) {
    ok(k.accel === undefined, `${k.id} 既有手势又有 accel —— 只能有一个`);
    ok(k.owner === "key", `${k.id} 是手势，表达不成 accelerator，只能归 keydown`);
  }
}

// 别名只在速查表里出现，所以有别名的必须是进了菜单的主键位
for (const k of KEYS) {
  if (!k.alias) continue;
  ok(k.accel !== undefined, `${k.id} 有别名却没有主键位`);
  ok(k.owner === "menu", `${k.id} 有别名说明菜单只写得下一个，那主键位就该归菜单`);
}

// ── 显示键位 → Tauri accelerator：不存两份，只能推 ──────────────────

ok(toTauriAccel("⌘S") === "CmdOrCtrl+S", `⌘S，实得 ${toTauriAccel("⌘S")}`);
ok(toTauriAccel("⌘1") === "CmdOrCtrl+1", `⌘1，实得 ${toTauriAccel("⌘1")}`);
ok(toTauriAccel("⇧⌘F") === "Shift+CmdOrCtrl+F", `⇧⌘F，实得 ${toTauriAccel("⇧⌘F")}`);
ok(toTauriAccel("⌥⌘F") === "Alt+CmdOrCtrl+F", `⌥⌘F，实得 ${toTauriAccel("⌥⌘F")}`);
// muda 不认裸的反引号，得给它 Backquote
ok(toTauriAccel("⌃⇧`") === "Ctrl+Shift+Backquote", `⌃⇧\`，实得 ${toTauriAccel("⌃⇧`")}`);
ok(toTauriAccel("⌘/") === "CmdOrCtrl+/", `⌘/，实得 ${toTauriAccel("⌘/")}`);
ok(toTauriAccel("F9") === "F9" && toTauriAccel("⇧F8") === "Shift+F8", `功能键：F9 → ${toTauriAccel("F9")}，⇧F8 → ${toTauriAccel("⇧F8")}`);
ok(toTauriAccel(undefined) === undefined, "没有键位就是 undefined，不能给空串");
// 只有修饰键、没有主键：这种输入不该产出一个 muda 解析不了的串
ok(toTauriAccel("⌘") === undefined, "只有修饰键时返回 undefined");

/*
 * 反向自洽：每一个 owner === "menu" 且有 accel 的，都必须推得出
 * 一个非空的 Tauri 串 —— 推不出就是菜单上写着键位却按不动。
 */
for (const k of KEYS) {
  if (k.owner !== "menu" || !k.accel) continue;
  const t = toTauriAccel(k.accel);
  // 裸的功能键也算（调试的 F7 / F8 / F9，同 IDEA）：muda 认 `F9` 这种不带修饰键的写法；别的裸键（一个字母）进菜单就是把打字吃掉
  ok(!!t && (t.includes("+") || /^F\d{1,2}$/.test(t)), `${k.id} 的 ${k.accel} 推不出 Tauri accelerator（实得 ${t}）`);
}

// ── byId ──────────────────────────────────────────────────────────

ok(byId("save")?.accel === "⌘S", "byId 拿得到");
ok(byId("并不存在的") === undefined, "找不到给 undefined，不抛");

// ── 设计里点名的那几条，锁死 ──────────────────────────────────────

ok(byId("cm-find")?.owner === "cm6", "⌘F 必须归 CM6 —— 进菜单等于把编辑器的查找抢没了");
ok(byId("term-find")?.owner === "xterm", "终端的 ⌘F 归 xterm —— 同一个理由");
ok(byId("quick-file")?.accel === "⇧⌘O" && byId("param-info")?.accel === "⌘P" && byId("param-info")?.owner === "cm6", "照 IDEA：找文件 ⇧⌘O、参数提示 ⌘P（归编辑器，终端里的 ⌘P 照样给 shell）");
ok(byId("quick-all")?.gesture === "连按两下 ⇧", "随处搜索是手势，菜单里只能写进标签");
/*
 * **⌘B 归跳转，不再是侧边栏的别名**（2026-09-09 换的）。
 *
 * 这条原来反着写（`toggle-sidebar` 的 alias 是 ⌘B）。换的判据是
 * 「谁没有主键位」：侧边栏有 ⌘1，而 IDEA 里 ⌘B 就是跳到声明的主键位。
 * 两条一起锁，任何一边悄悄改回去都会红。
 */
ok(byId("toggle-sidebar")?.alias === undefined, "侧边栏交出了 ⌘B，只留 ⌘1");
ok(byId("jump-decl")?.accel === "⌘B", "⌘B 归跳到声明");
ok(
  byId("jump-decl")?.owner === "cm6",
  "跳转必须归 CM6：它要读编辑器此刻的语法树和光标，而菜单 accelerator 不看焦点",
);
/*
 * 多光标**没有被跳转挤掉**：⌘Click 只在有下划线的词上跳，别处照旧加光标。
 * 一度想挪到 ⌥Click，实测那条路被 `rectangularSelection()` 接走了
 * （它默认就吃 altKey），锁住这条免得有人再挪一次。
 */
ok(byId("cm-multi-cursor")?.accel === "⌘Click", "多光标还在 ⌘Click 上");

// ── 你改过的键位（docs/TAKEOVER.md R3） ─────────────────────────────

ok(toCmKey("⌥⌘L") === "Alt-Mod-l" && toCmKey("⌃⇧R") === "Ctrl-Shift-r" && toCmKey("⌘⌫") === "Mod-Backspace", `换成 CM6 的写法：${toCmKey("⌥⌘L")} ${toCmKey("⌃⇧R")} ${toCmKey("⌘⌫")}`);
ok(toCmKey("⌥⇧↑") === "Alt-Shift-ArrowUp" && toCmKey("F1") === "F1" && toCmKey("⌃↵") === "Ctrl-Enter" && toCmKey("⇧F6") === "Shift-F6", "方向键、功能键、回车");
ok(toCmKey("⌘⇧G") === null && toCmKey("L") === null && toCmKey("⌘Click") === null && toCmKey(undefined) === null, "次序错的、裸字母、鼠标手势、没键位：都不挂");
ok(toTauriAccel("⌃↵") === "Ctrl+Enter" && toTauriAccel("⌘⌫") === "CmdOrCtrl+Backspace", "菜单那边也认回车退格（和 keymap.rs 同一张表）");
ok(splitAccel("F9") !== null && splitAccel("F21") === null && splitAccel("⌘é") === null, "功能键 F1–F20；不认识的字不收");

// 录键按 e.code：⌥ 在 macOS 上把 e.key 变成别的字（⌥L = ¬），按 key 认就录成 ⌥⌘¬
const ev = (code: string, m: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...m });
ok(accelFromEvent(ev("KeyL", { altKey: true, metaKey: true })) === "⌥⌘L", `⌥⌘L：${accelFromEvent(ev("KeyL", { altKey: true, metaKey: true }))}`);
ok(accelFromEvent(ev("Slash", { metaKey: true, shiftKey: true })) === "⇧⌘/", "修饰键按 ⌃⌥⇧⌘ 排，不按按下的先后");
ok(accelFromEvent(ev("ArrowUp", { altKey: true })) === "⌥↑" && accelFromEvent(ev("Enter", { ctrlKey: true })) === "⌃↵", "方向键、回车");
ok(accelFromEvent(ev("MetaLeft", { metaKey: true })) === null, "只按了修饰键：还在等主键");

// 生效的键位：改过用改的，"" = 不要了
const save = KEYS.find((k) => k.id === "save")!;
ok(accelOf(save, { save: "⌥⌘S" }) === "⌥⌘S" && accelOf(save, { save: "" }) === undefined && accelOf(save, {}) === "⌘S", "改过用改的、空串是不要、没改用默认");
setOverrides({ save: "⌥⌘S", "git-fetch": "⌃⌘F9" });
ok(byId("save")?.accel === "⌥⌘S" && byId("git-fetch")?.accel === "⌃⌘F9", "byId 给的是生效的（菜单、随处搜索、右键菜单都从这儿印）");
ok(shortcuts().some((k) => k.id === "git-fetch"), "改出了键位的进速查表");
ok(KEYS.find((k) => k.id === "save")!.accel === "⌘S", "默认表本身没被改");
setOverrides({});
ok(byId("save")?.accel === "⌘S", "换回空的覆盖表 = 全部默认");

// 撞键：默认表一条都不撞；把「保存」改到 ⌘F 上撞编辑器的查找（菜单先吃键）；编辑器里两件事一个键也撞；别名也占着
ok(Object.keys(conflicts({})).length === 0, `默认表不撞：${JSON.stringify(conflicts({}))}`);
ok(conflicts({ save: "⌘F" })["⌘F"]?.includes("save") === true, "菜单改到 ⌘F：和编辑器的查找撞");
ok(conflicts({ "cm-duplicate": "⌘G" })["⌘G"]?.length === 2, "编辑器里两件事一个键：撞");
ok(conflicts({ "git-fetch": "⌘[" })["⌘["]?.includes("nav-back") === true, "别名（⌘[ 是回退的别名）也占着键");
ok(conflicts({ "cm-find": "⌘G", "cm-find-next": "⌘F" })["⌘F"] === undefined, "编辑器和终端各吃各的 ⌘F：不算撞");

// 能改的：菜单的、编辑器的；归 keydown 的、终端的、手势、鼠标手势不能
ok(editable(byId("save")!) && editable(byId("find-usages")!) && editable(byId("git-fetch")!), "菜单、编辑器、没默认键的菜单项都能改");
ok(!editable(byId("run-last")!) && !editable(byId("term-find")!) && !editable(byId("quick-all")!) && !editable(byId("cm-multi-cursor")!), "keydown、终端、⇧⇧、⌘Click 不能改");

/*
 * 编辑器里每一条能改的，都真的按 id 挂上了（`id: "…"` 出现在 src/lib/editor 里）；反过来，编辑器里不许再写死
 * 一条能改的默认键（`key: "Alt-F7"`）—— 写死的那条你在设置里改了也没用，而且它不报错，只表现为「改了没生效」
 */
{
  const dir = "src/lib/editor";
  const src = readdirSync(dir).filter((f) => /\.(ts|svelte)$/.test(f)).map((f) => readFileSync(`${dir}/${f}`, "utf8")).join("\n");
  for (const k of KEYS.filter((k) => k.owner === "cm6" && editable(k))) {
    ok(src.includes(`id: "${k.id}"`), `${k.id}（${k.accel}）在 keymap.ts 里归编辑器，src/lib/editor 里却没有按 id 挂它`);
    const cm = toCmKey(k.accel);
    if (cm) ok(!src.includes(`key: "${cm}"`), `${k.id} 的默认键 ${cm} 在 src/lib/editor 里写死了 —— 改成 idKeymap({ id: "${k.id}" })`);
  }
}

/*
 * 工具窗的编号键 / 菜单项（`tw-*`）在菜单动作里是一张写明的转交表（`menu-actions.ts` 的那个 case）：
 * 新加一个工具窗、表里漏了，菜单点了什么都不发生、不报错 —— TODO 工具窗（G18）第一版就是这样，smoke 走真菜单才撞见
 */
{
  const src = readFileSync("src/lib/shell/menu-actions.ts", "utf8");
  for (const k of KEYS.filter((k) => k.id.startsWith("tw-"))) {
    ok(src.includes(`case "${k.id}":`), `${k.id} 在 keymap.ts 里有，menu-actions.ts 的转交表里没有它 —— 菜单点了没反应`);
  }
}

console.log(fail === 0 ? `✅ 键位表：${pass} 通过，0 失败` : `❌ 键位表：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
