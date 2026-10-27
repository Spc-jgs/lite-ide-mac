/**
 * SQL 控制台的结果（docs/DATABASE.md 第 5 节 DB-2）：底部「数据库」工具窗里一条语句一个标签。
 *
 * - **同一个连接再跑，没钉住的旧标签换掉**（同 DataGrip：控制台里改一改再 ⌘↵ 是常态，一次一个新标签很快就是几十个）。钉住的留着
 * - **一个连接只有一个开着的游标**（边车那头的规矩）：新查询一开，同一连接上旧结果就取不了下一批了（`stale`），已经取到的照旧看
 * - 同一个连接上还有一条在跑：不再发第二条，说一句（JDBC 的连接不能两条语句同时用；边车会把前一条的语句关掉，那条就以一句莫名其妙的错收场）
 * - 执行前先过一道（DB-3，`sql-guard.ts`）：生产库上改数据的、任何库上不带 WHERE 的 UPDATE / DELETE，先弹对话框（`DbConfirm`），说清会动几行
 * - 改单元格（DB-3，`edits.ts`）：双击改的先攒着（`edits`），「提交」先给人看要执行的 UPDATE（`DbApply`），点了才一个事务提交；不提交就不动库
 *
 * 行是 `$state.raw`：十万行每格都包一层响应式代理是几十 MB 和几百毫秒；追加一批就换一个新数组，表格按引用变了重画
 */
import type { DbCol, DbTable } from "../ipc/commands";
import { dbApply, dbCancel, dbCloseCursor, dbFetch, dbQuery } from "../ipc/db";
import { applyLocal, buildEdit, previewSql, type Edits } from "./edits";
import { needsConfirm } from "./sql-guard";
import { layout } from "../state/layout.svelte";
import { notify } from "../state/notify.svelte";

export type DbResStatus = "running" | "done" | "failed" | "cancelled";

let nextId = 1;

export class DbRes {
  readonly id = nextId++;
  readonly root: string;
  readonly conn: string;
  readonly connName: string;
  readonly sql: string;
  /** 生产库：重新运行时照样要过确认 */
  readonly prod: boolean;
  status = $state<DbResStatus>("running");
  kind = $state<"rows" | "count" | null>(null);
  cols = $state.raw<DbCol[]>([]);
  rows = $state.raw<(string | null)[][]>([]);
  more = $state(false);
  fetching = $state(false);
  count = $state<number | null>(null);
  ms = $state(0);
  error = $state("");
  /** 同一连接上后来又跑了查询：游标已经关了，取不了下一批 */
  stale = $state(false);
  pinned = $state(false);
  /** 结果来自哪张表（能不能改格子） */
  table = $state.raw<DbTable | null>(null);
  /** 攒着、还没提交的改动 */
  edits = $state.raw<Edits>({});
  committing = $state(false);
  cursor = 0;
  cancelling = false;
  readonly started = Date.now();

  constructor(root: string, conn: string, connName: string, sql: string, prod: boolean) {
    this.root = root;
    this.conn = conn;
    this.connName = connName;
    this.sql = sql;
    this.prod = prod;
  }

  /** 标签上的字：第一行、去掉多余空白，长了截短 */
  get title(): string {
    const one = this.sql.replace(/\s+/g, " ").trim();
    return one.length > 28 ? `${one.slice(0, 27)}…` : one;
  }
}

export interface ConfirmAsk {
  root: string;
  conn: string;
  connName: string;
  prod: boolean;
  sqls: string[];
}

export interface ApplyAsk {
  connName: string;
  prod: boolean;
  table: string;
  sqls: string[];
}

/** 提交前给人看的那一份（`DbApply`）。状态层测试换成直接给答案的 */
async function askApply(q: ApplyAsk): Promise<boolean> {
  const [{ openModal }, m] = await Promise.all([import("../settings/modal-host"), import("./DbApply.svelte")]);
  return new Promise((done) => {
    let answered = false;
    openModal(m.default, { ...q, onanswer: (go: boolean) => ((answered = true), done(go)) }, () => !answered && done(false));
  });
}

/** 弹确认对话框，等人回答。状态层测试换成直接给答案的（裸 node 里挂不了组件） */
async function askInDialog(q: ConfirmAsk): Promise<boolean> {
  const [{ openModal }, m] = await Promise.all([import("../settings/modal-host"), import("./DbConfirm.svelte")]);
  return new Promise((done) => {
    let answered = false;
    openModal(m.default, { ...q, onanswer: (go: boolean) => ((answered = true), done(go)) }, () => !answered && done(false));
  });
}

class Results {
  list = $state<DbRes[]>([]);
  activeId = $state<number | null>(null);
  confirm: (q: ConfirmAsk) => Promise<boolean> = askInDialog;
  previewApply: (q: ApplyAsk) => Promise<boolean> = askApply;

  get active(): DbRes | null {
    return this.list.find((r) => r.id === this.activeId) ?? null;
  }

