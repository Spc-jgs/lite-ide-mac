/**
 * 桩：HTTP 请求（docs/IDEA.md 第 19 节），对应 `src-tauri/src/commands/http.rs`（规则在 `crates/httpsvc`）。
 *
 * 环境照真实现找：从 `.http` 所在目录往上到项目根，`http-client.env.json` / `.private.` 那两个文件，近的盖远的、私有的盖公开的、`$shared` 是底子。
 * 发请求**故意等 600ms**（frontend.md「桩要能停在中间一会儿」）：运行窗那一格的「正在发…」和取消按钮在 0ms 的桩上看不见。
 * 回什么按 URL 认：带 `refused` 的连不上、带 `/fail` 的 500、带 `/html` 的回 HTML，别的回一段 JSON（键故意不按字母排：验「原样的次序」）。
 */
import { type A, bump, FILES, NOT_MINE } from "./data";

const PENDING = new Map<number, { reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
let seq = 0;

function envs(root: string, path: string) {
  const dirs: string[] = [];
  let d = path.slice(0, path.lastIndexOf("/"));
  for (;;) {
    dirs.push(d);
    if (d === root || !d.startsWith(`${root}/`)) break;
    d = d.slice(0, d.lastIndexOf("/"));
  }
  const raw: Record<string, Record<string, unknown>> = {};
  const files: string[] = [];
  const problems: string[] = [];
  for (const dir of dirs.reverse())
    for (const name of ["http-client.env.json", "http-client.private.env.json"]) {
      const p = `${dir}/${name}`;
      if (FILES[p] === undefined) continue;
      files.push(p);
      try {
        for (const [env, vars] of Object.entries(JSON.parse(FILES[p]) as Record<string, Record<string, unknown>>)) raw[env] = { ...(raw[env] ?? {}), ...vars };
      } catch (e) {
        problems.push(`${p}：不是合法的 JSON（${e}）`);
      }
    }
  const shared = raw.$shared ?? {};
  delete raw.$shared;
  const out: Record<string, Record<string, unknown>> = {};
  for (const k of Object.keys(raw).sort()) out[k] = { ...shared, ...raw[k] };
  return { names: Object.keys(out), json: JSON.stringify(out), files, problems };
}

export function httpCmd(cmd: string, a: A): unknown {
  switch (cmd) {
    case "http_envs":
      return envs(String(a.root), String(a.path));
    case "http_cancel": {
      const p = PENDING.get(Number(a.op));
      if (p) {
        clearTimeout(p.timer);
        PENDING.delete(Number(a.op));
        p.reject(new Error("已取消"));
      }
      return null;
    }
    case "http_send": {
      const op = Number(a.op);
      const url = String(a.url);
      if (!/^https?:\/\//i.test(url)) throw new Error(`仅支持 http:// 和 https:// 请求：${url}`);
      // 同 httpsvc::body_file_inside：`< 文件` 只许在项目里（桩不解软链，只比路径）
      if (a.bodyFile && !String(a.bodyFile).startsWith(`${String(a.root)}/`)) throw new Error(`请求体的文件在项目外面（${String(a.bodyFile)}），没发：.http 是项目带来的文件，从项目外面读文件发出去可能把你的私钥、配置发给别人。要发就把文件放进项目里`);
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          PENDING.delete(op);
          if (url.includes("refused")) return reject(new Error("连不上（服务没起？端口不对？）：Failed to connect to localhost port 8080 after 0 ms: Couldn't connect to server"));
          const n = ++seq;
          const html = url.includes("/html");
          const status = url.includes("/fail") ? 500 : String(a.method) === "POST" ? 201 : 200;
          const body = html
            ? "<!doctype html>\n<html><body><h1>Whitelabel Error Page</h1></body></html>\n"
            : JSON.stringify({ zeta: "原样的次序", id: n, items: [{ sku: "A-1", qty: 2 }], echo: { method: a.method, url, body: a.body ?? null, bodyFile: a.bodyFile ?? null } }, null, 2);
          const path = `/mock-http/resp-${n}.${html ? "html" : "json"}`;
          FILES[path] = body;
          bump(path);
          resolve({
            version: "HTTP/1.1",
            status,
            reason: status === 500 ? "Internal Server Error" : status === 201 ? "Created" : "OK",
            headers: [
              { name: "Content-Type", value: html ? "text/html;charset=UTF-8" : "application/json" },
              { name: "X-Trace-Id", value: `trace-${n}` },
              { name: "Date", value: "Mon, 20 Oct 2026 09:18:20 GMT" },
            ],
            ms: 35 + n,
            bytes: new TextEncoder().encode(body).length,
            body: path,
            truncated: false,
            pretty: !html,
            redirects: 0,
            finalUrl: url,
          });
        }, 600);
        PENDING.set(op, { reject, timer });
      });
    }
    default:
      return NOT_MINE;
  }
}
