/** AI 助手（docs/TAKEOVER.md MVP-6）的命令包装：只有 AI 面板用，它是懒的（同 ipc/debug.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { AgentStarted } from "./commands";

export type AgentKind = "claude" | "codex";

/** 起 agent、在项目根里建一个会话。第一次要从 npm 下适配器（几十秒） */
export const agentStart = (root: string, kind: AgentKind) => invoke<AgentStarted>("agent_start", { root, kind });

/** 发一轮话（ACP 的内容块数组），等到这一轮说完，回 stopReason。过程走 `agent-event` */
export const agentPrompt = (id: number, prompt: unknown[]) => invoke<string>("agent_prompt", { id, prompt });

export const agentCancel = (id: number) => invoke<void>("agent_cancel", { id });

export const agentSetMode = (id: number, mode: string) => invoke<void>("agent_set_mode", { id, mode });

/** 回 agent 问的那个请求（`req` 原样带回）。读文件那种的 `result` 是 `{ content: 编辑器里的文本 | null }` */
export const agentReply = (id: number, req: unknown, result: unknown, error?: string) => invoke<void>("agent_reply", { id, req, result, error: error ?? null });

export const agentStop = (id: number) => invoke<void>("agent_stop", { id });

/** `agent-event` 的负载 */
export interface AgentEvent {
  id: number;
  kind: "update" | "permission" | "read" | "write" | "exit";
  data: Record<string, unknown>;
}
