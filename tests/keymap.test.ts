import { KEYS, byId, toTauriAccel, accelOrderIsApple } from "../src/lib/state/keymap.ts";

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

  const accels = KEYS.filter((k) => k.accel).map((k) => k.accel!);
  const dup = accels.filter((a, i) => accels.indexOf(a) !== i);
  ok(dup.length === 0, `同一个键位不许挂两条：${dup.join(" ")}`);
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
  if (k.owner === "cm6") {
    // ⌘F 进了菜单就等于把 CM6 的查找抢没了，而且是静默的
    ok(k.accel !== undefined, `${k.id} 标了 cm6 却没写键位，那它凭什么占着这个位置`);
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
  ok(!!t && t.includes("+"), `${k.id} 的 ${k.accel} 推不出 Tauri accelerator（实得 ${t}）`);
}

// ── byId ──────────────────────────────────────────────────────────

ok(byId("save")?.accel === "⌘S", "byId 拿得到");
ok(byId("并不存在的") === undefined, "找不到给 undefined，不抛");

// ── 设计里点名的那几条，锁死 ──────────────────────────────────────

ok(byId("cm-find")?.owner === "cm6", "⌘F 必须归 CM6 —— 进菜单等于把编辑器的查找抢没了");
ok(byId("quick-file")?.owner === "key", "⌘P 故意留在 keydown：进菜单会在终端里被抢走");
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

console.log(fail === 0 ? `✅ 键位表：${pass} 通过，0 失败` : `❌ 键位表：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
