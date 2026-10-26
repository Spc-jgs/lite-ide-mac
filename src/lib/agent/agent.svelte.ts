/**
 * AI 助手的状态（docs/TAKEOVER.md MVP-6）：一个窗口一个会话。整个模块是懒的 —— 只有 AI 面板和编辑器右键「发给 AI」引它。
 *
 * agent 是外面的进程（Claude Code / Codex 经 ACP 适配器），这里管三件事：
 * - **对话**：发一轮、收它一块块推来的字 / 想法 / 工具调用 / 计划，拼成 `items`
 * - **它反过来问我们的**：权限（人点了才回，不超时）、读文件（编辑器里没保存的那份）、写文件（写进编辑器、不存盘）
 * - **审阅**：它写过的文件记在 `edits`，一个个「留下」（存盘）或「退回」（编辑器里换回改之前那份）
 *
 * 为什么 agent 的改动先进编辑器、不直接落盘（docs/DIRECTION.md）：人看得见、⌘Z 撤得回、和自己没保存的改动不打架；
 * 落盘之后再想退回，要么靠 git、要么靠本地历史，都比「还没存」难。
 */
import { tabs } from "../state/tabs.svelte";
import { docs } from "../state/docs.svelte";
import { project } from "../state/project.svelte";
import { notify } from "../state/notify.svelte";
import { layout } from "../state/layout.svelte";
import { listenHere, readText, trashEntry, writeText, type AgentStarted } from "../ipc/commands";
import { agentCancel, agentPrompt, agentReply, agentSetMode, agentStart, agentStop, type AgentEvent, type AgentKind } from "../ipc/agent";
import { viewOf } from "../editor/live-views";
import { lineChanges } from "../editor/doc-diff";

/** 选中的一段代码、带着位置（编辑器右键「发给 AI」） */
export interface Ctx {
  path: string;
  from: number;
  to: number;
  text: string;
}

export interface PermOption {
  optionId: string;
  name: string;
  kind: string;
}

export type Item =
  | { t: "user"; text: string; ctx: Ctx[] }
  | { t: "agent"; text: string }
  | { t: "thought"; text: string }
  | { t: "tool"; id: string; title: string; kind: string; status: string; path: string; out: string }
  | { t: "plan"; entries: { content: string; status: string }[] }
  | { t: "perm"; req: unknown; title: string; options: PermOption[]; chosen: string }
  | { t: "note"; text: string; bad: boolean };

/** agent 写过的一个文件。`before` 是它第一次写之前编辑器里的那份（null = 原来没有这个文件，它新建的） */
export interface Edit {
  path: string;
  before: string | null;
  after: string;
  state: "pending" | "kept" | "reverted";
}

const KIND_KEY = "lite.agent.kind";

/** 等编辑器挂上（刚打开的标签要一会儿才有视图），最多等 3 秒。同 history-ops */
async function waitView(path: string) {
  for (let i = 0; i < 60; i++) {
    const v = viewOf(path);
    if (v) return v;
    await new Promise((r) => setTimeout(r, 50));
  }
  return null;
}

/** 编辑器里此刻的文本：有视图读视图，有标签读标签（切走了的草稿），都没有就是 null（读盘那条由调用方走） */
function liveOf(path: string): string | null {
  const v = viewOf(path);
  if (v) return v.state.doc.toString();
  const t = tabs.byPath(path);
  return t && t.mode === "edit" ? docs.liveText(t) : null;
}

export const rel = (path: string) => {
  const r = project.root;
  return r && path.startsWith(`${r}/`) ? path.slice(r.length + 1) : path;
};

class AgentState {
  kind = $state<AgentKind | null>(null);
  session = $state<(AgentStarted & { root: string }) | null>(null);
  /** 正在起（第一次要下适配器）：起了多少秒 */
  starting = $state(0);
  startErr = $state("");
  /** 这一轮还没说完 */
  running = $state(false);
  items = $state<Item[]>([]);
  edits = $state<Edit[]>([]);
  /** 输入框上方挂着的上下文（下一轮一起发） */
  ctx = $state<Ctx[]>([]);
  /** 审阅页开着 */
  reviewing = $state(false);
  private unlisten: (() => void) | null = null;

