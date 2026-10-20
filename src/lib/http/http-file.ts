/**
 * `.http` 文件（IDEA 的 HTTP Client 格式，docs/IDEA.md 第 19 节）里不碰编辑器、不碰 IPC 的那一半：切出每个请求、替换变量。
 * 拆出来是为了在裸 node 里测（tests/http-file.test.ts）—— 行号旁的 ▶ 和 ⌃⇧R 在 editor/http-run.ts，发请求在 http-ops.ts。
 *
 * 认的写法（照 IDEA 的文档）：
 *
 * ```http
 * @host = localhost:8080              ← 文件里的变量（写在请求行上面，整个文件都能用）
 *
 * ### 下单                            ← `###` 分开请求，后面的字是名字
 * # @name createOrder                ← 也能这样起名字；`# @no-redirect` 不跟重定向
 * POST http://{{host}}/api/orders?debug=true
 *     &trace=1                       ← 缩进的 `?` / `&` 行接在 URL 后面
 * Content-Type: application/json
 *
 * { "sku": "{{sku}}", "id": "{{$uuid}}" }
 * ```
 *
 * 请求体可以是 `< ./body.json`（从文件读）。响应处理脚本（`> {% … %}`、`> handler.js`）**这一版不跑**：
 * 那是在我们进程里执行文件里带来的 JavaScript —— 认出来、从请求体里摘掉、在运行窗里说一句。
 */

export interface HttpReq {
  /** 请求行在第几行（从 1 数）：▶ 画在这一行 */
  line: number;
  name: string | null;
  method: string;
  url: string;
  headers: [string, string][];
  body: string | null;
  /** `< ./file`：相对 `.http` 所在目录的路径（原样，发的时候再解析） */
  bodyFile: string | null;
  follow: boolean;
  /** 写了响应处理脚本（没跑） */
  handler: boolean;
}

export interface HttpFile {
  /** `@name = value` */
  vars: Record<string, string>;
  requests: HttpReq[];
}

