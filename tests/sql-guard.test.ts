/**
 * 执行之前看一眼「这条要动什么」（src/lib/db/sql-guard.ts）。错的方向有两个，都要卡：
 * 该问的没问（子查询里的 WHERE 被当成整条的、注释里的 where 骗过去）；数出来的行数和会改的对不上（多表、LIMIT 还硬数）
 */
import { countSqlFor, hasWhere, needsConfirm, target, writeKind } from "../src/lib/db/sql-guard.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

eq(writeKind("select * from t"), null, "SELECT 只读");
eq(writeKind("  -- 先看看\n/* x */ SELECT 1"), null, "开头的注释跳过");
eq(writeKind("with a as (select 1) select * from a"), null, "WITH … SELECT 只读");
eq(writeKind("with d as (delete from t where id = 1 returning *) select * from d"), "delete", "WITH 里藏着 DELETE：算写");
eq(writeKind("UPDATE t SET a = 1"), "update", "UPDATE");
eq(writeKind("delete from t"), "delete", "DELETE");
eq(writeKind("insert into t values (1)"), "insert", "INSERT");
eq(writeKind("drop table t"), "ddl", "DROP");
eq(writeKind("truncate table t"), "ddl", "TRUNCATE");
eq(writeKind("call purge_all()"), "write", "不认识的（CALL）按会改数据算");

ok(!hasWhere("delete from t"), "没有 WHERE");
ok(hasWhere("delete from t where id = 1"), "有 WHERE");
ok(!hasWhere("update t set a = (select max(b) from u where u.id = 1)"), "子查询里的 WHERE 不算：这条改的是整张表");
ok(!hasWhere("delete from t -- where id = 1"), "注释里的 where 不算");
ok(!hasWhere("update t set note = 'where is it'"), "字符串里的 where 不算");

eq(target("UPDATE shop.orders SET amount = 0 WHERE id < 4;"), { table: "shop.orders", where: "id < 4" }, "单表 UPDATE：表名、条件（去掉分号）");
eq(target("delete from \"Order\" where status = 'x'"), { table: "\"Order\"", where: "status = 'x'" }, "带引号的表名照原样");
eq(target("delete from t"), { table: "t", where: null }, "没有条件");
eq(target("update t o set a = 1 where o.id = 2"), { table: "t o", where: "o.id = 2" }, "带别名：条件里用得到别名");
eq(target("delete from t where id = 1 returning id"), { table: "t", where: "id = 1" }, "PostgreSQL 的 RETURNING 不进条件");
eq(target("update a, b set a.x = b.x where a.id = b.id"), null, "多表 UPDATE：认不准");
eq(target("update a join b on a.id = b.id set a.x = 1"), null, "UPDATE … JOIN：认不准");
eq(target("delete from t where x = 1 order by id limit 10"), null, "带 LIMIT 的 DELETE：按条件数出来的不是会删的，认不准");
eq(target("delete t1 from t1 join t2 on t1.id = t2.id"), null, "DELETE t1 FROM …：认不准");
eq(target("delete from t using u where t.id = u.id"), null, "DELETE … USING：认不准");
eq(target("update t set a = (select b from u where u.id = t.id)"), { table: "t", where: null }, "子查询里的 WHERE 不当成条件");

eq(countSqlFor("UPDATE shop.orders SET amount = 0 WHERE id < 4"), "SELECT COUNT(*) FROM shop.orders WHERE id < 4", "数行数的 SELECT");
eq(countSqlFor("delete from t"), "SELECT COUNT(*) FROM t", "没条件：数整张表");
eq(countSqlFor("insert into t values (1)"), null, "INSERT 不数");

ok(needsConfirm("delete from t", false), "不是生产库：不带 WHERE 的 DELETE 也要问");
ok(!needsConfirm("delete from t where id = 1", false), "不是生产库：带 WHERE 的不问");
ok(!needsConfirm("insert into t values (1)", false), "不是生产库：INSERT 不问");
ok(needsConfirm("insert into t values (1)", true), "生产库：INSERT 也问");
ok(needsConfirm("delete from t where id = 1", true), "生产库：带 WHERE 的也问（要说动几行）");
ok(!needsConfirm("select * from t", true), "生产库：只读的不问");

console.log(`${fail ? "❌" : "✅"} SQL 执行前检查：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
