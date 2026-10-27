/**
 * 提示的胶囊（`state/notify.svelte.ts` + `shell/Toast.svelte`，2026-10-13 盘点 B1 / B2）：一次一条，
 * **没做成的不被做成的顶掉**，「什么都没发生」是单独一档，停多久按字数算，鼠标停上去不走。
 */
const { notify, readMs } = await import("../src/lib/state/notify.svelte");

let pass = 0, fail = 0;
const ok = (c: unknown, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// 停多久按字数：短的起步 1.6 秒，长的多给，封顶 6 秒；失败至少 4 秒
ok(readMs("已推送") === 1600, `四个字以内：起步 1.6 秒，实际 ${readMs("已推送")}`);
ok(readMs("已".repeat(30)) === 2800, `三十个字：1000 + 30×60，实际 ${readMs("已".repeat(30))}`);
ok(readMs("已".repeat(200)) === 6000, "再长也封顶 6 秒");
ok(readMs("推送失败", 4000) === 4000, "失败起步 4 秒");

// 三档各自的 kind
notify.ok("已保存 a.txt");
ok(notify.toast?.kind === "ok" && notify.toast.text === "已保存 a.txt", "做成了：ok");
notify.hint("剪贴板里没有文字");
ok(notify.toast?.kind === "hint", "什么都没发生：hint，不是 ok（原来画成了做成了的颜色）");

// 没做成的那条还没读完：做成了 / 提示不顶掉它；新的失败照样顶掉旧的
notify.fail("推送失败：远程拒绝");
notify.ok("已保存 a.txt");
ok(notify.toast?.text === "推送失败：远程拒绝", `失败后紧跟一条「已保存」：人要读的是前一条，实际 ${notify.toast?.text}`);
notify.hint("把光标放到一个名字上再按");
ok(notify.toast?.kind === "fail", "提示也不顶掉没读完的失败");
notify.fail("拉取失败：网络断了");
ok(notify.toast?.text === "拉取失败：网络断了", "新的失败顶掉旧的失败（那是更新的坏消息）");

// 同样的字再来一次算新的一条（胶囊要重新进场、重新计时）
notify.dismiss();
notify.ok("已复制");
const id1 = notify.toast?.id;
notify.ok("已复制");
ok(notify.toast?.id !== id1, "同样的字再来一次：新的号");

// 到点自己走；鼠标停在上面不走，离开之后再给剩下的时间
notify.ok("x", 60);
await sleep(100);
ok(notify.toast === null, "到点自己走");
notify.ok("y", 80);
notify.hold();
await sleep(150);
ok(notify.toast?.text === "y", "鼠标停在上面：过了时间也不走（人在读）");
notify.release();
ok(notify.toast?.text === "y", "离开那一刻还在");
await sleep(1100);
ok(notify.toast === null, "离开之后至少再给 1 秒，然后走");

// 点一下就关；关掉之后失败的「护着」也解除了
notify.fail("推送失败");
notify.dismiss();
ok(notify.toast === null, "点掉");
notify.ok("已保存");
ok(notify.toast?.text === "已保存", "失败被点掉之后，新的做成了照常显示");

// 附一个动作（「撤销」）：带着它，而且至少停 10 秒 —— 1.6 秒来不及把手挪过去点（2026-10-21 撤销卡片改成胶囊）
let ran = 0;
notify.ok("已替换 2 个文件", undefined, { label: "撤销", run: () => ran++ });
ok(notify.toast?.act?.label === "撤销", "动作跟着这一条");
notify.toast?.act?.run();
ok(ran === 1, "点它就做");
notify.dismiss();
notify.ok("短", undefined, { label: "撤销", run: () => {} });
await sleep(readMs("短") + 300);
ok(notify.toast?.text === "短", "带动作的不按字数走（1.6 秒后还在）");
notify.dismiss();
notify.fail("撤销没做完", 8000, { label: "撤销", run: () => {} });
ok(notify.toast?.kind === "fail" && notify.toast.act?.label === "撤销", "没做成的也能带动作");
notify.dismiss();
notify.ok("已保存");
ok(notify.toast?.act === undefined, "不带的就没有：上一条的动作不残留");
notify.fail("重命名失败：Customer.java 已存在，未覆盖");
notify.ok("已把 Client 改名为 Buyer：2 个文件 3 处", undefined, { label: "撤销", run: () => {} });
ok(notify.toast?.act?.label === "撤销", "失败没读完时紧跟一条带「撤销」的：它顶掉失败 —— 那是人刚做的那一下，按钮不能被吞");

notify.dismiss();
console.log(`${fail === 0 ? "✅" : "❌"} 提示胶囊（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