const METHODS = /^(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS|TRACE|CONNECT)\s+(\S.*?)(?:\s+HTTP\/[\d.]+)?\s*$/;
const COMMENT = /^\s*(#|\/\/)/;
const FILE_VAR = /^@([A-Za-z_][\w.-]*)\s*=\s*(.*?)\s*$/;

export function parseHttp(text: string): HttpFile {
  const lines = text.split("\n").map((l) => l.replace(/\r$/, ""));
  const vars: Record<string, string> = {};
  const requests: HttpReq[] = [];
  let i = 0;
  while (i < lines.length) {
    // 一块：到下一个 `###` 为止
    let end = i + 1;
    while (end < lines.length && !lines[end].startsWith("###")) end++;
    let name: string | null = null;
    let follow = true;
    let j = i;
    if (lines[j].startsWith("###")) {
      name = lines[j].replace(/^#+/, "").trim() || null;
      j++;
    }
    // 请求行之前：空行、注释（`# @name` / `# @no-redirect`）、文件变量
    let req: HttpReq | null = null;
    for (; j < end; j++) {
      const l = lines[j];
      if (!l.trim()) continue;
      if (COMMENT.test(l)) {
        const tag = /^\s*(?:#|\/\/)\s*@([\w-]+)\s*(.*)$/.exec(l);
        if (tag?.[1] === "name" && tag[2].trim()) name = tag[2].trim();
        if (tag?.[1] === "no-redirect") follow = false;
        continue;
      }
      const fv = FILE_VAR.exec(l);
      if (fv) {
        vars[fv[1]] = fv[2];
        continue;
      }
      const m = METHODS.exec(l.trim());
      const bare = /^(https?:\/\/|\{\{)\S*$/.test(l.trim());
      if (!m && !bare) break; // 认不出的行：这一块不是请求（散文、半截的），不猜
      req = { line: j + 1, name, method: m ? m[1] : "GET", url: m ? m[2] : l.trim(), headers: [], body: null, bodyFile: null, follow, handler: false };
      j++;
      break;
    }
    if (req) {
      // URL 接续行：缩进的、以 ? 或 & 开头
      while (j < end && /^\s+[?&]/.test(lines[j])) req.url += lines[j++].trim();
      // 头：到空行为止（中间的注释跳过）
      for (; j < end && lines[j].trim(); j++) {
        if (COMMENT.test(lines[j])) continue;
        const h = /^([^:\s]+)\s*:\s*(.*?)\s*$/.exec(lines[j]);
        if (h) req.headers.push([h[1], h[2]]);
      }
      // 体：空行之后到块尾。`< 文件`、响应处理脚本、`<>` 上次的响应 都摘出来
      const body: string[] = [];
      let inScript = false;
      for (j++; j < end; j++) {
        const l = lines[j];
        if (inScript) {
          if (l.includes("%}")) inScript = false;
          continue;
        }
        if (/^>\s*\{%/.test(l)) {
          req.handler = true;
          inScript = !l.includes("%}");
          continue;
        }
        if (/^>\s*\S/.test(l)) {
          req.handler = true;
          continue;
        }
        if (/^<>\s/.test(l)) continue;
        const f = /^<\s+(\S.*?)\s*$/.exec(l);
        if (f && !body.some((b) => b.trim())) {
          req.bodyFile = f[1];
          continue;
        }
        body.push(l);
      }
      while (body.length && !body[body.length - 1].trim()) body.pop();
      while (body.length && !body[0].trim()) body.shift();
      if (body.length && !req.bodyFile) req.body = body.join("\n");
      requests.push(req);
    }
    i = end;
  }
  return { vars, requests };
}

/** 光标在第 `line` 行时要发的那个：它所在的那一块的请求（光标在两个请求之间的空行上，算上面那个） */
export function requestAt(f: HttpFile, line: number): HttpReq | null {
  let hit: HttpReq | null = null;
  for (const r of f.requests) if (r.line <= line) hit = r;
  // 光标在第一个请求的请求行上面（注释、`###` 那几行）：就是第一个
  return hit ?? f.requests[0] ?? null;
}

// ── 变量 ──

/** `{{a.b}}` 按路径从环境里取（IDEA 2024.2 起这么取）；值是对象 / 数组的给 JSON 文本，数字布尔给字符串 */
function lookup(env: unknown, path: string): string | undefined {
  let cur: unknown = env;
  for (const k of path.split(".")) {
    if (cur === null || typeof cur !== "object" || !(k in (cur as object))) return undefined;
    cur = (cur as Record<string, unknown>)[k];
  }
  if (cur === undefined || cur === null) return cur === null ? "null" : undefined;
  return typeof cur === "object" ? JSON.stringify(cur) : String(cur);
}

const ALNUM = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const pick = (chars: string, n: number) => Array.from({ length: Math.max(0, Math.min(n, 4096)) }, () => chars[Math.floor(Math.random() * chars.length)]).join("");

/** 动态变量（`{{$uuid}}` 这些）。认不出的给 undefined —— 和没定义的变量一样报出来，不悄悄发一个 `{{$xxx}}` 出去 */
export function dynamic(name: string, now = new Date()): string | undefined {
  if (name === "$uuid" || name === "$random.uuid") return crypto.randomUUID();
  if (name === "$timestamp") return String(Math.floor(now.getTime() / 1000));
  if (name === "$isoTimestamp") return now.toISOString();
  if (name === "$randomInt") return String(Math.floor(Math.random() * 1001));
  if (name === "$random.email") return `${pick(ALNUM.slice(26, 52), 8)}@example.com`;
  const m = /^\$random\.(integer|float|alphabetic|alphanumeric|hexadecimal)\((.*)\)$/.exec(name);
  if (!m) return undefined;
  const args = m[2].split(",").map((s) => Number(s.trim()));
  if (args.some((n) => Number.isNaN(n))) return undefined;
  const [a, b] = args;
  switch (m[1]) {
    case "integer":
      return String(Math.floor(a + Math.random() * (b - a)));
    case "float":
      return String(a + Math.random() * (b - a));
    case "alphabetic":
      return pick(ALNUM.slice(0, 52), a);
    case "alphanumeric":
      return pick(ALNUM, a);
    default:
      return pick("0123456789abcdef", a);
  }
}

export interface Resolved {
  method: string;
  url: string;
  headers: [string, string][];
  body: string | null;
}

/**
 * 把 `{{名字}}` 换掉：先动态变量，再文件里的 `@变量`（它的值里也能引用环境里的，展开一层），再选中的环境。
 * 有没定义的就**不发**，把名字全列出来 —— 发一个带着 `{{host}}` 的请求出去，回来的错只会让人更糊涂
 */
export function resolve(req: HttpReq, fileVars: Record<string, string>, env: unknown): { ok: true; req: Resolved } | { ok: false; missing: string[] } {
  const missing = new Set<string>();
  const one = (name: string, depth: number): string | undefined => {
    if (name.startsWith("$")) return dynamic(name);
    if (name in fileVars) return depth > 0 ? fileVars[name] : sub(fileVars[name], depth + 1);
    return lookup(env, name);
  };
  const sub = (s: string, depth = 0): string =>
    s.replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (all, name: string) => {
      const v = one(name, depth);
      if (v === undefined) {
        missing.add(name);
        return all;
      }
      return v;
    });
  const out: Resolved = {
    method: req.method,
    url: sub(req.url),
    headers: req.headers.map(([k, v]) => [sub(k), sub(v)]),
    body: req.body === null ? null : sub(req.body),
  };
  return missing.size ? { ok: false, missing: [...missing] } : { ok: true, req: out };
}

/** 运行窗那一格叫什么：有名字用名字，没有就是「方法 + 路径」（不带主机：一排格子里主机都一样） */
export function tabName(req: HttpReq): string {
  if (req.name) return req.name;
  const path = req.url.replace(/^[a-z]+:\/\/[^/]*/i, "").replace(/^\{\{[^}]*\}\}/, "") || "/";
  return `${req.method} ${path.length > 40 ? `${path.slice(0, 39)}…` : path}`;
}
