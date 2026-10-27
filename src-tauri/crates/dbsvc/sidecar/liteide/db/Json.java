package liteide.db;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * 边车和 lite-ide 之间那一行 JSON 的读写（docs/DATABASE.md 第 3 节）。Java 21 没有内置 JSON，为几十行不值得带一个第三方库进包：
 * 对象读成 LinkedHashMap、数组读成 ArrayList、数字读成 Long 或 Double；写的时候反过来。只服务这条线，不是通用的 JSON 库
 */
final class Json {
    private final String s;
    private int i;

    private Json(String s) {
        this.s = s;
    }

    static Object parse(String text) {
        Json p = new Json(text);
        p.ws();
        Object v = p.value();
        p.ws();
        if (p.i != p.s.length()) throw new IllegalArgumentException("JSON 后面还有多余的字符（第 " + p.i + " 个）");
        return v;
    }

    private void ws() {
        while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++;
    }

    private char peek() {
        if (i >= s.length()) throw new IllegalArgumentException("JSON 意外结束");
        return s.charAt(i);
    }

    private Object value() {
        char c = peek();
        switch (c) {
            case '{': return object();
            case '[': return array();
            case '"': return string();
            case 't': lit("true"); return Boolean.TRUE;
            case 'f': lit("false"); return Boolean.FALSE;
            case 'n': lit("null"); return null;
            default: return number();
        }
    }

    private void lit(String w) {
        if (!s.startsWith(w, i)) throw new IllegalArgumentException("JSON 无效（第 " + i + " 个字符）");
        i += w.length();
    }

    private Map<String, Object> object() {
        Map<String, Object> m = new LinkedHashMap<>();
        i++;
        ws();
        if (peek() == '}') { i++; return m; }
        while (true) {
            ws();
            String k = string();
            ws();
            if (peek() != ':') throw new IllegalArgumentException("JSON 缺少冒号（第 " + i + " 个字符）");
            i++;
            ws();
            m.put(k, value());
            ws();
            char c = peek();
            i++;
            if (c == '}') return m;
            if (c != ',') throw new IllegalArgumentException("JSON 缺少逗号（第 " + (i - 1) + " 个字符）");
        }
    }

    private List<Object> array() {
        List<Object> a = new ArrayList<>();
        i++;
        ws();
        if (peek() == ']') { i++; return a; }
        while (true) {
            ws();
            a.add(value());
            ws();
            char c = peek();
            i++;
            if (c == ']') return a;
            if (c != ',') throw new IllegalArgumentException("JSON 缺少逗号（第 " + (i - 1) + " 个字符）");
        }
    }

    private String string() {
        if (peek() != '"') throw new IllegalArgumentException("JSON 这里应是字符串（第 " + i + " 个字符）");
        i++;
        StringBuilder b = new StringBuilder();
        while (true) {
            char c = peek();
            i++;
            if (c == '"') return b.toString();
            if (c != '\\') { b.append(c); continue; }
            char e = peek();
            i++;
            switch (e) {
                case 'n': b.append('\n'); break;
                case 't': b.append('\t'); break;
                case 'r': b.append('\r'); break;
                case 'b': b.append('\b'); break;
                case 'f': b.append('\f'); break;
                case 'u': b.append((char) Integer.parseInt(s.substring(i, i + 4), 16)); i += 4; break;
                default: b.append(e);
            }
        }
    }

    private Object number() {
        int st = i;
        while (i < s.length() && "+-0123456789.eE".indexOf(s.charAt(i)) >= 0) i++;
        String t = s.substring(st, i);
        if (t.isEmpty()) throw new IllegalArgumentException("JSON 无效（第 " + st + " 个字符）");
        if (t.contains(".") || t.contains("e") || t.contains("E")) return Double.parseDouble(t);
        return Long.parseLong(t);
    }

    /** 写：Map / List / String / Number / Boolean / null；别的类型（JDBC 的 BigDecimal、时间……）调用方先转成字符串 */
    static String write(Object v) {
        StringBuilder b = new StringBuilder();
        write(b, v);
        return b.toString();
    }

    @SuppressWarnings("unchecked")
    private static void write(StringBuilder b, Object v) {
        if (v == null) {
            b.append("null");
        } else if (v instanceof String) {
            str(b, (String) v);
        } else if (v instanceof Boolean || v instanceof Long || v instanceof Integer) {
            b.append(v);
        } else if (v instanceof Number) {
            double d = ((Number) v).doubleValue();
            if (Double.isNaN(d) || Double.isInfinite(d)) str(b, v.toString());
            else b.append(v);
        } else if (v instanceof Map) {
            b.append('{');
            boolean first = true;
            for (Map.Entry<String, Object> e : ((Map<String, Object>) v).entrySet()) {
                if (!first) b.append(',');
                first = false;
                str(b, e.getKey());
                b.append(':');
                write(b, e.getValue());
            }
            b.append('}');
        } else if (v instanceof List) {
            b.append('[');
            boolean first = true;
            for (Object x : (List<Object>) v) {
                if (!first) b.append(',');
                first = false;
                write(b, x);
            }
            b.append(']');
        } else {
            str(b, v.toString());
        }
    }

    private static void str(StringBuilder b, String s) {
        b.append('"');
        for (int k = 0; k < s.length(); k++) {
            char c = s.charAt(k);
            switch (c) {
                case '"': b.append("\\\""); break;
                case '\\': b.append("\\\\"); break;
                case '\n': b.append("\\n"); break;
                case '\r': b.append("\\r"); break;
                case '\t': b.append("\\t"); break;
                default:
                    if (c < 0x20) b.append(String.format("\\u%04x", (int) c));
                    else b.append(c);
            }
        }
        b.append('"');
    }
}
