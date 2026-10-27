/**
 * 执行之前先看一眼「这条要动什么」（docs/DATABASE.md 第 4 节，DB-3）：
 * - 生产库：改数据的语句一律先问；UPDATE / DELETE 先数会动几行（`countSqlFor`）
 * - 不是生产库：只拦不带 WHERE 的 UPDATE / DELETE（同 IDEA 默认开着的「执行可能不安全的语句前提示」）
 *
 * 只认最外层：子查询里的 WHERE 不算（`UPDATE t SET a = (SELECT … WHERE …)` 照样是整张表），字符串、注释里的词不算。
 * 数不准的不硬数（多表 UPDATE、带 LIMIT 的 DELETE、USING）：`countSqlFor` 回 null，对话框说「无法统计」，不给一个错的数。
 * 不碰编辑器、不碰 IPC：在裸 node 里测（tests/sql-guard.test.ts）
 */

export type WriteKind = "update" | "delete" | "insert" | "ddl" | "write";

interface Tok {
  /** 小写的词；标点是它自己 */
  t: string;
  /** 在原文里的起止 */
  from: number;
  to: number;
  /** 括号深度（最外层是 0） */
  depth: number;
}

/** 切成词：引号里的整段算一个词（不看里面），注释跳过 */
function lex(sql: string): Tok[] {
  const out: Tok[] = [];
  let depth = 0;
  let i = 0;
  const n = sql.length;
  while (i < n) {
    const c = sql[i];
    if (/\s/.test(c)) {
      i++;
    } else if (c === "-" && sql[i + 1] === "-") {
      const nl = sql.indexOf("\n", i);
      i = nl < 0 ? n : nl;
    } else if (c === "/" && sql[i + 1] === "*") {
      const e = sql.indexOf("*/", i + 2);
      i = e < 0 ? n : e + 2;
    } else if (c === "'" || c === '"' || c === "`") {
      let j = i + 1;
      while (j < n && !(sql[j] === c && sql[j + 1] !== c)) j += sql[j] === c || (c === "'" && sql[j] === "\\") ? 2 : 1;
      out.push({ t: c === "'" ? "'str'" : sql.slice(i, j + 1), from: i, to: Math.min(j + 1, n), depth });
      i = j + 1;
    } else if (/[A-Za-z0-9_$.\u0080-￿]/.test(c)) {
      let j = i;
      while (j < n && /[A-Za-z0-9_$.\u0080-￿"`]/.test(sql[j])) {
        // 带引号的那一段（shop."order"）连着算一个名字
        if (sql[j] === '"' || sql[j] === "`") {
          const e = sql.indexOf(sql[j], j + 1);
          j = e < 0 ? n : e + 1;
        } else j++;
      }
      out.push({ t: sql.slice(i, j).toLowerCase(), from: i, to: j, depth });
      i = j;
    } else {
      if (c === ")") depth = Math.max(0, depth - 1);
      out.push({ t: c, from: i, to: i + 1, depth });
      if (c === "(") depth++;
      i++;
    }
  }
  return out;
}

const READS = new Set(["select", "show", "explain", "describe", "desc", "values", "table", "help", "use", "set"]);
const DDL = new Set(["create", "drop", "alter", "truncate", "rename", "comment", "grant", "revoke"]);

/** 改不改数据、改的是哪一种；只读的是 null。`WITH … DELETE` 这种按里面那个写的动词算 */
export function writeKind(sql: string): WriteKind | null {
  const toks = lex(sql);
  const first = toks[0]?.t;
  if (!first || READS.has(first)) return null;
  if (first === "with") {
    const w = toks.find((x) => ["update", "delete", "insert", "merge"].includes(x.t));
    return w ? (w.t === "merge" ? "write" : (w.t as WriteKind)) : null;
  }
  if (first === "update" || first === "delete") return first;
  if (first === "insert" || first === "replace" || first === "merge" || first === "upsert") return first === "insert" ? "insert" : "write";
  if (DDL.has(first)) return "ddl";
  // call、exec、do、begin……不知道它做什么，按会改数据算（问一句的代价比悄悄执行小）
  return "write";
}

/** 最外层有没有 WHERE */
export function hasWhere(sql: string): boolean {
  return lex(sql).some((x) => x.depth === 0 && x.t === "where");
}

export interface Target {
  /** 原文里的表名（带 schema、引号照原样） */
  table: string;
  /** 最外层 WHERE 后面那一段（原文），没有是 null */
  where: string | null;
}

/**
 * 单表的 UPDATE / DELETE 改的是哪张表、条件是什么。认不准就是 null：
 * 多表（逗号、JOIN、USING、`DELETE t1 FROM …`）、带 ORDER BY / LIMIT（MySQL 的「只删前 N 行」，按条件数出来的不是会删的）、RETURNING 以外的尾巴
 */
export function target(sql: string): Target | null {
  const toks = lex(sql);
  const kind = toks[0]?.t;
  let i: number;
  if (kind === "update") {
    i = 1;
    while (toks[i] && ["low_priority", "ignore", "only"].includes(toks[i].t)) i++;
  } else if (kind === "delete") {
    if (toks[1]?.t !== "from") return null;
    i = 2;
    if (toks[i]?.t === "only") i++;
  } else return null;
  const tbl = toks[i];
  if (!tbl || /^[(,;']/.test(tbl.t)) return null;
  // 表名后面：可选的别名，然后 UPDATE 是 SET、DELETE 是 WHERE 或者结束
  let j = i + 1;
  if (toks[j] && toks[j].t === "as") j++;
  const after = toks[j]?.t;
  const alias = after && !["set", "where", ";", "returning"].includes(after) && /^[a-z_]/.test(after) ? after : null;
  if (alias) j++;
  if (kind === "update" && toks[j]?.t !== "set") return null;
  const top = toks.filter((x) => x.depth === 0);
  if (top.some((x) => ["join", "using", "limit", "order", "from"].includes(x.t) && x !== toks[1])) return null;
  if (kind === "update") {
    // SET 之前出现逗号就是多表 UPDATE（MySQL 的 `UPDATE a, b SET …`）
    const set = toks.indexOf(toks[j]);
    if (toks.slice(i, set).some((x) => x.t === ",")) return null;
  }
  const w = top.find((x) => x.t === "where");
  const ret = top.find((x) => x.t === "returning");
  const end = ret ? ret.from : sql.length;
  const table = sql.slice(tbl.from, tbl.to) + (alias ? ` ${sql.slice(toks[i + 1].t === "as" ? toks[i + 2].from : toks[i + 1].from, toks[j - 1].to)}` : "");
  return { table, where: w ? sql.slice(w.to, end).trim().replace(/;\s*$/, "") || null : null };
}

/** 数「这条 UPDATE / DELETE 会动几行」的那条 SELECT；认不准就是 null（对话框说无法统计，不给错的数） */
export function countSqlFor(sql: string): string | null {
  const t = target(sql);
  if (!t) return null;
  return `SELECT COUNT(*) FROM ${t.table}${t.where ? ` WHERE ${t.where}` : ""}`;
}

/** 这条要不要先问。生产库：改数据的都问；不是：只有不带 WHERE 的 UPDATE / DELETE */
export function needsConfirm(sql: string, prod: boolean): boolean {
  const k = writeKind(sql);
  if (!k) return false;
  if (prod) return true;
  return (k === "update" || k === "delete") && !hasWhere(sql);
}
