/**
 * SQL 控制台的结果（docs/DATABASE.md 第 5 节 DB-2）：底部「数据库」工具窗里一条语句一个标签。
 *
 * - **同一个连接再跑，没钉住的旧标签换掉**（同 DataGrip：控制台里改一改再 ⌘↵ 是常态，一次一个新标签很快就是几十个）。钉住的留着
 * - **一个连接只有一个开着的游标**（边车那头的规矩）：新查询一开，同一连接上旧结果就取不了下一批了（`stale`），已经取到的照旧看
 * - 同一个连接上还有一条在跑：不再发第二条，说一句（JDBC 的连接不能两条语句同时用；边车会把前一条的语句关掉，那条就以一句莫名其妙的错收场）
 *
 * 行是 `$state.raw`：十万行每格都包一层响应式代理是几十 MB 和几百毫秒；追加一批就换一个新数组，表格按引用变了重画
 */
import type { DbCol } from "../ipc/commands";
import { dbCancel, dbCloseCursor, dbFetch, dbQuery } from "../ipc/db";
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
  cursor = 0;
  cancelling = false;
  readonly started = Date.now();

  constructor(root: string, conn: string, connName: string, sql: string) {
    this.root = root;
    this.conn = conn;
    this.connName = connName;
    this.sql = sql;
  }

  /** 标签上的字：第一行、去掉多余空白，长了截短 */
  get title(): string {
    const one = this.sql.replace(/\s+/g, " ").trim();
    return one.length > 28 ? `${one.slice(0, 27)}…` : one;
  }
}

class Results {
  list = $state<DbRes[]>([]);
  activeId = $state<number | null>(null);

  get active(): DbRes | null {
    return this.list.find((r) => r.id === this.activeId) ?? null;
  }

  /** 跑几条（控制台里选中了好几条就是好几条，一条一个标签，按次序一条跑完再跑下一条） */
  async run(root: string, conn: string, connName: string, sqls: string[]) {
    if (!sqls.length) return;
    if (this.list.some((r) => r.conn === conn && r.root === root && r.status === "running")) {
      notify.hint(`「${connName}」上的查询仍在执行，请先取消或等待完成`);
      return;
    }
    // 换掉这个连接没钉住的旧标签；钉住的留着，但它的游标要被新查询关掉了
    for (const r of this.list.filter((x) => x.conn === conn && x.root === root && !x.pinned)) this.drop(r, false);
    for (const r of this.list) if (r.conn === conn && r.root === root) r.stale = true;
    const made = sqls.map((s) => new DbRes(root, conn, connName, s));
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

  cancel(r: DbRes) {
    if (r.status !== "running") return;
    r.cancelling = true;
    void dbCancel(r.root, r.conn).catch(() => {});
  }

  rerun(r: DbRes) {
    void this.run(r.root, r.conn, r.connName, [r.sql]);
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