  constructor() {
    try {
      const k = localStorage.getItem(KIND_KEY);
      if (k === "claude" || k === "codex") this.kind = k;
    } catch {
      /* 读不到就当第一次用 */
    }
  }

  get pendingEdits() {
    return this.edits.filter((e) => e.state === "pending");
  }

  choose(k: AgentKind) {
    this.kind = k;
    try {
      localStorage.setItem(KIND_KEY, k);
    } catch {
      /* 存不下只是下次再选一次 */
    }
  }

  private async listen() {
    if (this.unlisten) return;
    this.unlisten = await listenHere<AgentEvent>("agent-event", (e) => this.onEvent(e.payload));
  }

  /** 起会话。已经起好、项目没换就什么都不做 */
  async ensure(): Promise<boolean> {
    const root = project.root;
    if (!root || !this.kind) return false;
    if (this.session && this.session.root === root) return true;
    if (this.session) this.stop();
    if (this.starting) return false;
    await this.listen();
    this.startErr = "";
    this.starting = 1;
    const tick = setInterval(() => this.starting && this.starting++, 1000);
    try {
      const s = await agentStart(root, this.kind);
      this.session = { ...s, root };
      return true;
    } catch (e) {
      this.startErr = String(e);
      return false;
    } finally {
      clearInterval(tick);
      this.starting = 0;
    }
  }

  stop() {
    const s = this.session;
    if (!s) return;
    this.session = null;
    this.running = false;
    void agentStop(s.id);
  }

  /** 换个 agent：停掉现在这个，下一句话用新的起 */
  switchTo(k: AgentKind) {
    if (k === this.kind) return;
    this.stop();
    this.choose(k);
    this.items.push({ t: "note", text: `换成 ${k === "claude" ? "Claude Code" : "Codex"}，下一句话起新会话`, bad: false });
  }

  /** 新对话：停掉会话、清空 */
  reset() {
    this.stop();
    this.items = [];
    this.ctx = [];
    this.startErr = "";
  }

  attach(c: Ctx) {
    this.ctx.push(c);
    layout.showSide("agent");
  }

  async send(text: string) {
    text = text.trim();
    if ((!text && this.ctx.length === 0) || this.running) return;
    const ctx = this.ctx;
    this.ctx = [];
    // 审阅单只留还没处理的：处理过的上一轮已经看过了
    this.edits = this.edits.filter((e) => e.state === "pending");
    this.items.push({ t: "user", text, ctx });
    this.running = true;
    try {
      if (!(await this.ensure())) {
        this.items.push({ t: "note", text: this.startErr || "agent 没起来", bad: true });
        return;
      }
      const blocks: unknown[] = ctx.map((c) => ({
        type: "text",
        text: `${rel(c.path)} 第 ${c.from}${c.to > c.from ? `–${c.to}` : ""} 行：\n\`\`\`\n${c.text}\n\`\`\``,
      }));
      if (text) blocks.push({ type: "text", text });
      const why = await agentPrompt(this.session!.id, blocks);
      if (why === "cancelled") this.items.push({ t: "note", text: "已停止", bad: false });
      else if (why === "max_tokens" || why === "max_turn_requests") this.items.push({ t: "note", text: `说到上限停了（${why}）`, bad: true });
      else if (why === "refusal") this.items.push({ t: "note", text: "agent 拒绝了这个请求", bad: true });
    } catch (e) {
      this.items.push({ t: "note", text: String(e), bad: true });
    } finally {
      this.running = false;
      this.cancelPerms();
    }
  }

  cancel() {
    if (!this.session || !this.running) return;
    void agentCancel(this.session.id);
    // 协议要求：停这一轮时，还挂着的权限请求回 cancelled（不然 agent 那边一直等着）
    this.cancelPerms();
  }

