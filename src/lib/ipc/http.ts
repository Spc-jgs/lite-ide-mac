/** HTTP 请求（docs/IDEA.md 第 19 节）的命令包装：只有 `.http` 文件和运行窗里那几块用，它们是懒的（同 ipc/tasks.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { HttpCookie, HttpEnvs, HttpPart, HttpResponse } from "./commands";

/** 这个 `.http` 文件能用的环境（从它所在的目录往上到项目根找环境文件） */
export const httpEnvs = (root: string, path: string) => invoke<HttpEnvs>("http_envs", { root, path });

export interface HttpSend {
  method: string;
  url: string;
  headers: [string, string][];
  body: string | null;
  /** `< ./file.json`：绝对路径 */
  bodyFile: string | null;
  follow: boolean;
  /** 选的环境：Cookie 罐按「项目 + 环境」一个 */
  env: string | null;
  /** false = `# @no-cookie-jar`：不带也不记 */
  cookies: boolean;
  /** multipart（#77）：请求体按段拼（有它时 `body` 是 null） */
  parts: HttpPart[] | null;
}

/** 发一个变量已经替换好的请求。`op`：前端发的操作号，取消要用（发之前就定下来）；`root`：`bodyFile` 只许在它底下 */
export const httpSend = (op: number, root: string, r: HttpSend) => invoke<HttpResponse>("http_send", { op, root, ...r });

export const httpCancel = (op: number) => invoke<void>("http_cancel", { op });

/** 这个项目、这个环境记着的 Cookie（#75） */
export const httpCookies = (root: string, env: string | null) => invoke<HttpCookie[]>("http_cookies", { root, env });

export const httpCookiesClear = (root: string, env: string | null) => invoke<void>("http_cookies_clear", { root, env });
