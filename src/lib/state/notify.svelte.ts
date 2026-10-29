import { untrack } from "svelte";
import { errText } from "./err-text";

/**
 * 状态消息通道。
 *
 * # 为什么要有它
 *
 * 原本 App.svelte 里散着 45 处 `error = …` / `saved = …`，每处后面跟一个
 * 自己写的 `setTimeout(() => (error = ""), N)`，N 有 1800 / 2200 / 2600 /
 * 3000 / 3600 / 4000 六种值 —— 同一类消息在不同地方停留时长不同，纯属手抖。
 *
 * 更要命的是**定时器互相踩**：所有消息共用一个变量，却各起各的定时器。
 * 先弹一条 1800ms 的「已保存」，紧接着来一条 4000ms 的错误，
 * 那条错误会在 1800ms 时被前一条的定时器抹掉 —— 用户根本来不及读。
 *
 * 现在只有一条胶囊、一个定时器，新消息进来先把旧的取消掉（见下面「画在哪」）。
 *
 * # 四种，按「结果是什么」分
 *
 * - `ok` —— 做成了。自己消失。
 * - `fail` —— 没做成，一句话说得清。停得久一点，点一下就关。
 * - `hint` —— **什么都没发生，告诉你为什么**（剪贴板里没有文字、光标不在名字上、标签钉住了所以 ⌘W 没关它）。
 *   原来这几条走 `ok`，画成了「做成了」的颜色 —— 一扫过去人以为做成了（2026-10-13 盘点 B2）。灰字、没有图标。
 * - `block` —— 没做成，而且说明是多行的（git 拒绝切分支时会列出涉及的文件）。走卡片，**不自动消失**。
 *
 * `ok` / `fail` 可以带**一个动作**（`act`，胶囊右边一个字按钮）：做成了、附一条退路 —— 「已替换 3 个文件 12 处 · 撤销」。
 * 原来这种是一张顶在编辑区上的卡片，和胶囊里同一句话说两遍（2026-10-21 用户：「撤销那个是不是可以换成这样」）。
 * 带动作的至少停 10 秒（够人看完再伸手），悬停照旧不走。胶囊走了这条退路也不能没了：撤销替换在「编辑」菜单里常驻一项。
 *
 * # 画在哪：编辑区底部居中的胶囊（`shell/Toast.svelte`）
 *
 * 原来 `ok` / `fail` 画在状态栏最左那一格，顶掉面包屑 2.6 秒。眼睛这时候在编辑区或者刚点的那块，左下角是视野最边上，
 * 很多提示没人看见就走了（2026-10-13 盘点 B1，用户选了方案 B）。照 macOS 的 HUD：一次只有一条，新的顶掉旧的 ——
 * **但没做成的不被做成的顶掉**：一条「推送失败」后 100ms 来一条「已保存」，人要读的是前一条。
 * 停留时长按字数算（`readMs`），调用方给了就用给的。
 */

/** 一句话读完要多久：起步 1.6 秒，每个字加 60ms，最多 6 秒。原来一律 2.6 秒，四个字的「已推送」嫌长，三十个字的说明读不完 */
export function readMs(text: string, floor = 1600): number {
  return Math.min(6000, Math.max(floor, 1000 + [...text].length * 60));
}
/** 没做成的至少停 4 秒：它通常比成功消息长，也更需要读清楚 */
const FAIL_FLOOR = 4000;

export type ToastKind = "ok" | "fail" | "hint";
/** 胶囊上的那一个动作（「撤销」） */
export interface ToastAct {
  label: string;
  run: () => void;
}
export interface Toast {
  kind: ToastKind;
  text: string;
  act?: ToastAct;
  /** 每条一个号：同样的字再来一次也算新的一条（胶囊要重新进场、重新计时） */
  id: number;
}

/** 通知记录里的一条（docs/TAKEOVER.md I11）：卡片记成 `block`，正文是标题 + 说明 */
export interface LogItem {
  id: number;
  at: number;
  kind: ToastKind | "block";
  text: string;
}
/** 记录留多少条：一次会话里几百条提示已经翻不过来了，再多只是占内存 */
const LOG_MAX = 300;

class Notify {
  /**
   * 这次会话说过的每一句（I11，IDEA 的 Notifications）：胶囊几秒就走、被没读完的失败压下去的「已保存」根本没露面 ——
   * 不打断的代价是「错过了」，要能补看。底部工具窗「通知」列它。只在内存里、按窗口各一份
   */
  log = $state<LogItem[]>([]);
  /** 上次看过之后又来了几条坏消息（没做成、卡片）：导轨上那个铃的角标。看一眼就清 */
  unseen = $state(0);
  /** 此刻胶囊里那一条；没有就是 null */
  toast = $state<Toast | null>(null);
  /** 多行说明，需要用户自己关（卡片） */
  banner = $state<{ title: string; body: string } | null>(null);

