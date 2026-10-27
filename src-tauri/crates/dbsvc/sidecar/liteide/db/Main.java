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
 * DB-1 只有连接和表结构：connect / meta / close / ping。查询（DB-2）往这里加 query / fetch / cancel
 */
public final class Main {
    private static final PrintStream OUT = new PrintStream(System.out, true, StandardCharsets.UTF_8);
    private static final Map<String, Connection> CONNS = new ConcurrentHashMap<>();
    private static final Map<String, ClassLoader> LOADERS = new ConcurrentHashMap<>();

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
        Connection c = CONNS.remove(str(p, "conn"));
        if (c != null) c.close();
        return Boolean.TRUE;
    }
}