  /** 跑几条（控制台里选中了好几条就是好几条，一条一个标签，按次序一条跑完再跑下一条） */
  async run(root: string, conn: string, connName: string, sqls: string[], prod = false) {
    if (!sqls.length) return;
    if (this.list.some((r) => r.conn === conn && r.root === root && r.status === "running")) {
      notify.hint(`「${connName}」上的查询仍在执行，请先取消或等待完成`);
      return;
    }
    const risky = sqls.filter((s) => needsConfirm(s, prod));
    if (risky.length && !(await this.confirm({ root, conn, connName, prod, sqls: risky }))) return;
    // 换掉这个连接没钉住的旧标签；钉住的留着，但它的游标要被新查询关掉了
    for (const r of this.list.filter((x) => x.conn === conn && x.root === root && !x.pinned)) this.drop(r, false);
    for (const r of this.list) if (r.conn === conn && r.root === root) r.stale = true;
    const made = sqls.map((s) => new DbRes(root, conn, connName, s, prod));
    this.list = [...this.list, ...made];
    this.activeId = made[0].id;
    layout.panelView = "db";
    layout.panel = true;
    let prev: DbRes | null = null;
    for (const r of made) {
      // 前一条没成功：后面的不跑（同 IDEA：一串里有一条错了就停）
      if (prev && prev.status !== "done") {
        r.status = "cancelled";
        r.error = "前一条语句未成功，未执行";
        continue;
      }
      // 这一条一开，前一条的游标就被边车关了
      if (prev) prev.stale = true;
      this.activeId = r.id;
      await this.exec(r);
      prev = r;
    }
  }

  private async exec(r: DbRes) {
    try {
      const res = await dbQuery(r.root, r.conn, r.sql);
      r.kind = res.kind;
      r.cols = res.cols;
      r.rows = res.rows;
      r.more = res.more;
      r.count = res.count;
      r.ms = res.ms;
      r.cursor = res.cursor;
      r.table = res.table;
      r.status = "done";
    } catch (e) {
      r.status = r.cancelling ? "cancelled" : "failed";
      r.error = String(e);
      r.ms = Date.now() - r.started;
    }
  }

  /** 滚到底了：要下一批。被顶掉的、正在取的、取完的不取 */
  async fetchMore(r: DbRes) {
    if (!r.more || r.fetching || r.stale || r.status !== "done") return;
    r.fetching = true;
    try {
      const b = await dbFetch(r.root, r.conn, r.cursor);
      r.rows = r.rows.concat(b.rows);
      r.more = b.more;
    } catch (e) {
      r.stale = true;
      notify.fail(`无法读取更多结果：${String(e)}`);
    } finally {
      r.fetching = false;
    }
  }

  /** 攒一格改动；改回原值就不算改动 */
  stage(r: DbRes, row: number, col: number, value: string | null) {
    const k = `${row}\n${col}`;
    const next = { ...r.edits };
    if (value === r.rows[row]?.[col]) delete next[k];
    else next[k] = value;
    r.edits = next;
  }

  discard(r: DbRes) {
    r.edits = {};
  }

  /**
   * 提交攒下来的改动：先给人看 UPDATE，点了才执行（一个事务、每条正好改 1 行，不是就整笔回滚、改动留着）。
   * 提交会关掉这个连接开着的游标（边车那头的规矩），同一连接上的结果都取不了下一批了
   */
  async commit(r: DbRes) {
    const n = Object.keys(r.edits).length;
    if (!n || !r.table || r.committing) return;
    const edit = buildEdit(r.table, r.rows, r.edits);
    const sqls = previewSql(edit);
    if (!(await this.previewApply({ connName: r.connName, prod: r.prod, table: r.table.name, sqls }))) return;
    r.committing = true;
    try {
      await dbApply(r.root, r.conn, edit);
      r.rows = applyLocal(r.rows, r.edits);
      r.edits = {};
      for (const x of this.list) if (x.conn === r.conn && x.root === r.root && x.more) x.stale = true;
      notify.ok(`已提交 ${edit.rows.length} 行的 ${n} 处更改`);
    } catch (e) {
      notify.fail(`提交失败：${String(e)}`);
    } finally {
      r.committing = false;
    }
  }

  cancel(r: DbRes) {
    if (r.status !== "running") return;
    r.cancelling = true;
    void dbCancel(r.root, r.conn).catch(() => {});
  }

  rerun(r: DbRes) {
    void this.run(r.root, r.conn, r.connName, [r.sql], r.prod);
  }

  close(id: number) {
    const r = this.list.find((x) => x.id === id);
    if (r) this.drop(r, true);
  }

  /** 关一个标签：还在跑的先取消；还开着游标的（取到一半、没被顶掉）让边车关掉 —— PostgreSQL 为分批取开着一个事务 */
  private drop(r: DbRes, collapse: boolean) {
    if (r.status === "running") this.cancel(r);
    else if (r.more && !r.stale) void dbCloseCursor(r.root, r.conn).catch(() => {});
    const i = this.list.indexOf(r);
    this.list = this.list.filter((x) => x !== r);
    if (this.activeId === r.id) this.activeId = (this.list[Math.min(i, this.list.length - 1)] ?? null)?.id ?? null;
    // 最后一个关了：工具窗收起（不切到终端 —— 那会顺手起一个 shell）
    if (collapse && !this.list.length && layout.panelView === "db") layout.panel = false;
  }
}

export const results = new Results();
