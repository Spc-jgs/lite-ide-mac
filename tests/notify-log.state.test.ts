/**
 * 通知记录（docs/TAKEOVER.md I11）：胶囊说过的、卡片说过的都进记录；**被没读完的失败压下去、没露面的那条也要在**（那正是最需要补看的）；
 * 角标只数坏消息；记录有上限、超了丢最老的。
 */
const { notify } = await import("../src/lib/state/notify.svelte");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

notify.fail("推送失败：远程比你多了东西");
notify.ok("已保存");
ok(notify.toast?.text === "推送失败：远程比你多了东西", "没读完的失败不被「已保存」顶掉（胶囊照旧）");
ok(notify.log.map((x) => x.text).join("|") === "推送失败：远程比你多了东西|已保存", `被压下去的「已保存」也进了记录：${notify.log.map((x) => x.text).join("|")}`);
ok(notify.unseen === 1, `角标只数坏消息：${notify.unseen}`);

notify.hint("剪贴板里没有文字");
notify.block("切不过去", "你有未提交的改动：\n  a.txt");
const last = notify.log[notify.log.length - 1];
ok(last.kind === "block" && last.text === "切不过去\n你有未提交的改动：\n  a.txt", `卡片记成标题 + 说明：${JSON.stringify(last)}`);
ok(notify.unseen === 2, `卡片也算坏消息、提示不算：${notify.unseen}`);
ok(notify.log.every((x, i, a) => i === 0 || x.id > a[i - 1].id), "按先后排、号不重");

notify.unseen = 0;
for (let i = 0; i < 400; i++) notify.hint(`第 ${i} 条`);
ok(notify.log.length === 300, `记录有上限：${notify.log.length}`);
ok(notify.log[notify.log.length - 1].text === "第 399 条" && notify.log[0].text === "第 100 条", "超了丢最老的，最新的一定在");

console.log(fail === 0 ? `✅ 通知记录：${pass} 通过，0 失败` : `❌ 通知记录：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
