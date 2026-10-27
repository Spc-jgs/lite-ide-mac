package liteide.db;

import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.Driver;
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
 * 连接和表结构：connect / meta / close / ping（DB-1）；查询：query / fetch / cancel / closeCursor（DB-2）。
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
                default: throw new IllegalArgumentException("未知的方法：" + method);
            }
            reply(id, result, null);
        } catch (SQLException e) {
            reply(id, null, describe(e));
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
        if (str(p, "user") != null) props.setProperty("user", str(p, "user"));
        if (str(p, "password") != null) props.setProperty("password", str(p, "password"));
        Connection c = d.connect(str(p, "url"), props);
        if (c == null) throw new IllegalArgumentException("驱动 " + str(p, "driver") + " 不接受这个 URL：" + str(p, "url"));
        Connection old = CONNS.put(key, c);
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

    private static Connection conn(Map<String, Object> p) {
        Connection c = CONNS.get(str(p, "conn"));
        if (c == null) throw new IllegalArgumentException("连接未打开");
        return c;
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
            throw e;
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
        Cursor cur = CURSORS.get(str(p, "conn"));
        if (cur == null) return Boolean.FALSE;
        try {
            cur.st.cancel();
        } catch (SQLException ignored) {
            // 驱动不支持取消、或者语句刚好跑完：没什么可做的，界面照样把这个结果标成已取消
        }
        return Boolean.TRUE;
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
}
