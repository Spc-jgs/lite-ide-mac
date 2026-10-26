/**
 * Spring Boot 配置文件里「光标在哪个键上」（docs/TAKEOVER.md G30）：补全要知道前面的父键、悬停要知道整个键名。
 * 不带依赖，裸 node 里测（tests/spring-path.test.ts）。
 *
 * YAML 只认 Spring 配置里常见的那几种写法：`键:` 换行缩进写子键、`键: 值`、注释、`---` 分文档。列表（`- `）里面的不认父键
 * （那是一组对象，不是配置键的一段），`键: |` 这种多行字面量里面的也不当键 —— 认错了补全会给出一串不存在的键
 */

/** 一行里的键（`  server:`、`port: 8080` 的 `port`）；不是写键的行给 null */
function keyOf(line: string): { indent: number; key: string } | null {
  const m = /^(\s*)([A-Za-z0-9_.\-[\]]+)\s*:(\s|$)/.exec(line);
  if (!m) return null;
  return { indent: m[1].length, key: m[2] };
}

/** 第 `idx` 行（从 0 数）的父键（`["spring", "datasource"]`）：往上找缩进比它少的写键的行，一层层接起来。遇到 `---` 停（新文档） */
export function yamlParents(lines: readonly string[], idx: number): string[] {
  const cur = lines[idx] ?? "";
  let indent = /^\s*/.exec(cur)![0].length;
  const out: string[] = [];
  for (let i = idx - 1; i >= 0 && indent > 0; i--) {
    const l = lines[i];
    if (/^---/.test(l)) break;
    if (!l.trim() || /^\s*#/.test(l)) continue;
    const lead = /^\s*/.exec(l)![0].length;
    if (lead >= indent) continue;
    // 比它浅的那一行：是列表项或者多行字面量，就不是我们认的结构了
    if (/^\s*-\s/.test(l) || /:\s*[|>][-+]?\s*$/.test(l)) return [];
    const k = keyOf(l);
    if (!k) return [];
    out.unshift(k.key);
    indent = lead;
  }
  return out;
}

/** 光标所在那一行的整个键名（父键 + 这一行的键），给悬停用。这一行不是写键的行给 null */
export function yamlKeyAt(lines: readonly string[], idx: number): string | null {
  const k = keyOf(lines[idx] ?? "");
  if (!k) return null;
  return [...yamlParents(lines, idx), k.key].join(".");
}

/**
 * 补全的位置：在写键（`before` 是缩进 + 半截键名）还是写值（`键: 半截值`）。
 * `before` 是这一行光标前面那一段
 */
export function yamlSpot(before: string): { kind: "key"; typed: string } | { kind: "value"; key: string; typed: string } | null {
  const k = /^\s*([A-Za-z0-9_.\-[\]]*)$/.exec(before);
  if (k) return { kind: "key", typed: k[1] };
  const v = /^\s*([A-Za-z0-9_.\-[\]]+)\s*:\s+([^#\s]*)$/.exec(before);
  if (v) return { kind: "value", key: v[1], typed: v[2] };
  return null;
}

/** `.properties`：同上（`键=值` 或 `键: 值`；`#` `!` 开头是注释） */
export function propsSpot(before: string): { kind: "key"; typed: string } | { kind: "value"; key: string; typed: string } | null {
  if (/^\s*[#!]/.test(before)) return null;
  const k = /^\s*([A-Za-z0-9_.\-[\]]*)$/.exec(before);
  if (k) return { kind: "key", typed: k[1] };
  const v = /^\s*([A-Za-z0-9_.\-[\]]+)\s*[=:]\s*(\S*)$/.exec(before);
  if (v) return { kind: "value", key: v[1], typed: v[2] };
  return null;
}

/** `.properties` 一行的键（悬停用） */
export function propsKeyAt(line: string): string | null {
  if (/^\s*[#!]/.test(line)) return null;
  return /^\s*([A-Za-z0-9_.\-[\]]+)\s*[=:]/.exec(line)?.[1] ?? null;
}

/**
 * 配置键的「宽松绑定」（Spring 的 relaxed binding）：`server.servlet.context-path`、`server.servlet.contextPath`、`SERVER_SERVLET_CONTEXT_PATH`
 * 是同一个。比较前都化成小写去掉 `-` `_` —— 你写的是驼峰也认得出对应的元数据键（同 Rust 的 `spring::norm_key`）
 */
export const normKey = (k: string) => k.toLowerCase().replace(/[-_]/g, "");
