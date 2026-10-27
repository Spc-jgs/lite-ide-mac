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
 * 请求体可以是 `< ./body.json`（从文件读）。`# @no-cookie-jar`：这个请求不带、也不记 Cookie（#75）。
 *
 * 响应处理脚本（`> {% … %}`）**不当 JavaScript 跑**：那是在我们进程里执行文件里带来的代码（#76）。摘出来交给 `http-script.ts`，
 * 它只认「登录拿 token」那一种写法（`client.global.set("token", response.body.token)`），别的语句不执行、说清楚。`> handler.js` 那种外面的文件不认。
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
  /** `# @no-cookie-jar` 的是 false */
  cookies: boolean;
  /** 写了响应处理脚本（`> {% %}` 或 `> 文件`） */
  handler: boolean;
  /** `> {% … %}` 里的文本（几段的话接起来）；没有是 null */
  script: string | null;
  /** `> handler.js`：外面的脚本文件（不认，要说一句） */
  scriptFile: string | null;
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
    let cookies = true;
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
        if (tag?.[1] === "no-cookie-jar") cookies = false;
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
      req = { line: j + 1, name, method: m ? m[1] : "GET", url: m ? m[2] : l.trim(), headers: [], body: null, bodyFile: null, follow, cookies, handler: false, script: null, scriptFile: null };
      j++;
      break;
    }
    if (req) {
      // URL 接续行：缩进的、以 ? 或 & 开头
      while (j < end && /^\s+[?&]/.test(lines[j])) req.url += lines[j++].trim();
      // 头：到空行为止（中间的注释跳过）。`>` 开头的是响应处理脚本：GET 这种没有体的，IDEA 许它紧跟在请求行 / 头下面、不空一行
      let toScript = false;
      for (; j < end && lines[j].trim(); j++) {
        if (/^>\s*\S/.test(lines[j])) {
          toScript = true;
          break;
        }
        if (COMMENT.test(lines[j])) continue;
        const h = /^([^:\s]+)\s*:\s*(.*?)\s*$/.exec(lines[j]);
        if (h) req.headers.push([h[1], h[2]]);
      }
      // 体：空行之后到块尾。`< 文件`、响应处理脚本、`<>` 上次的响应 都摘出来
      const body: string[] = [];
      const script: string[] = [];
      let inScript = false;
      for (j = toScript ? j : j + 1; j < end; j++) {
        const l = lines[j];
        if (inScript) {
          const k = l.indexOf("%}");
          script.push(k < 0 ? l : l.slice(0, k));
          if (k >= 0) inScript = false;
          continue;
        }
        if (/^>\s*\{%/.test(l)) {
          req.handler = true;
          const rest = l.replace(/^>\s*\{%/, "");
          const k = rest.indexOf("%}");
          script.push(k < 0 ? rest : rest.slice(0, k));
          inScript = k < 0;
          continue;
        }
        if (/^>\s*\S/.test(l)) {
          req.handler = true;
          req.scriptFile = l.replace(/^>\s*/, "").trim();
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
      if (script.some((x) => x.trim())) req.script = script.join("\n").trim();
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
 * 把 `{{名字}}` 换掉：先动态变量，再文件里的 `@变量`（它的值里也能引用环境里的，展开一层），再响应脚本存下的（`client.global.set`，`globals`），最后选中的环境。
 * 脚本存的排在环境前面：环境文件里写着一个过期的 `token`，登录之后拿到的新的要能盖过它
 * 有没定义的就**不发**，把名字全列出来 —— 发一个带着 `{{host}}` 的请求出去，回来的错只会让人更糊涂
 */
export function resolve(req: HttpReq, fileVars: Record<string, string>, env: unknown, globals: Record<string, string> = {}): { ok: true; req: Resolved } | { ok: false; missing: string[] } {
  const missing = new Set<string>();
  const one = (name: string, depth: number): string | undefined => {
    if (name.startsWith("$")) return dynamic(name);
    if (name in fileVars) return depth > 0 ? fileVars[name] : sub(fileVars[name], depth + 1);
    if (Object.hasOwn(globals, name)) return globals[name];
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

/**
 * multipart（#77）：Content-Type 是 `multipart/*` 时，请求体按行切成「文字段 + 文件」交给 Rust 拼。
 * - 行与行之间是 **CRLF**（RFC 7578；Tomcat / commons-fileupload 的解析器找的是 `\r\n--边界`，只有 LF 的整个请求体认不出一段）。
 *   文件里写的是 LF，发出去之前换掉 —— IDEA 也是这么发的
 * - `< ./logo.png` 那一行换成那个文件的全部字节（`abs` 按 `.http` 所在目录解析路径）。文件内容不转换换行（图片、压缩包原样）
 * 返回 null = 不是 multipart，照原来的走
 */
export function multipartParts(contentType: string, body: string | null, abs: (rel: string) => string): { text: string | null; file: string | null }[] | null {
  if (body === null || !/^\s*multipart\//i.test(contentType)) return null;
  const out: { text: string | null; file: string | null }[] = [];
  let text = "";
  body.split("\n").forEach((raw, i) => {
    const l = raw.replace(/\r$/, "");
    if (i > 0) text += "\r\n";
    const f = /^<\s+(\S.*?)\s*$/.exec(l);
    if (!f) {
      text += l;
      return;
    }
    if (text) out.push({ text, file: null });
    out.push({ text: null, file: abs(f[1]) });
    text = "";
  });
  // 收尾的边界行（`--边界--`）后面补一个 CRLF：请求体以行尾结束，和别的行一个样子
  if (!text.endsWith("\r\n")) text += "\r\n";
  out.push({ text, file: null });
  return out;
}

/** 运行窗那一格叫什么：有名字用名字，没有就是「方法 + 路径」（不带主机：一排格子里主机都一样） */
export function tabName(req: HttpReq): string {
  if (req.name) return req.name;
  const path = req.url.replace(/^[a-z]+:\/\/[^/]*/i, "").replace(/^\{\{[^}]*\}\}/, "") || "/";
  return `${req.method} ${path.length > 40 ? `${path.slice(0, 39)}…` : path}`;
}
