/**
 * 发 `.http` 里的请求（docs/IDEA.md 第 19 节）：找到光标所在的请求、选中的环境、替换变量、在运行窗开一格、交给 Rust（curl）、回来填上。懒加载 ——
 * `.http` 文件的编辑器扩展（editor/http-run.ts）和运行窗动态引它，入口里一个字节不多。
 *
 * 和 IDEA 的分法一样：每次发是运行窗里的一格（同名的原地换掉，同任务），格子里是响应（状态、耗时、响应体、响应头）；
 * 「环境」按项目记一个（IDEA 是编辑器顶上的「Run with」），存 localStorage。
 */
import { runs, type RunTab } from "../state/runs.svelte";
import { layout } from "../state/layout.svelte";
import { notify } from "../state/notify.svelte";
import { tabs } from "../state/tabs.svelte";
import { docs } from "../state/docs.svelte";
import { readText } from "../ipc/commands";
import { httpCancel, httpEnvs, httpSend } from "../ipc/http";
import { parseHttp, requestAt, resolve, tabName, type HttpFile, type HttpReq } from "./http-file";

const KEY = "lite-ide.http-env";
const subs = new Set<() => void>();

function table(): Record<string, string> {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return v && typeof v === "object" ? v : {};
  } catch {
    return {};
  }
}

/** 这个项目选的环境（没选是 null） */
export const envOf = (root: string): string | null => table()[root] ?? null;

export function setEnv(root: string, name: string | null) {
  const t = table();
  if (name) t[root] = name;
  else delete t[root];
  try {
    localStorage.setItem(KEY, JSON.stringify(t));
  } catch {
    /* 存不下：只在这次运行里有 */
  }
  for (const f of subs) f();
}

export function onEnv(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}

/**
 * 运行窗那一格的 id：从二十亿起，不和 Rust 的任务 id（从 1 数）撞 —— 两种格子在同一张表里（`runs.list`），关、停、重跑按 `http` 字段分开走
 */
let nextTab = 2_000_000_000;
/** 发给 Rust 的操作号（取消用）：发之前定下来（rust.md「操作 id 由谁发」） */
let nextOp = 1;

const tab = (id: number) => runs.list.find((t) => t.id === id);

/** `< ./body.json` 按 `.http` 所在目录解析成绝对路径（`..` 也认） */
export function absPath(httpPath: string, rel: string): string {
  if (rel.startsWith("/")) return rel;
  const parts = httpPath.split("/").slice(0, -1);
  for (const seg of rel.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") parts.pop();
    else parts.push(seg);
  }
  return parts.join("/");
}

/** 发光标所在的那个（⌃⇧R、行号旁的 ▶）。`text` 是编辑器里此刻的文本：没保存的改动也算（同 IDEA） */
export async function sendAt(root: string, path: string, text: string, line: number) {
  const f = parseHttp(text);
  const req = requestAt(f, line);
  if (!req) {
    notify.hint("这个文件里没有请求（请求行是「方法 URL」，比如 GET http://localhost:8080/api）");
    return;
  }
  await send(root, path, f, req);
}

async function send(root: string, path: string, f: HttpFile, req: HttpReq) {
  let envs: Record<string, unknown> = {};
  let names: string[] = [];
  try {
    const e = await httpEnvs(root, path);
    envs = JSON.parse(e.json);
    names = e.names;
    if (e.problems.length) notify.fail(`环境文件读坏了：${e.problems[0]}`, 8000);
  } catch (e) {
    notify.fail(`读不了环境文件：${e instanceof Error ? e.message : String(e)}`);
  }
  let env = envOf(root);
  if (env && !(env in envs)) env = null;
  const r = resolve(req, f.vars, env ? envs[env] : null);
  if (!r.ok) {
    const why = env ? `（环境「${env}」里没有）` : names.length ? "（还没选环境：编辑器顶上那一条里选）" : "（没有 http-client.env.json，也没在文件里用 @名字 = 值 定义）";
    notify.fail(`没定义的变量：${r.missing.map((m) => `{{${m}}}`).join("、")}${why}，没发`, 8000);
    return;
  }
  const name = tabName(req);
  const id = nextTab++;
  const op = nextOp++;
  const run: RunTab = {
    id,
    root,
    name,
    command: `${r.req.method} ${r.req.url}`,
    log: "",
    status: "running",
    code: null,
    holder: null,
    service: false,
    startedAt: Date.now(),
    tests: null,
    debug: null,
    http: { op, path, req, env, method: r.req.method, url: r.req.url, resp: null, error: "" },
  };
  // 同名的那一格原地换掉（位置不跳），同任务；还在发的先取消
  const i = runs.list.findIndex((t) => t.root === root && t.http && t.name === name);
  if (i >= 0) {
    const old = runs.list[i];
    if (old.status === "running" && old.http) void httpCancel(old.http.op).catch(() => {});
    runs.list[i] = run;
  } else runs.list = [...runs.list, run];
  runs.activeId = id;
  layout.panelView = "run";
  layout.panel = true;
  try {
    const resp = await httpSend(op, {
      method: r.req.method,
      url: r.req.url,
      headers: r.req.headers,
      body: r.req.body,
      bodyFile: req.bodyFile ? absPath(path, req.bodyFile) : null,
      follow: req.follow,
    });
    const now = tab(id);
    if (!now?.http) return;
    now.http.resp = resp;
    now.log = resp.body;
    now.code = resp.status;
    // 4xx / 5xx 格子上的点是红的（同任务「失败」）：一排请求里一眼看出哪个没成
    now.status = resp.status >= 400 ? "failed" : "done";
  } catch (e) {
    const now = tab(id);
    if (!now?.http) return;
    const msg = e instanceof Error ? e.message : String(e);
    now.http.error = msg;
    now.status = msg === "已取消" ? "stopped" : "failed";
  }
}

/** 取消正在发的（运行窗的停止按钮、⌘F2、关掉那一格） */
export function cancel(id: number) {
  const r = tab(id);
  if (r?.http && r.status === "running") void httpCancel(r.http.op).catch(() => {});
}

/**
 * 重发（运行窗的重跑按钮）：照文件**此刻**的样子再发一次 —— 改了请求体再点重发，发的是改过的（同 IDEA 重跑运行配置）。
 * 按名字找回那个请求（没名字按方法 + 原来的 URL 写法），找不到（删了、改名了）就发上次那份
 */
export async function resend(id: number) {
  const r = tab(id);
  if (!r?.http) return;
  const { path, req } = r.http;
  const open = tabs.byPath(path);
  let text: string | null = open ? docs.liveText(open) : null;
  if (text === null) text = await readText(path).then((t) => t.content).catch(() => null);
  const f = text === null ? { vars: {}, requests: [] } : parseHttp(text);
  const again = f.requests.find((x) => (req.name ? x.name === req.name : x.method === req.method && x.url === req.url)) ?? req;
  await send(r.root, path, f, again);
}
