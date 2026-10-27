/**
 * SQL 控制台按 `;` 和空行切语句（src/lib/db/sql-split.ts）。
 * 切错的后果不报错、只是发出去的不是你写的那条：字符串里的分号把一条 INSERT 切成两半，`DELETE … WHERE` 在 WHERE 前面被切开
 */
import { splitSql, statementAt, statementsIn } from "../src/lib/db/sql-split.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);
const sqls = (t: string) => splitSql(t).map((s) => s.sql);

eq(sqls("select 1; select 2;"), ["select 1", "select 2"], "分号切开、两头空白去掉");
eq(sqls("select 1\nfrom t\n\nselect 2"), ["select 1\nfrom t", "select 2"], "没写分号：空行分开，同一条里的换行不切");
eq(sqls("select 1\n  \t\nselect 2"), ["select 1", "select 2"], "只有空白的一行也算空行");
eq(sqls("insert into t values ('a;b', 'it''s\n\nfine');"), ["insert into t values ('a;b', 'it''s\n\nfine')"], "字符串里的分号和空行不切");
// '' 连写不单独测：把它当成「结束 + 又开始一个」切出来的边界一模一样（试过把那段删掉，这里照样绿），它只关系到位置换算的语义
eq(sqls("select 'a\\'; b' from t; select 2"), ["select 'a\\'; b' from t", "select 2"], "反斜杠转义的引号不结束字符串（MySQL）");
eq(sqls('select "a;b", `c;d` from t;'), ['select "a;b", `c;d` from t'], "带引号的名字里的分号不切");
eq(sqls("-- 先看看; 再删\nselect 1;\n/* 多行\n\n注释; */ select 2"), ["-- 先看看; 再删\nselect 1", "/* 多行\n\n注释; */ select 2"], "注释里的分号和空行不切");
eq(sqls("-- 只有注释\n;\n  ;  "), [], "只有注释、空白的段不算一条");
const fn = "create function f() returns int as $$\nbegin\n  return 1;\n\nend;\n$$ language plpgsql;\nselect f();";
eq(sqls(fn), ["create function f() returns int as $$\nbegin\n  return 1;\n\nend;\n$$ language plpgsql", "select f()"], "PostgreSQL 的 $$ 函数体里面的分号和空行不切");
eq(sqls("do $body$ begin perform 1; end $body$; select 2"), ["do $body$ begin perform 1; end $body$", "select 2"], "带标签的 $标签$ 同样");
eq(sqls("delete from t\nwhere id = 1;"), ["delete from t\nwhere id = 1"], "WHERE 在下一行：不切（没空行）");

const T = "select 1;\nselect 2;\n\nupdate t set a = 1\nwhere id = 3;";
const at = (pos: number) => statementAt(T, pos)?.sql ?? null;
eq(at(0), "select 1", "光标在第一条开头");
eq(at(T.indexOf(";")), "select 1", "光标正好在分号前");
eq(at(T.indexOf(";") + 1), "select 1", "光标在分号后（同一行）：还是这一条");
eq(at(T.indexOf("where") + 3), "update t set a = 1\nwhere id = 3", "光标在多行语句的第二行");
eq(at(T.indexOf("\n\n") + 1), "select 2", "光标在空行上：前面一条");
eq(statementAt("   \n  ", 2), null, "没有语句：null");
const S = statementAt(T, T.indexOf("update"));
ok(!!S && T.slice(S.from, S.to) === S.sql, "起止对得上全文里那一段（高亮按它）");

const sel = statementsIn(T, T.indexOf("select 2"), T.length);
eq(sel.map((s) => s.sql), ["select 2", "update t set a = 1\nwhere id = 3"], "选中一段：里面的每一条");
ok(sel.every((s) => T.slice(s.from, s.to) === s.sql), "选中里切出来的位置换算回全文");

console.log(`${fail ? "❌" : "✅"} SQL 切语句：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
