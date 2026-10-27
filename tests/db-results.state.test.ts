/**
 * 数据库查询结果的状态层（src/lib/db/results.svelte.ts，docs/DATABASE.md DB-2），IPC 走桩（mock/db.ts 的假 MySQL）。
 * 每一条都是一种「看着对、其实错」的形状：钉住的被换掉、被顶掉的游标还去取、同一个连接上同时跑两条、一串里错了一条还往下跑、
 * 项目里的 .sql 被当成控制台
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { results } = await import("../src/lib/db/results.svelte");
const { layout } = await import("../src/lib/state/layout.svelte");
const { project } = await import("../src/lib/state/project.svelte");
const { notify } = await import("../src/lib/state/notify.svelte");
const { scratchDir } = await import("../src/lib/ipc/commands");
project.scratchRoot = await scratchDir();

let pass = 0, fail = 0;
const ok = (c: unknown, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const ROOT = "/Users/you/proj";
const run = (...sqls: string[]) => results.run(ROOT, "c1", "订单库", sqls);

// 第一条：结果出来、工具窗打开
await run("select * from orders");
const r1 = results.active!;
ok(r1.status === "done" && r1.rows.length === 500 && r1.more, `第一批 500 行、还有：${r1.status} ${r1.rows.length} ${r1.more}`);
ok(layout.panel && layout.panelView === "db", "工具窗开在「数据库」上");
await results.fetchMore(r1);
ok(r1.rows.length === 1000, `取下一批：${r1.rows.length}`);

// 钉住的留着、但游标被新查询关了；没钉住的被换掉
r1.pinned = true;
await run("select * from orders where id > 0");
const r2 = results.active!;
ok(results.list.includes(r1) && results.list.includes(r2), "钉住的那个留着");
ok(r1.stale, "钉住的那个被同一连接上的新查询顶掉了游标");
const before = r1.rows.length;
await results.fetchMore(r1);
ok(r1.rows.length === before, "被顶掉的不再去取下一批（取的话桩会报「结果集已关闭」）");
await run("select * from orders where id > 1");
ok(!results.list.includes(r2) && results.list.includes(r1), "没钉住的旧结果被换掉、钉住的还在");

// 同一个连接上还有一条在跑：第二条不发
const slow = run("select sleep(30)");
await new Promise((r) => setTimeout(r, 20));
const s = results.active!;
ok(s.status === "running", `慢查询在跑：${s.status}`);
const n = results.list.length;
await run("select * from orders");
ok(results.list.length === n && results.active === s, "跑着时再执行：不新开、不换掉跑着的那个");
ok(notify.log.some((x) => x.text.includes("仍在执行")), "说了一句为什么没执行");
results.cancel(s);
await slow;
ok(s.status === "cancelled" && /interrupted/.test(s.error), `取消：${s.status} ${s.error.slice(0, 40)}`);

// 一串里错了一条：后面的不跑
await run("selec 1", "select * from orders");
const [bad, skipped] = results.list.slice(-2);
ok(bad.status === "failed" && skipped.status === "cancelled" && skipped.error.startsWith("前一条"), `前一条错了后面的不跑：${bad.status} / ${skipped.status} ${skipped.error}`);

// 更新语句：影响的行数
await run("update orders set amount = 0 where id < 4");
ok(results.active!.kind === "count" && results.active!.count === 3, `更新回影响的行数：${results.active!.kind} ${results.active!.count}`);

// 执行前确认（DB-3）：答「取消」就一条都不跑；不是生产库只拦不带 WHERE 的
const asked: { prod: boolean; sqls: string[] }[] = [];
let answer = false;
results.confirm = async (q) => (asked.push({ prod: q.prod, sqls: q.sqls }), answer);
let n0 = results.list.length;
await run("delete from orders where id = 1");
ok(asked.length === 0 && results.list.length >= n0, "不是生产库、带 WHERE：不问、照常执行");
const atStart = results.list.map((r) => r.id).join();
await run("select * from orders", "delete from orders");
ok(asked.length === 1 && JSON.stringify(asked[0].sqls) === '["delete from orders"]', `不带 WHERE 的 DELETE：问一次、只列这一条：${JSON.stringify(asked)}`);
ok(results.list.map((r) => r.id).join() === atStart, "答「取消」：一条都不跑（同一批里的 SELECT 也不跑，不留半截）");
answer = true;
await results.run(ROOT, "c1", "订单库", ["update orders set amount = 0 where id < 4"], true);
ok(asked.length === 2 && asked[1].prod, "生产库：带 WHERE 的 UPDATE 也问");
ok(results.active!.status === "done" && results.active!.prod, "答「执行」：执行了，结果记着是生产库");
asked.length = 0;
results.rerun(results.active!);
await new Promise((r) => setTimeout(r, 300));
ok(asked.length === 1, "生产库的结果「重新运行」照样要过确认");
results.confirm = async () => true;

// 改格子（DB-3）：攒着、给人看、提交；不提交不动
await run("select * from orders");
const g = results.active!;
ok(g.table?.name === "orders", "单表的结果知道是哪张表");
results.stage(g, 0, 1, "9.99");
results.stage(g, 1, 1, "1.50");
results.stage(g, 1, 1, g.rows[1][1]);
ok(JSON.stringify(Object.keys(g.edits)) === JSON.stringify(["0\n1"]), `改回原值的不算改动：${JSON.stringify(g.edits)}`);
let shown: string[] = [];
let go = false;
results.previewApply = async (q) => ((shown = q.sqls), go);
await results.commit(g);
ok(shown.length === 1 && shown[0].includes("amount = '9.99'") && shown[0].includes("WHERE id = '1'"), `提交前给人看 UPDATE：${shown[0]}`);
ok(g.rows[0][1] === "1.50" && Object.keys(g.edits).length === 1, "答「取消」：表格里还是原值、改动还攒着");
go = true;
await results.commit(g);
ok(g.rows[0][1] === "9.99" && Object.keys(g.edits).length === 0, "提交成功：表格里换成新值、攒着的清空");
ok(g.stale, "提交关掉了这个连接的游标：这个结果取不了下一批了");
results.stage(g, 2, 0, "4");
g.rows = g.rows.map((r, i) => (i === 2 ? ["999", ...r.slice(1)] : r));
await results.commit(g);
ok(Object.keys(g.edits).length === 1 && notify.log.some((x) => x.text.startsWith("提交失败") && x.text.includes("全部更改已撤回")), "整笔回滚：改动留着、说清楚");

// 关掉最后一个：工具窗收起（不切到终端 —— 那会顺手起一个 shell）
for (const r of [...results.list]) results.close(r.id);
ok(results.list.length === 0 && !layout.panel && layout.panelView === "db", `全关了：工具窗收起、不切到别的工具窗（panel=${layout.panel} view=${layout.panelView}）`);

// 哪个文件是控制台：只认应用数据目录底下的
const base = project.scratchRoot!.slice(0, project.scratchRoot!.lastIndexOf("/"));
ok(project.dbConsole(`${base}/db/console/c1a2b/订单库.sql`) === "c1a2b", "应用数据目录里的控制台认得出是哪个连接");
ok(project.dbConsole(`${ROOT}/db/console/c1a2b/订单库.sql`) === null, "项目里碰巧同样层级的 .sql 不是控制台");
ok(project.dbConsole(`${base}/db/console/../x/a.sql`) === null && project.dbConsole(`${base}/db/console/c1/sub/a.sql`) === null, "连接 id 那一段只认 c + 十六进制、只有一层");
ok(project.autosaves(`${base}/db/console/c1a2b/订单库.sql`) && !project.isScratch(`${base}/db/console/c1a2b/订单库.sql`), "控制台自己存，但不是草稿（标签名、草稿本、空了删掉都不沾）");

console.log(`${fail ? "❌" : "✅"} 数据库查询结果：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