  /**
   * 正在做、还没做完的事（"提交"、"切分支"…）。
   *
   * **没有定时器，是有意的** —— 上面几种都由时间收走，而这一条的结束
   * 由操作本身决定：git 提交跑 pre-commit 钩子可能要三十秒，检出几千个文件
   * 是秒级（那几条正是被有意挪到阻塞池上的，见 rules/rust.md）。
   * 按时间收走它，等于在操作还没完的时候告诉用户"完了"。
   *
   * 直接赋值：开始时 `notify.doing = "提交"`，结束时 `notify.doing = ""`。
   * 结束一定要走 `finally` —— 失败路径上不清的话，状态栏会永远卡着一句
   * "正在提交…"，而那比不显示更糟。
   */
  doing = $state("");

  #timer: ReturnType<typeof setTimeout> | undefined;
  #until = 0;
  #left = 0;
  #seq = 0;

  /** 做成了。`act`：附一条退路（「撤销」） */
  ok(text: string, ms?: number, act?: ToastAct) {
    this.#show("ok", text, ms ?? readMs(text), act);
  }

  /** 没做成，一句话说得清 */
  fail(text: string, ms?: number, act?: ToastAct) {
    this.#show("fail", text, Math.max(ms ?? 0, readMs(text, FAIL_FLOOR)), act);
  }

  /** 什么都没发生，说一句为什么。`act`：下一步（「安装」） */
  hint(text: string, ms?: number, act?: ToastAct) {
    this.#show("hint", text, ms ?? readMs(text), act);
  }

  /*
   * `untrack`：调用方常在 `$effect` 里（「按需加载失败了」那几条），这里要先读一遍 `log` / `unseen` 再写 ——
   * 不 untrack 的话那个 effect 就依赖上了自己写的东西，一报就停不下来（同 progress.svelte.ts 那次）
   */
  #record(kind: LogItem["kind"], text: string) {
    untrack(() => {
      this.log = [...this.log.slice(-(LOG_MAX - 1)), { id: ++this.#seq, at: Date.now(), kind, text }];
      if (kind === "fail" || kind === "block") this.unseen++;
    });
  }

  #show(kind: ToastKind, text: string, ms: number, act?: ToastAct) {
    // 先记再判要不要露面：被没读完的失败压下去的那条，正是最需要在记录里补看的
    this.#record(kind, text);
    const cur = this.toast;
    // 没做成的那条还没读完：做成了 / 提示不顶掉它（新的失败照样顶 —— 那是更新的坏消息）。
    // 带动作的例外：它是人刚做的那一下的结果、上面有人要按的按钮 —— 「没改名：X 已经有了」之后马上换个名字再改，「撤销」不能被吞掉
    if (cur?.kind === "fail" && kind !== "fail" && !act && Date.now() < this.#until) return;
    clearTimeout(this.#timer);
    this.toast = { kind, text, act, id: ++this.#seq };
    this.#arm(act ? Math.max(ms, 10000) : ms);
  }

  #arm(ms: number) {
    this.#left = ms;
    this.#until = Date.now() + ms;
    this.#timer = setTimeout(() => (this.toast = null), ms);
  }

  /** 鼠标停在胶囊上：先别走（人在读）。离开之后再给剩下的时间、至少 1 秒 */
  hold() {
    if (!this.toast || this.#until === Number.MAX_SAFE_INTEGER) return;
    clearTimeout(this.#timer);
    this.#left = Math.max(0, this.#until - Date.now());
    this.#until = Number.MAX_SAFE_INTEGER;
  }
  release() {
    if (!this.toast) return;
    this.#arm(Math.max(1000, this.#left));
  }

  /** 点掉 */
  dismiss() {
    clearTimeout(this.#timer);
    this.#until = 0;
    this.toast = null;
  }

  /**
   * 没做成，说明是多行的 —— 走卡片，不自动消失。
   *
   * git 的错误就是这样：「你有未提交的改动，请先 commit 或 stash，
   * 涉及这些文件：…」。塞进一句话的胶囊会被截断成一句没头没尾的话。
   */
  block(title: string, body: unknown) {
    // **不能 String(body)** —— IPC 那层有几条命令 reject 的是结构化对象
    // （`SwitchErr` 那种），String() 出来是 `[object Object]`。见 err-text.ts
    this.banner = { title, body: errText(body) };
    this.#record("block", `${title}\n${this.banner.body}`);
  }

  closeBanner() {
    this.banner = null;
  }

  /** 手动清掉胶囊（比如开始一次新操作时） */
  clear() {
    this.dismiss();
  }
}

export const notify = new Notify();