  private cancelPerms() {
    const s = this.session;
    for (const it of this.items) {
      if (it.t === "perm" && !it.chosen) {
        it.chosen = "—";
        if (s) void agentReply(s.id, it.req, { outcome: { outcome: "cancelled" } });
      }
    }
  }

  answer(it: Item & { t: "perm" }, opt: PermOption) {
    const s = this.session;
    if (!s || it.chosen) return;
    it.chosen = opt.optionId;
    void agentReply(s.id, it.req, { outcome: { outcome: "selected", optionId: opt.optionId } });
  }

  async cycleMode() {
    const s = this.session;
    const m = s?.modes;
    if (!s || !m || m.availableModes.length < 2) return;
    const i = m.availableModes.findIndex((x) => x.id === m.currentModeId);
    const next = m.availableModes[(i + 1) % m.availableModes.length];
    try {
      await agentSetMode(s.id, next.id);
      m.currentModeId = next.id;
    } catch (e) {
      notify.fail(`换不了模式：${e}`);
    }
  }

  private last(): Item | undefined {
    return this.items[this.items.length - 1];
  }

  private tool(id: string) {
    for (let i = this.items.length - 1; i >= 0; i--) {
      const it = this.items[i];
      if (it.t === "tool" && it.id === id) return it;
    }
    return undefined;
  }

  private onEvent(e: AgentEvent) {
    const s = this.session;
    if (!s || e.id !== s.id) {
      // 刚起、还没拿到 id 时 agent 也可能已经开口（available_commands_update）：那种不要紧，丢掉
      return;
    }
    const d = e.data;
    switch (e.kind) {
      case "update":
        this.onUpdate(d);
        break;
      case "permission": {
        const p = d.params as { toolCall?: { title?: string }; options?: PermOption[] };
        this.items.push({ t: "perm", req: d.req, title: p.toolCall?.title ?? "要一个权限", options: p.options ?? [], chosen: "" });
        break;
      }
      case "read":
        // 编辑器里有没保存的那份就给它；没有回 null，Rust 那边读盘
        void agentReply(s.id, d.req, { content: liveOf(String(d.path)) });
        break;
      case "write":
        void this.onWrite(s.id, d.req, String(d.path), String(d.content ?? ""));
        break;
      case "exit":
        this.session = null;
        if (this.running) this.items.push({ t: "note", text: `agent 退出了（退出码 ${d.code ?? "无"}）`, bad: true });
        this.running = false;
        break;
    }
  }

  private onUpdate(u: Record<string, unknown>) {
    const kind = u.sessionUpdate;
    const text = (u.content as { text?: string } | undefined)?.text ?? "";
    const last = this.last();
    switch (kind) {
      case "agent_message_chunk":
        if (last?.t === "agent") last.text += text;
        else this.items.push({ t: "agent", text });
        break;
      case "agent_thought_chunk":
        if (last?.t === "thought") last.text += text;
        else this.items.push({ t: "thought", text });
        break;
      case "tool_call":
      case "tool_call_update": {
        const id = String(u.toolCallId ?? "");
        let it = this.tool(id);
        if (!it) {
          this.items.push({ t: "tool", id, title: "", kind: "other", status: "pending", path: "", out: "" });
          it = this.tool(id)!;
        }
        if (typeof u.title === "string") it.title = u.title;
        if (typeof u.kind === "string") it.kind = u.kind;
        if (typeof u.status === "string") it.status = u.status;
        const loc = (u.locations as { path?: string }[] | undefined)?.[0]?.path;
        if (loc) it.path = loc;
        const out = (u.content as { type?: string; content?: { text?: string } }[] | undefined)
          ?.map((c) => (c.type === "content" ? (c.content?.text ?? "") : ""))
          .join("");
        if (out) it.out = out.length > 4000 ? `${out.slice(0, 4000)}\n…（还有 ${out.length - 4000} 个字没显示）` : out;
        break;
      }
      case "plan":
        this.items.push({ t: "plan", entries: ((u.entries as { content: string; status: string }[]) ?? []).map((x) => ({ content: x.content, status: x.status })) });
        // 同一轮里计划会推好几版：只留最新那版
        for (let i = this.items.length - 2; i >= 0; i--) {
          if (this.items[i].t === "plan") {
            this.items.splice(i, 1);
            break;
          }
          if (this.items[i].t === "user") break;
        }
        break;
      case "current_mode_update":
        if (this.session?.modes && typeof u.currentModeId === "string") this.session.modes.currentModeId = u.currentModeId;
        break;
    }
  }

