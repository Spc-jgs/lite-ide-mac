/**
 * 桩：AI 助手（docs/TAKEOVER.md MVP-6），对应 `src-tauri/src/commands/agent.rs`（规则在 `src-tauri/src/agent.rs`，线在 `crates/acpsvc`）。
 *
 * 一轮照真 agent 的次序走、**故意走一阵**（frontend.md「桩要能停在中间一会儿」）：想法 → 计划 → 读一个文件（问编辑器要没保存的那份）→
 * 一块块说 → 要权限改它（等人点）→ 写进编辑器 → 新建一个文件 → 说完。`agent_cancel` 之后在下一步停、回 cancelled；等着的权限由前端回 cancelled。
 * 起会话等 0.9 秒（真的第一次要下适配器，几十秒）。
 *
 * 和 scripts/fake-acp-agent.py（测试通道里冒充真 agent 的那个）讲的是同一个剧本：一个在浏览器里验界面，一个过真的 ACP 线验 Rust 那半
 */
import { type A, FILES, NOT_MINE } from "./data";
import { mockEmit } from "./events";

interface Sess {
  root: string;
  cancelled: boolean;
  /** 等前端回话的反向请求：req → 交回 result 的 */
  waits: Map<string, (r: unknown) => void>;
}
const SESS = new Map<number, Sess>();
let next = 0;
let reqs = 0;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const emit = (id: number, kind: string, data: unknown) => mockEmit("agent-event", { id, kind, data });
const update = (id: number, u: Record<string, unknown>) => emit(id, "update", u);

/** 问前端、等它回（权限、读文件、写文件） */
function ask(id: number, s: Sess, kind: string, data: Record<string, unknown>): Promise<unknown> {
  const req = `r${++reqs}`;
  return new Promise((done) => {
    s.waits.set(req, done);
    emit(id, kind, { req, ...data });
  });
}

async function turn(id: number, s: Sess, prompt: { text?: string }[]): Promise<string> {
  const said = prompt.map((b) => b.text ?? "").join("\n");
  const target = Object.keys(FILES).find((k) => k.startsWith(`${s.root}/src/`) && /\.(ts|java|rs)$/.test(k)) ?? `${s.root}/README.md`;
  const name = target.slice(s.root.length + 1);
  const step = async (ms: number) => {
    await sleep(ms);
    return s.cancelled;
  };
  update(id, { sessionUpdate: "agent_thought_chunk", content: { type: "text", text: `用户问的是「${said.slice(0, 40)}」。先看看 ${name}。` } });
  if (await step(300)) return "cancelled";
  update(id, { sessionUpdate: "plan", entries: [{ content: `读 ${name}`, status: "in_progress", priority: "high" }, { content: "改一处", status: "pending", priority: "medium" }, { content: "补一份说明", status: "pending", priority: "low" }] });
  update(id, { sessionUpdate: "tool_call", toolCallId: "t1", title: `Read ${name}`, kind: "read", status: "in_progress", locations: [{ path: target }] });
  const live = (await ask(id, s, "read", { path: target })) as { content?: string | null };
  const text = live?.content ?? FILES[target] ?? "";
  update(id, { sessionUpdate: "tool_call_update", toolCallId: "t1", status: "completed", content: [{ type: "content", content: { type: "text", text: `${text.split("\n").length} 行` } }] });
  if (await step(250)) return "cancelled";
  for (const w of [`${name} 一共 ${text.split("\n").length} 行。`, "我在开头加一行注释，", "再新建一份说明：\n\n```\nNOTES.md\n```\n"]) {
    update(id, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: w } });
    if (await step(180)) return "cancelled";
  }
  update(id, { sessionUpdate: "plan", entries: [{ content: `读 ${name}`, status: "completed", priority: "high" }, { content: "改一处", status: "in_progress", priority: "medium" }, { content: "补一份说明", status: "pending", priority: "low" }] });
  update(id, { sessionUpdate: "tool_call", toolCallId: "t2", title: `Edit ${name}`, kind: "edit", status: "pending", locations: [{ path: target }] });
  const perm = (await ask(id, s, "permission", {
    params: {
      toolCall: { toolCallId: "t2", title: `Edit ${name}` },
      options: [
        { optionId: "allow", name: "Allow", kind: "allow_once" },
        { optionId: "always", name: "Always Allow", kind: "allow_always" },
        { optionId: "reject", name: "Reject", kind: "reject_once" },
      ],
    },
  })) as { outcome?: { outcome?: string; optionId?: string } };
  if (s.cancelled || perm?.outcome?.outcome === "cancelled") return "cancelled";
  if (perm?.outcome?.optionId === "reject") {
    update(id, { sessionUpdate: "tool_call_update", toolCallId: "t2", status: "failed" });
    update(id, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "好，不改了。" } });
    return "end_turn";
  }
  const lines = text.split("\n");
  lines.splice(1, 0, "// 桩 agent 加的一行");
  await ask(id, s, "write", { path: target, content: lines.join("\n") });
  update(id, { sessionUpdate: "tool_call_update", toolCallId: "t2", status: "completed" });
  update(id, { sessionUpdate: "tool_call", toolCallId: "t3", title: "Write NOTES.md", kind: "edit", status: "in_progress", locations: [{ path: `${s.root}/NOTES.md` }] });
  await ask(id, s, "write", { path: `${s.root}/NOTES.md`, content: `# 说明\n\n${name} 开头加了一行注释。\n` });
  update(id, { sessionUpdate: "tool_call_update", toolCallId: "t3", status: "completed" });
  update(id, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "\n改好了，两处都在编辑器里、还没存盘，去审阅。" } });
  return "end_turn";
}

export function agentCmd(cmd: string, a: A): unknown {
  switch (cmd) {
    case "agent_start":
      return sleep(900).then(() => {
        const id = ++next;
        SESS.set(id, { root: String(a.root), cancelled: false, waits: new Map() });
        return {
          id,
          sessionId: `mock-${id}`,
          name: String(a.kind) === "codex" ? "Codex（桩）" : "Claude Code（桩）",
          version: "0.0.0",
          modes: { currentModeId: "default", availableModes: [{ id: "default", name: "Default" }, { id: "acceptEdits", name: "Accept Edits" }, { id: "plan", name: "Plan Mode" }] },
        };
      });
    case "agent_prompt": {
      const s = SESS.get(Number(a.id));
      if (!s) return Promise.reject("这个 AI 会话已经结束了");
      s.cancelled = false;
      return turn(Number(a.id), s, a.prompt as unknown as { text?: string }[]);
    }
    case "agent_cancel": {
      const s = SESS.get(Number(a.id));
      if (s) s.cancelled = true;
      return null;
    }
    case "agent_set_mode":
      return null;
    case "agent_reply": {
      const s = SESS.get(Number(a.id));
      const done = s?.waits.get(String(a.req));
      if (done) {
        s!.waits.delete(String(a.req));
        done(a.error ? null : a.result);
      }
      return null;
    }
    case "agent_stop":
      SESS.delete(Number(a.id));
      return null;
    default:
      return NOT_MINE;
  }
}
