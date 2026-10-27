package liteide.db;

import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.Driver;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.Statement;
import java.sql.Types;
import java.sql.SQLException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * lite-ide 的数据库边车（docs/DATABASE.md 第 3 节）：stdin / stdout 上的 JSON-RPC 2.0，一行一条（同 ACP，Rust 那头复用 acpsvc 的线）。
 *
 * 一条请求一个线程（连一个不通的主机要等几秒，不能把别的请求堵在后面）；写回话在同一把锁下（两条回话不能交错成一行）。
 * 驱动按 jar 装：同一组 jar 一个 ClassLoader，直接调 {@link Driver#connect} —— DriverManager 只认它自己的 ClassLoader 装进来的驱动，
 * 别的 ClassLoader 里的驱动它看不见（JDBC 的老规矩，DBeaver、IDEA 也是绕开它）。
 *
 * 连接和表结构：connect / meta / close / ping（DB-1）；查询：query / fetch / cancel / closeCursor（DB-2）；导出、单值查询、改单元格、补全用的表和列：export / scalar / apply / outline（DB-3）。
 *
 * 查询的结果不一次取完（docs/DATABASE.md 第 3 节）：开着的 ResultSet 当游标留在这里，界面滚到底再要下一批。
 * **一个连接最多一个开着的游标**：新查询先关掉上一个 —— 结果标签可以开很多个，但每个开着的游标在 MySQL / PostgreSQL 那边都攥着资源
 * （PostgreSQL 为了分批取还得开着一个事务），不能让人开十个标签就攥十份
 */
public final class Main {
    private static final PrintStream OUT = new PrintStream(System.out, true, StandardCharsets.UTF_8);
    private static final Map<String, Connection> CONNS = new ConcurrentHashMap<>();
    private static final Map<String, ClassLoader> LOADERS = new ConcurrentHashMap<>();
    /** 每个连接正在跑、或者结果还开着的那条语句：取消按它，关游标也按它 */
    private static final Map<String, Cursor> CURSORS = new ConcurrentHashMap<>();
    /** 一批最多几行；字数到了也停（单条消息 Rust 那头设了闸，一行几十列长文本时 500 行会过线） */
    private static final int BATCH = 500;
    private static final int BATCH_CHARS = 4 << 20;
    /** 一格最多给界面多少字：CLOB / 大 JSON 列一格几 MB，表格里只看得见开头 */
    private static final int CELL_CHARS = 10_000;
    private static long nextCursor = 1;
    /** 正在导出的语句（按连接）：取消时一起停。导出不占「一个连接一个游标」那个位置 —— 你正在看的结果导出完还能接着往下滚 */
    private static final Map<String, Statement> EXPORTS = new ConcurrentHashMap<>();
    /**
     * 叫停了的导出：写盘的循环每行看一眼。光靠 Statement.cancel() 停不下来 —— 分批取的时候语句在服务器上早就执行完了
     * （MySQL 结果在临时表里、PostgreSQL 停在游标上），两批之间服务器是空闲的，KILL QUERY / cancel request 落了空，
     * 导出照样写完（2026-10-28 连真库撞的；H2 的懒执行每产一行都查取消标志，所以 db.sh 一直是绿的）
     */
    private static final java.util.Set<Statement> STOP = ConcurrentHashMap.newKeySet();
    /** 怎么连上的（按连接）：取消之后要换一个新连接时照原样再连一次 */
    private static final Map<String, Reopen> ARGS = new ConcurrentHashMap<>();
    /**
     * 取消过语句、不能再用的 MySQL 连接：下一次用到时先换一个新的（{@link #conn}）。
     * Connector/J 9.1 用服务器端游标（useCursorFetch）跑的语句被 KILL QUERY 打断之后，收发差了一拍：
     * 接下来每条语句读到的都是上一步的应答（2026-10-28 连真 MySQL 8.4 撞的：之后的 select 1 回「影响了 2 行」—— 2 是服务器给新预编译语句的编号）。
     * 不开服务器端游标时取消是干净的，但那样十万行要整个读进内存；改用流式结果的话，关一个没读完的结果驱动要把剩下的行全读完
     */
    private static final java.util.Set<String> STALE = ConcurrentHashMap.newKeySet();

    private record Reopen(Driver driver, String url, Properties props) {}

    /** 一条查询：语句、结果（没有结果集的语句 rs 是 null）、PostgreSQL 为分批取临时关掉的自动提交 */
    private static final class Cursor {
        final long id;
        final Statement st;
        ResultSet rs;
        boolean restoreAutoCommit;
        Cursor(long id, Statement st) { this.id = id; this.st = st; }
    }

    public static void main(String[] args) throws Exception {
        ExecutorService pool = Executors.newCachedThreadPool(r -> {
            Thread t = new Thread(r, "lite-db");
            t.setDaemon(true);
            return t;
        });
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        String line;
        while ((line = in.readLine()) != null) {
            if (line.isBlank()) continue;
            final String l = line;
            pool.submit(() -> handle(l));
        }
        // stdin 关了 = lite-ide 走了：连接都关掉再退，不留着占数据库的连接数
        for (Connection c : CONNS.values()) {
            try { c.close(); } catch (SQLException ignored) {}
        }
        System.exit(0);
    }

    @SuppressWarnings("unchecked")
    private static void handle(String line) {
        Object id = null;
        try {
            Map<String, Object> req = (Map<String, Object>) Json.parse(line);
            id = req.get("id");
            String method = (String) req.get("method");
            Map<String, Object> p = req.get("params") instanceof Map ? (Map<String, Object>) req.get("params") : Map.of();
            Object result;
            switch (method == null ? "" : method) {
                case "ping": result = "pong"; break;
                case "connect": result = connect(p); break;
                case "meta": result = meta(p); break;
                case "close": result = close(p); break;
                case "query": result = query(p); break;
                case "fetch": result = fetch(p); break;
                case "cancel": result = cancel(p); break;
                case "closeCursor": closeCursor(str(p, "conn")); result = Boolean.TRUE; break;
                case "export": result = export(p); break;
                case "scalar": result = scalar(p); break;
                case "apply": result = apply(p); break;
                case "outline": result = outline(p); break;
                default: throw new IllegalArgumentException("未知的方法：" + method);
            }
            reply(id, result, null);
        } catch (SQLException e) {
            reply(id, null, describe(e));
        } catch (IllegalArgumentException | IllegalStateException e) {
            // 我们自己写的那几句（「结果集已关闭…」「第 2 处更改影响了 0 行…」）：原话就是给人看的，不带 Java 的类名
            reply(id, null, e.getMessage());
        } catch (Throwable e) {
            reply(id, null, e.getClass().getSimpleName() + "：" + e.getMessage());
        }
    }

    /** SQLException 带上 SQLState 和厂商错误码（MySQL 的 1045 就是用户名或密码不对，比一句英文好查） */
    private static String describe(SQLException e) {
        StringBuilder b = new StringBuilder(e.getMessage() == null ? e.getClass().getSimpleName() : e.getMessage());
        if (e.getSQLState() != null) b.append("（SQLState ").append(e.getSQLState());
        if (e.getErrorCode() != 0) b.append(e.getSQLState() != null ? "，" : "（").append("错误码 ").append(e.getErrorCode());
        if (e.getSQLState() != null || e.getErrorCode() != 0) b.append("）");
        return b.toString();
    }

    private static void reply(Object id, Object result, String error) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("jsonrpc", "2.0");
        m.put("id", id);
        if (error == null) {
            m.put("result", result);
        } else {
            Map<String, Object> e = new LinkedHashMap<>();
            e.put("code", -32000L);
            e.put("message", error);
            m.put("error", e);
        }
        String s = Json.write(m);
        synchronized (OUT) {
            OUT.println(s);
        }
    }

    private static String str(Map<String, Object> p, String k) {
        Object v = p.get(k);
        return v == null ? null : v.toString();
    }

    @SuppressWarnings("unchecked")
    private static Object connect(Map<String, Object> p) throws Exception {
        String key = str(p, "conn");
        List<Object> jars = p.get("jars") instanceof List ? (List<Object>) p.get("jars") : List.of();
        ClassLoader loader = LOADERS.computeIfAbsent(String.join("\n", jars.stream().map(Object::toString).toList()), k -> {
            List<URL> urls = new ArrayList<>();
            for (Object j : jars) {
                try { urls.add(new File(j.toString()).toURI().toURL()); } catch (Exception e) { throw new IllegalArgumentException(e); }
            }
            return new URLClassLoader(urls.toArray(new URL[0]), Main.class.getClassLoader());
        });
        Driver d = (Driver) Class.forName(str(p, "driver"), true, loader).getDeclaredConstructor().newInstance();
        Properties props = new Properties();
        // MySQL 不开这个就把整个结果读进内存再给第一行（setFetchSize 只在它开着时生效）；URL 里自己写了的不盖
        if (str(p, "driver").startsWith("com.mysql.") && !str(p, "url").contains("useCursorFetch")) props.setProperty("useCursorFetch", "true");
        // H2 同理：默认把整个结果算进内存才给第一行（十万乘十万的交叉连接直接把堆撑爆、连库一起关掉 —— 2026-10-27 探针撞的）。
        // 懒执行是边取边算；URL 里自己写了的不盖
        if (str(p, "driver").startsWith("org.h2.") && !str(p, "url").toUpperCase().contains("LAZY_QUERY_EXECUTION")) props.setProperty("LAZY_QUERY_EXECUTION", "TRUE");
        // 改单元格（apply）一律用 setString 传值：PostgreSQL 默认把它当 varchar 发，往 integer 列里写会报类型不符；unspecified 让服务器自己按列的类型转。
        // 2026-10-28 连真 PostgreSQL 17 验过（scripts/accept/db-real.sh：往 numeric 列写 '9.99'）
        if (str(p, "driver").startsWith("org.postgresql.") && !str(p, "url").contains("stringtype")) props.setProperty("stringtype", "unspecified");
        // SQLite 默认「文件不存在就建一个」：路径写错一个字，连一下就在你的项目里多出一个空库。只开已有的（读写、不建）；
        // 真要新建的在 URL 里自己写 ?open_mode=6。值是 SQLite 的打开标志位（READWRITE = 2），2026-10-27 拿 3.53.4.0 探过：不存在的报 SQLITE_CANTOPEN、文件没被建出来
        if (str(p, "driver").startsWith("org.sqlite.") && !str(p, "url").contains("open_mode")) props.setProperty("open_mode", "2");
        if (str(p, "user") != null) props.setProperty("user", str(p, "user"));
        if (str(p, "password") != null) props.setProperty("password", str(p, "password"));
        Connection c = d.connect(str(p, "url"), props);
        if (c == null) throw new IllegalArgumentException("驱动 " + str(p, "driver") + " 不接受这个 URL：" + str(p, "url"));
        Connection old = CONNS.put(key, c);
        ARGS.put(key, new Reopen(d, str(p, "url"), props));
        STALE.remove(key);
        if (old != null) {
            try { old.close(); } catch (SQLException ignored) {}
        }
        DatabaseMetaData m = c.getMetaData();
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("product", m.getDatabaseProductName());
        r.put("version", m.getDatabaseProductVersion());
        r.put("catalog", c.getCatalog());
        String schema = null;
        try { schema = c.getSchema(); } catch (Throwable ignored) {}
        r.put("schema", schema);
        return r;
    }

    private static Connection conn(Map<String, Object> p) throws SQLException {
        String key = str(p, "conn");
        if (STALE.contains(key)) reopen(key);
        Connection c = CONNS.get(key);
        if (c == null) throw new IllegalArgumentException("连接未打开");
        return c;
    }

    /** 换掉取消过语句的连接（{@link #STALE}）。连不上就留着标记，下一次再试 */
    private static void reopen(String key) throws SQLException {
        // 锁 STALE 不锁 Main.class：连不上的服务器要等到连接超时，别让别的连接分配游标号时跟着等
        synchronized (STALE) {
            reopen0(key);
        }
    }

    private static void reopen0(String key) throws SQLException {
        if (!STALE.contains(key)) return;
        Reopen a = ARGS.get(key);
        if (a == null) return;
        Connection fresh = a.driver().connect(a.url(), a.props());
        if (fresh == null) throw new IllegalArgumentException("无法重新连接");
        closeCursor(key);
        Connection old = CONNS.put(key, fresh);
        STALE.remove(key);
        if (old != null) {
            try { old.close(); } catch (SQLException ignored) {}
        }
    }

    /** 取消了的语句报的错，带上「这个连接要换了」（MySQL） */
    private static SQLException noteStale(String key, SQLException e) {
        if (!STALE.contains(key)) return e;
        return new SQLException(e.getMessage() + "。下次执行时将重新连接：未提交的事务会回滚，会话变量和临时表需重新设置", e.getSQLState(), e.getErrorCode(), e);
    }

    /**
     * 表结构树的一层（懒加载：展开到哪一层问哪一层）。
     * schemas：有 schema 的库（PostgreSQL、H2、Oracle）列 schema；MySQL 没有 schema，库就是 catalog —— 列 catalog。两种都回成 {name, catalog, schema}
     */
    private static Object meta(Map<String, Object> p) throws SQLException {
        Connection c = conn(p);
        DatabaseMetaData m = c.getMetaData();
        String what = str(p, "what");
        String catalog = str(p, "catalog");
        String schema = str(p, "schema");
        List<Object> out = new ArrayList<>();
        if ("schemas".equals(what)) {
            try (ResultSet r = m.getSchemas()) {
                while (r.next()) out.add(node(r.getString("TABLE_SCHEM"), safe(r, "TABLE_CATALOG"), r.getString("TABLE_SCHEM")));
            }
            if (out.isEmpty()) {
                try (ResultSet r = m.getCatalogs()) {
                    while (r.next()) out.add(node(r.getString("TABLE_CAT"), r.getString("TABLE_CAT"), null));
                }
            }
            // SQLite：schema、catalog 都没有，树的第一层就空着、点不进去了。给一个 main（SQLite 自己对主库的叫法），表照 catalog / schema 都是 null 去取
            if (out.isEmpty()) out.add(node("main", null, null));
        } else if ("tables".equals(what)) {
            try (ResultSet r = m.getTables(catalog, schema, "%", new String[] {"TABLE", "VIEW", "BASE TABLE"})) {
                while (r.next()) {
                    Map<String, Object> n = new LinkedHashMap<>();
                    n.put("name", r.getString("TABLE_NAME"));
                    n.put("kind", r.getString("TABLE_TYPE"));
                    n.put("remarks", r.getString("REMARKS"));
                    out.add(n);
                }
            }
        } else if ("columns".equals(what)) {
            String table = str(p, "table");
            List<String> pk = new ArrayList<>();
            try (ResultSet r = m.getPrimaryKeys(catalog, schema, table)) {
                while (r.next()) pk.add(r.getString("COLUMN_NAME"));
            }
            try (ResultSet r = m.getColumns(catalog, schema, table, "%")) {
                while (r.next()) {
                    Map<String, Object> n = new LinkedHashMap<>();
                    String name = r.getString("COLUMN_NAME");
                    n.put("name", name);
                    n.put("type", r.getString("TYPE_NAME"));
                    n.put("size", (long) r.getInt("COLUMN_SIZE"));
                    n.put("nullable", r.getInt("NULLABLE") == DatabaseMetaData.columnNullable);
                    n.put("default", r.getString("COLUMN_DEF"));
                    n.put("remarks", r.getString("REMARKS"));
                    n.put("pk", pk.contains(name));
                    out.add(n);
                }
            }
        } else {
            throw new IllegalArgumentException("未知的元数据类型：" + what);
        }
        return out;
    }

    private static String safe(ResultSet r, String col) {
        try { return r.getString(col); } catch (SQLException e) { return null; }
    }

    private static Map<String, Object> node(String name, String catalog, String schema) {
        Map<String, Object> n = new LinkedHashMap<>();
        n.put("name", name);
        n.put("catalog", catalog);
        n.put("schema", schema);
        return n;
    }

    private static Object close(Map<String, Object> p) throws SQLException {
        closeCursor(str(p, "conn"));
        Connection c = CONNS.remove(str(p, "conn"));
        ARGS.remove(str(p, "conn"));
        STALE.remove(str(p, "conn"));
        if (c != null) c.close();
        return Boolean.TRUE;
    }

    /**
     * 跑一条 SQL。有结果集的回列定义 + 第一批；没有的回影响了几行。
     * 不设超时（同 git fetch：慢查询本来就要等），停它靠 cancel —— 从另一条线程调 {@link Statement#cancel()}，JDBC 认这种用法
     */
    private static Object query(Map<String, Object> p) throws SQLException {
        String key = str(p, "conn");
        Connection c = conn(p);
        closeCursor(key);
        String sql = str(p, "sql");
        Statement st = c.createStatement();
        Cursor cur;
        synchronized (Main.class) {
            cur = new Cursor(nextCursor++, st);
        }
        CURSORS.put(key, cur);
        long t0 = System.nanoTime();
        try {
            st.setFetchSize(BATCH);
            // PostgreSQL 只在事务里才按 fetchSize 分批（自动提交开着时它把整个结果读进内存）。只读的语句临时关掉自动提交、关游标时提交 ——
            // 提交不回滚：`WITH x AS (DELETE …) SELECT` 也是 WITH 开头，回滚会悄悄撤掉人家要做的删除
            if (c.getAutoCommit() && c.getMetaData().getDatabaseProductName().toLowerCase().contains("postgres") && readsOnly(sql)) {
                c.setAutoCommit(false);
                cur.restoreAutoCommit = true;
            }
            boolean rows = st.execute(sql);
            Map<String, Object> r = new LinkedHashMap<>();
            r.put("cursor", cur.id);
            if (rows) {
                cur.rs = st.getResultSet();
                ResultSetMetaData md = cur.rs.getMetaData();
                List<Object> cols = new ArrayList<>();
                for (int i = 1; i <= md.getColumnCount(); i++) {
                    Map<String, Object> col = new LinkedHashMap<>();
                    col.put("name", md.getColumnLabel(i));
                    col.put("type", md.getColumnTypeName(i));
                    col.put("num", numeric(md.getColumnType(i)));
                    cols.add(col);
                }
                r.put("kind", "rows");
                r.put("cols", cols);
                r.put("table", editTarget(c, md));
                batch(cur, r);
            } else {
                r.put("kind", "count");
                r.put("count", (long) st.getUpdateCount());
                closeIf(key, cur);
            }
            r.put("ms", (System.nanoTime() - t0) / 1_000_000L);
            return r;
        } catch (SQLException e) {
            closeIf(key, cur);
            throw noteStale(key, e);
        }
    }

    /** 下一批。游标被新查询顶掉了、或者连接关了，说清楚（界面上那个标签的结果就停在已经取到的那些） */
    private static Object fetch(Map<String, Object> p) throws SQLException {
        String key = str(p, "conn");
        Cursor cur = CURSORS.get(key);
        Object want = p.get("cursor");
        if (cur == null || cur.rs == null || want == null || cur.id != ((Number) want).longValue()) {
            throw new IllegalArgumentException("结果集已关闭：同一连接上已执行新的查询，请重新运行此查询");
        }
        Map<String, Object> r = new LinkedHashMap<>();
        batch(cur, r);
        return r;
    }

    private static void batch(Cursor cur, Map<String, Object> r) throws SQLException {
        int n = cur.rs.getMetaData().getColumnCount();
        int[] types = new int[n];
        for (int i = 0; i < n; i++) types[i] = cur.rs.getMetaData().getColumnType(i + 1);
        List<Object> rows = new ArrayList<>();
        long chars = 0;
        boolean more = false;
        while (rows.size() < BATCH && chars < BATCH_CHARS) {
            if (!cur.rs.next()) {
                more = false;
                break;
            }
            List<Object> row = new ArrayList<>(n);
            for (int i = 0; i < n; i++) {
                String v = cell(cur.rs, i + 1, types[i]);
                if (v != null) chars += v.length();
                row.add(v);
            }
            rows.add(row);
            more = true;
        }
        // 正好停在批的边界上时 more 是 true，下一次取回来零行、more 变 false：多一次往返，换不必先读一行再「还回去」
        r.put("rows", rows);
        r.put("more", more);
        if (!more) {
            for (Map.Entry<String, Cursor> e : CURSORS.entrySet()) closeIf(e.getKey(), cur);
        }
    }

    /** 一格的字：一律字符串（BIGINT、DECIMAL 过 JSON 到 JavaScript 会丢精度），NULL 是 null（和空串分开） */
    private static String cell(ResultSet rs, int i, int type) throws SQLException {
        if (type == Types.BINARY || type == Types.VARBINARY || type == Types.LONGVARBINARY || type == Types.BLOB) {
            byte[] b = rs.getBytes(i);
            if (b == null) return null;
            StringBuilder h = new StringBuilder("0x");
            for (int k = 0; k < Math.min(b.length, 32); k++) h.append(String.format("%02X", b[k]));
            if (b.length > 32) h.append("…（").append(b.length).append(" 字节）");
            return h.toString();
        }
        String v = rs.getString(i);
        if (v != null && v.length() > CELL_CHARS) v = v.substring(0, CELL_CHARS) + "…（共 " + v.length() + " 字符）";
        return v;
    }

    private static boolean numeric(int t) {
        return t == Types.TINYINT || t == Types.SMALLINT || t == Types.INTEGER || t == Types.BIGINT || t == Types.DECIMAL || t == Types.NUMERIC
            || t == Types.REAL || t == Types.FLOAT || t == Types.DOUBLE;
    }

    /** 去掉开头的空白和注释，看第一个词 */
    static boolean readsOnly(String sql) {
        String s = sql.stripLeading();
        while (true) {
            if (s.startsWith("--")) {
                int nl = s.indexOf('\n');
                s = nl < 0 ? "" : s.substring(nl + 1).stripLeading();
            } else if (s.startsWith("/*")) {
                int end = s.indexOf("*/");
                s = end < 0 ? "" : s.substring(end + 2).stripLeading();
            } else {
                break;
            }
        }
        String w = s.split("[\\s(]", 2)[0].toLowerCase();
        return w.equals("select") || w.equals("with") || w.equals("values") || w.equals("table") || w.equals("show") || w.equals("explain");
    }

    private static Object cancel(Map<String, Object> p) {
        // 驱动不支持取消、或者语句刚好跑完：没什么可做的，界面照样把这个结果标成已取消
        Cursor cur = CURSORS.get(str(p, "conn"));
        if (cur != null) {
            try { cur.st.cancel(); } catch (SQLException ignored) {}
        }
        Statement ex = EXPORTS.get(str(p, "conn"));
        if (ex != null) {
            STOP.add(ex);
            try { ex.cancel(); } catch (SQLException ignored) {}
        }
        Reopen a = ARGS.get(str(p, "conn"));
        if ((cur != null || ex != null) && a != null && a.driver().getClass().getName().startsWith("com.mysql.")) STALE.add(str(p, "conn"));
        return cur != null || ex != null;
    }

    /**
     * 只在它还是这个连接当前的那条时才关：新查询先关掉旧的，旧的那条 execute 随即抛错、走到自己的 catch ——
     * 那时按连接名去关，关掉的是**新查询**的游标（新结果第一批之后就再也取不到下一批）
     */
    private static void closeIf(String key, Cursor cur) {
        if (CURSORS.remove(key, cur)) release(key, cur);
    }

    private static void closeCursor(String key) {
        if (key == null) return;
        Cursor cur = CURSORS.remove(key);
        if (cur != null) release(key, cur);
    }

    private static void release(String key, Cursor cur) {
        try { if (cur.rs != null) cur.rs.close(); } catch (SQLException ignored) {}
        try { cur.st.close(); } catch (SQLException ignored) {}
        if (cur.restoreAutoCommit) {
            Connection c = CONNS.get(key);
            try {
                if (c != null) {
                    c.commit();
                    c.setAutoCommit(true);
                }
            } catch (SQLException ignored) {}
        }
    }

    /**
     * 把一条查询的结果整个写进文件（docs/DATABASE.md 第 3 节：边车直接写盘、不经过 IPC，几百万行也不撑内存）。
     * 先写 `<path>.part`，写完再改名过去：中途取消、出错，原来那个文件（保存面板里确认过覆盖的）不会只剩半截。
     * 格子不截短（表格里那个一万字的上限只管显示），NULL：CSV 是空、JSON 是 null、INSERT 是 NULL
     */
    private static Object export(Map<String, Object> p) throws Exception {
        String key = str(p, "conn");
        Connection c = conn(p);
        String sql = str(p, "sql");
        String format = str(p, "format");
        Path out = Path.of(str(p, "path"));
        Path part = out.resolveSibling(out.getFileName() + ".part");
        long t0 = System.nanoTime();
        long rows = 0;
        boolean restore = false;
        Statement st = c.createStatement();
        EXPORTS.put(key, st);
        try {
            st.setFetchSize(BATCH);
            // 同 query：PostgreSQL 只在事务里分批取。已经在事务里（结果标签开着游标）就不碰，提交会把那个游标一起关掉
            if (c.getAutoCommit() && c.getMetaData().getDatabaseProductName().toLowerCase().contains("postgres") && readsOnly(sql)) {
                c.setAutoCommit(false);
                restore = true;
            }
            try (ResultSet rs = st.executeQuery(sql); Writer w = Files.newBufferedWriter(part, StandardCharsets.UTF_8)) {
                ResultSetMetaData md = rs.getMetaData();
                int n = md.getColumnCount();
                String[] names = new String[n];
                int[] types = new int[n];
                for (int i = 0; i < n; i++) {
                    names[i] = md.getColumnLabel(i + 1);
                    types[i] = md.getColumnType(i + 1);
                }
                String table = str(p, "table");
                if ("csv".equals(format)) {
                    for (int i = 0; i < n; i++) w.write((i > 0 ? "," : "") + csv(names[i]));
                    w.write("\r\n");
                } else if ("json".equals(format)) {
                    w.write("[");
                }
                while (rs.next()) {
                    if (STOP.contains(st)) throw new SQLException("已取消导出");
                    String[] v = new String[n];
                    for (int i = 0; i < n; i++) v[i] = full(rs, i + 1, types[i]);
                    if ("csv".equals(format)) {
                        for (int i = 0; i < n; i++) w.write((i > 0 ? "," : "") + (v[i] == null ? "" : csv(v[i])));
                        w.write("\r\n");
                    } else if ("json".equals(format)) {
                        Map<String, Object> o = new LinkedHashMap<>();
                        for (int i = 0; i < n; i++) o.put(names[i], v[i]);
                        w.write((rows > 0 ? ",\n" : "\n") + Json.write(o));
                    } else {
                        StringBuilder b = new StringBuilder("INSERT INTO ").append(table == null || table.isBlank() ? "t" : table).append(" (");
                        for (int i = 0; i < n; i++) b.append(i > 0 ? ", " : "").append(names[i]);
                        b.append(") VALUES (");
                        for (int i = 0; i < n; i++) b.append(i > 0 ? ", " : "").append(v[i] == null ? "NULL" : numeric(types[i]) ? v[i] : "'" + v[i].replace("'", "''") + "'");
                        w.write(b.append(");\n").toString());
                    }
                    rows++;
                }
                if ("json".equals(format)) w.write(rows > 0 ? "\n]\n" : "]\n");
            }
            Files.move(part, out, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (Exception e) {
            Files.deleteIfExists(part);
            throw e instanceof SQLException se ? noteStale(key, se) : e;
        } finally {
            EXPORTS.remove(key, st);
            STOP.remove(st);
            try { st.close(); } catch (SQLException ignored) {}
            if (restore) {
                try { c.commit(); c.setAutoCommit(true); } catch (SQLException ignored) {}
            }
        }
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("rows", rows);
        r.put("bytes", Files.size(out));
        r.put("ms", (System.nanoTime() - t0) / 1_000_000L);
        return r;
    }

    /** 导出用的一格：同 {@link #cell}，但不截短 */
    private static String full(ResultSet rs, int i, int type) throws SQLException {
        if (type == Types.BINARY || type == Types.VARBINARY || type == Types.LONGVARBINARY || type == Types.BLOB) {
            byte[] b = rs.getBytes(i);
            if (b == null) return null;
            StringBuilder h = new StringBuilder("0x");
            for (byte x : b) h.append(String.format("%02X", x));
            return h.toString();
        }
        return rs.getString(i);
    }

    /** RFC 4180：有逗号、引号、换行的才加引号，引号写两遍 */
    private static String csv(String v) {
        if (v.indexOf(',') < 0 && v.indexOf('"') < 0 && v.indexOf('\n') < 0 && v.indexOf('\r') < 0) return v;
        return '"' + v.replace("\"", "\"\"") + '"';
    }

    /**
     * 一条查询的第一行第一格（生产库保护：先数一数「这条 UPDATE / DELETE 会动几行」）。
     * 有时限（`seconds`）：生产库上一个没索引的 COUNT 可能扫很久，数不出来就说数不出来，不能让确认框一直转着
     */
    private static Object scalar(Map<String, Object> p) throws SQLException {
        Connection c = conn(p);
        try (Statement st = c.createStatement()) {
            Object sec = p.get("seconds");
            if (sec instanceof Number) st.setQueryTimeout(((Number) sec).intValue());
            try (ResultSet rs = st.executeQuery(str(p, "sql"))) {
                return rs.next() ? rs.getString(1) : null;
            }
        }
    }

    /**
     * 这个结果能不能改、改的是哪张表（DB-3 改单元格）：每一列都来自同一张表，才算。回 {catalog, schema, name, cols, pk}：
     * cols 是每一列在表里的真名（列标签可能是别名，SET 要用真名），pk 是主键列在结果里是第几列 —— 主键不全在结果里就是空的，界面说「不能改」。
     * 多表、表达式列（表名是空的）回 null
     */
    private static Object editTarget(Connection c, ResultSetMetaData md) {
        try {
            int n = md.getColumnCount();
            String table = null, schema = null, catalog = null;
            List<Object> cols = new ArrayList<>();
            for (int i = 1; i <= n; i++) {
                String t = tableName(md, i);
                if (t == null || t.isEmpty() || (table != null && !table.equals(t))) return null;
                table = t;
                schema = md.getSchemaName(i);
                catalog = md.getCatalogName(i);
                cols.add(md.getColumnName(i));
            }
            List<String> pk = new ArrayList<>();
            try (ResultSet r = c.getMetaData().getPrimaryKeys(empty(catalog), empty(schema), table)) {
                while (r.next()) pk.add(r.getString("COLUMN_NAME"));
            }
            List<Object> at = new ArrayList<>();
            for (String k : pk) {
                int idx = -1;
                for (int i = 0; i < cols.size(); i++) if (k.equalsIgnoreCase((String) cols.get(i))) idx = i;
                if (idx < 0) {
                    at.clear();
                    break;
                }
                at.add((long) idx);
            }
            Map<String, Object> t = new LinkedHashMap<>();
            t.put("catalog", empty(catalog));
            t.put("schema", empty(schema));
            t.put("name", table);
            t.put("cols", cols);
            t.put("pk", at);
            return t;
        } catch (Throwable e) {
            return null;
        }
    }

    private static String empty(String s) {
        return s == null || s.isEmpty() ? null : s;
    }

    /**
     * 列来自哪张表。先问标准的 getTableName；空的话再试驱动自己的 getBaseTableName（PostgreSQL 的驱动有这个方法）。
     * 两个各家驱动给的是不是真表名（还是别名）没有逐家读源码确认 —— 给错了 UPDATE 会报「表不存在」、整笔回滚，不会改错地方
     */
    private static String tableName(ResultSetMetaData md, int i) throws SQLException {
        String t = md.getTableName(i);
        if (t != null && !t.isEmpty()) return t;
        try {
            Object b = md.getClass().getMethod("getBaseTableName", int.class).invoke(md, i);
            return b == null ? null : b.toString();
        } catch (ReflectiveOperationException e) {
            return null;
        }
    }

    /**
     * 提交改过的格子（DB-3）：一行一条 `UPDATE 表 SET 列 = ? … WHERE 主键 = ? …`，参数化（值不拼进 SQL），全在一个事务里。
     * **每条必须正好改 1 行**，不是就整笔回滚、说清是第几条：0 行 = 这行已经被别人改了主键或者删了；多于 1 行 = 「主键」其实不唯一 —— 两种都不能悄悄提交。
     * 先关掉这个连接开着的游标（PostgreSQL 分批取开着的事务要先提交掉，不然这里的提交会把它一起带走）
     */
    @SuppressWarnings("unchecked")
    private static Object apply(Map<String, Object> p) throws SQLException {
        String key = str(p, "conn");
        Connection c = conn(p);
        closeCursor(key);
        DatabaseMetaData dm = c.getMetaData();
        String q = dm.getIdentifierQuoteString();
        q = q == null || q.isBlank() ? "" : q.trim();
        String name = (str(p, "schema") != null ? quote(q, str(p, "schema")) + "." : str(p, "catalog") != null ? quote(q, str(p, "catalog")) + "." : "") + quote(q, str(p, "table"));
        List<Object> rows = (List<Object>) p.get("rows");
        boolean ac = c.getAutoCommit();
        long t0 = System.nanoTime();
        c.setAutoCommit(false);
        try {
            int k = 0;
            for (Object o : rows) {
                k++;
                Map<String, Object> row = (Map<String, Object>) o;
                List<Object> set = (List<Object>) row.get("set");
                List<Object> where = (List<Object>) row.get("key");
                StringBuilder sql = new StringBuilder("UPDATE ").append(name).append(" SET ");
                for (int i = 0; i < set.size(); i++) sql.append(i > 0 ? ", " : "").append(quote(q, (String) ((Map<String, Object>) set.get(i)).get("col"))).append(" = ?");
                sql.append(" WHERE ");
                for (int i = 0; i < where.size(); i++) sql.append(i > 0 ? " AND " : "").append(quote(q, (String) ((Map<String, Object>) where.get(i)).get("col"))).append(" = ?");
                try (PreparedStatement ps = c.prepareStatement(sql.toString())) {
                    int i = 1;
                    for (Object x : set) bind(ps, i++, ((Map<String, Object>) x).get("value"));
                    for (Object x : where) bind(ps, i++, ((Map<String, Object>) x).get("value"));
                    int n = ps.executeUpdate();
                    if (n != 1) {
                        throw new IllegalStateException("第 " + k + " 处更改影响了 " + n + " 行（应为 1 行）"
                            + (n == 0 ? "：该行可能已被修改或删除" : "：主键不唯一") + "，全部更改已撤回");
                    }
                }
            }
            c.commit();
        } catch (SQLException | RuntimeException e) {
            try { c.rollback(); } catch (SQLException ignored) {}
            throw e;
        } finally {
            try { c.setAutoCommit(ac); } catch (SQLException ignored) {}
        }
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("applied", (long) rows.size());
        r.put("ms", (System.nanoTime() - t0) / 1_000_000L);
        return r;
    }

    private static void bind(PreparedStatement ps, int i, Object v) throws SQLException {
        if (v == null) ps.setNull(i, Types.VARCHAR);
        else ps.setString(i, v.toString());
    }

    /** 名字加上这个库的引号（MySQL 是反引号、别的是双引号），名字里的引号写两遍 */
    private static String quote(String q, String name) {
        return q.isEmpty() ? name : q + name.replace(q, q + q) + q;
    }

    private static final int OUTLINE_MAX = 50_000;
    private static final java.util.Set<String> SYSTEM_SCHEMAS = java.util.Set.of("information_schema", "pg_catalog", "pg_toast", "mysql", "performance_schema", "sys");

    /**
     * 控制台里补全表名、列名用的（DB-3）：当前库里每张表的每一列，一次 getColumns 拿完（一张表一次的话几百张表就是几百个来回）。
     * 系统库不收（H2 的 INFORMATION_SCHEMA 一个就几百列，补全里全是它）。最多 5 万列，多了就停、说截断了（不悄悄少一截）。
     * 回 {defaultSchema, cols: [{schema, table, name, type}], truncated}：MySQL 没有 schema，库名（catalog）当 schema 用
     */
    private static Object outline(Map<String, Object> p) throws SQLException {
        Connection c = conn(p);
        DatabaseMetaData m = c.getMetaData();
        String cat = c.getCatalog();
        String sch = null;
        try { sch = c.getSchema(); } catch (Throwable ignored) {}
        List<Object> cols = new ArrayList<>();
        boolean truncated = false;
        try (ResultSet r = m.getColumns(cat, null, "%", "%")) {
            while (r.next()) {
                String schema = r.getString("TABLE_SCHEM");
                if (schema == null) schema = r.getString("TABLE_CAT");
                if (schema != null && SYSTEM_SCHEMAS.contains(schema.toLowerCase())) continue;
                if (cols.size() >= OUTLINE_MAX) {
                    truncated = true;
                    break;
                }
                Map<String, Object> n = new LinkedHashMap<>();
                n.put("schema", schema);
                n.put("table", r.getString("TABLE_NAME"));
                n.put("name", r.getString("COLUMN_NAME"));
                n.put("type", r.getString("TYPE_NAME"));
                cols.add(n);
            }
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("defaultSchema", sch != null ? sch : cat);
        out.put("cols", cols);
        out.put("truncated", truncated);
        return out;
    }
}