  /** agent 要写一个文件：写进编辑器（一笔修改、不存盘），记进审阅单。新文件没有编辑器可进，直接落盘（退回 = 扔进废纸篓） */
  private async onWrite(id: number, req: unknown, path: string, content: string) {
    try {
      let before = liveOf(path);
      let isNew = false;
      if (before === null) {
        try {
          before = (await readText(path)).content;
        } catch {
          isNew = true;
        }
      }
      if (isNew) {
        await writeText(path, content);
      } else {
        let v = viewOf(path);
        if (!v) {
          const { tabflow } = await import("../state/tabflow.svelte");
          await tabflow.openPath(path, { preview: false });
          v = await waitView(path);
        }
        if (!v) throw new Error("编辑器没打开");
        v.dispatch({ changes: lineChanges(v.state.doc.toString(), content), userEvent: "input.paste" });
      }
      const had = this.edits.find((e) => e.path === path && e.state === "pending");
      if (had) had.after = content;
      else this.edits.push({ path, before: isNew ? null : before, after: content, state: "pending" });
      await agentReply(id, req, {});
    } catch (e) {
      await agentReply(id, req, null, `没写成：${e}`);
    }
  }

  /** 留下：存盘（编辑器里此刻那份，包括人自己在 agent 改完之后又动的） */
  async keep(e: Edit) {
    if (e.state !== "pending") return;
    if (e.before !== null) {
      const t = tabs.byPath(e.path);
      if (t && t.mode === "edit" && !(await docs.saveTab(t, liveOf(e.path) ?? docs.liveText(t)))) return;
    }
    e.state = "kept";
  }

  /** 退回：编辑器里换回 agent 第一次写之前那份（一笔修改，⌘Z 撤得回）；它新建的文件扔进废纸篓 */
  async revert(e: Edit) {
    if (e.state !== "pending") return;
    try {
      if (e.before === null) {
        const t = tabs.byPath(e.path);
        if (t) {
          const { tabflow } = await import("../state/tabflow.svelte");
          tabflow.doClose(t);
        }
        await trashEntry(e.path);
      } else {
        let v = viewOf(e.path);
        if (!v) {
          const { tabflow } = await import("../state/tabflow.svelte");
          await tabflow.openPath(e.path, { preview: false });
          v = await waitView(e.path);
        }
        if (!v) throw new Error("编辑器没打开");
        v.dispatch({ changes: lineChanges(v.state.doc.toString(), e.before), userEvent: "input.paste" });
      }
      e.state = "reverted";
    } catch (err) {
      notify.fail(`没退回 ${rel(e.path)}：${err}`);
    }
  }

  async keepAll() {
    for (const e of this.pendingEdits) await this.keep(e);
  }
  async revertAll() {
    for (const e of this.pendingEdits) await this.revert(e);
  }
}

export const agent = new AgentState();

/** 编辑器右键「发给 AI 助手」：选中的那段（没选就是光标那一行）带着行号挂到输入框上 */
export function attachSelection(path: string) {
  const v = viewOf(path);
  if (!v) return;
  const { from, to } = v.state.selection.main;
  const doc = v.state.doc;
  const a = doc.lineAt(from);
  const b = doc.lineAt(from === to ? from : Math.max(from, to - 1));
  const text = from === to ? a.text : doc.sliceString(from, to);
  agent.attach({ path, from: a.number, to: b.number, text: text.length > 20000 ? `${text.slice(0, 20000)}\n…（选中的太长，后面 ${text.length - 20000} 个字没带上）` : text });
}
