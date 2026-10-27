/**
 * git 要凭据时问到界面上（#62）：Rust 发 `git-askpass`（只发给发起那次拉取 / 推送 / 克隆的窗口），这里弹框、答完回 `git_askpass_answer`；
 * 那次操作结束了（成功、失败、取消）Rust 发 `git-askpass-done`，还开着的框收起来。
 * 第一次远程操作之前挂上（`ensureAskpass`），之后一直挂着 —— 同 runs-ops 的 `listen`
 */
import { listenHere, type GitAskpass } from "../ipc/commands";
import { gitAskpassAnswer } from "../ipc/git";

let listening: Promise<unknown> | null = null;
/** 开着的框：号 → 关掉它的函数 */
const open = new Map<number, () => void>();

export function ensureAskpass(): Promise<unknown> {
  listening ??= Promise.all([
    listenHere<GitAskpass>("git-askpass", (e) => void ask(e.payload)),
    listenHere<number>("git-askpass-done", (e) => open.get(e.payload)?.()),
  ]);
  return listening;
}

async function ask(q: GitAskpass) {
  const [{ openModal }, m] = await Promise.all([import("../settings/modal-host"), import("./GitAskpass.svelte")]);
  let answered = false;
  const reply = (v: string | null) => {
    if (answered) return;
    answered = true;
    void gitAskpassAnswer(q.id, v).catch(() => {});
  };
  // 从哪条路关的（取消、Esc、操作结束）都算「没答」；答过了就不再送
  const close = openModal(m.default, { prompt: q.prompt, secret: q.secret, onanswer: reply }, () => {
    open.delete(q.id);
    reply(null);
  });
  open.set(q.id, close);
}
