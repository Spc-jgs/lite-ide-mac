/**
 * `.http` 的响应处理脚本（`> {% … %}`，#76）的一个小子集。**不执行 JavaScript**：
 * `.http` 文件是项目带来的（别人仓库里的），在我们的页面里执行它就等于把这台机器交出去（no-html-sinks 那条防线防的就是这个）。
 * 装一个沙箱里的 JS 引擎要么进程外再起一个运行时，要么进入口包 —— 为一个「登录拿 token」不值。
 *
 * 只认一种语句（IDEA 文档里最常见的那个用法）：
 *
 * ```js
 * client.global.set("token", response.body.data.token);   // 响应体（JSON）里按路径取，a.b[0]["c-d"] 都认
 * client.global.set("trace", response.headers.valueOf("X-Trace-Id"));
 * client.global.set("tenant", "acme");                    // 字面量
 * ```
 *
 * 别的语句（`client.test`、`if`、`console.log`……）一律不执行，原样列出来告诉人「这几句没跑」—— 悄悄跳过会让人以为断言过了。
 * 取不到的（不是 JSON、路径不存在、没有那个头）记成错误，那个变量不设：设一个 `undefined` 进去，下一个请求发出去的是字面上的 "undefined"。
 */

export interface ScriptResult {
  /** 设上的变量，按语句次序 */
  set: [string, string][];
  /** 没执行的语句（原文） */
  skipped: string[];
  /** 认得、但取不到值的 */
  errors: string[];
}

export interface ScriptResp {
  headers: { name: string; value: string }[];
  /** 响应体的文本（JSON 的话是排好的那份，解析结果一样） */
  body: string;
}

const SET = /^client\.global\.set\(\s*(["'])([A-Za-z_][\w.-]*)\1\s*,\s*([\s\S]+?)\s*\)$/;
const HEADER = /^response\.headers\.valueOf\(\s*(["'])(.+?)\1\s*\)$/;
const BODY = /^response\.body((?:\.[A-Za-z_$][\w$]*|\[\d+\]|\[(["'])[^"'\]]*\2\])*)$/;
const LITERAL = /^(["'])(.*)\1$/;

/**
 * 去掉注释（`//` 到行尾、`/* *\/`），按 `;` 和换行切成语句 —— **只在括号外、字符串外切**：
 * `client.test("…", function () { client.assert(…); })` 里的分号和换行按字面切，会切出半截的语句和一个孤零零的 `})`（真 .app 验收撞的）。
 * 字符串里的 `//`（URL）不当注释：只认前面是空白或行首的
 */
function statements(src: string): string[] {
  const text = src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|\s)\/\/.*$/gm, "$1");
  const out: string[] = [];
  let cur = "";
  let depth = 0;
  let quote = "";
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === "\\") cur += c + (text[++i] ?? "");
      else {
        if (c === quote) quote = "";
        cur += c;
      }
      continue;
    }
    if (c === '"' || c === "'" || c === "`") quote = c;
    else if ("([{".includes(c)) depth++;
    else if (")]}".includes(c)) depth = Math.max(0, depth - 1);
    else if (depth === 0 && (c === ";" || c === "\n")) {
      if (cur.trim()) out.push(cur.trim());
      cur = "";
      continue;
    }
    cur += c;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

/** `.a.b[0]["c-d"]` → ["a", "b", 0, "c-d"] */
function segments(path: string): (string | number)[] {
  const out: (string | number)[] = [];
  for (const m of path.matchAll(/\.([A-Za-z_$][\w$]*)|\[(\d+)\]|\[(["'])([^"'\]]*)\3\]/g)) out.push(m[1] ?? (m[2] !== undefined ? Number(m[2]) : m[4]));
  return out;
}

export function runScript(src: string, resp: ScriptResp): ScriptResult {
  const r: ScriptResult = { set: [], skipped: [], errors: [] };
  let json: { ok: true; v: unknown } | { ok: false } | null = null;
  const body = () => {
    if (!json) {
      try {
        json = { ok: true, v: JSON.parse(resp.body) };
      } catch {
        json = { ok: false };
      }
    }
    return json;
  };
  for (const st of statements(src)) {
    const m = SET.exec(st);
    if (!m) {
      r.skipped.push(st);
      continue;
    }
    const [, , name, expr] = m;
    let value: string | undefined;
    const lit = LITERAL.exec(expr);
    const hd = HEADER.exec(expr);
    const bd = BODY.exec(expr);
    if (lit) value = lit[2];
    else if (hd) {
      value = resp.headers.find((x) => x.name.toLowerCase() === hd[2].toLowerCase())?.value;
      if (value === undefined) r.errors.push(`「${name}」未设置：响应中没有 ${hd[2]} 头`);
    } else if (bd) {
      const b = body();
      if (!b.ok) {
        r.errors.push(`「${name}」未设置：响应体不是 JSON，无法读取 ${expr}`);
        continue;
      }
      let cur: unknown = b.v;
      for (const k of segments(bd[1])) {
        cur = cur !== null && typeof cur === "object" ? (cur as Record<string | number, unknown>)[k] : undefined;
        if (cur === undefined) break;
      }
      if (cur === undefined) r.errors.push(`「${name}」未设置：响应体中不存在 ${expr}`);
      else value = typeof cur === "object" && cur !== null ? JSON.stringify(cur) : String(cur);
    } else {
      // 认得是 set、但取值的写法不认（拼接、函数调用）：不执行，和别的语句一样列出来
      r.skipped.push(st);
      continue;
    }
    if (value !== undefined) r.set.push([name, value]);
  }
  return r;
}
