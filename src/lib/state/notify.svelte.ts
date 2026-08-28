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
 * 这里每个通道只有一个定时器，新消息进来先把旧的取消掉。
 *
 * # 三个通道，按「用户要不要动手」分
 *
 * - `info` —— 做成了。自己消失。
 * - `fail` —— 没做成，但一句话说得清。自己消失。
 * - `block` —— 没做成，而且说明是多行的（git 拒绝切分支时会列出涉及的文件）。
 *   **不自动消失**，用户读完自己关：一段多行说明还没读完就被定时器收走，
 *   比不显示更气人。
 */

/** 一句话消息的默认停留时长。够读完一句中文，又不至于赖着不走 */
const INFO_MS = 2600;
/** 失败消息停留久一点 —— 它通常比成功消息长，也更需要读清楚 */
const FAIL_MS = 4000;

class Notify {
  /** 一句话的失败提示 */
  error = $state("");
  /** 一句话的成功提示 */
  info = $state("");
  /** 多行说明，需要用户自己关 */
  banner = $state<{ title: string; body: string } | null>(null);

  #errTimer: ReturnType<typeof setTimeout> | undefined;
  #infoTimer: ReturnType<typeof setTimeout> | undefined;

  /** 做成了 */
  ok(text: string, ms = INFO_MS) {
    clearTimeout(this.#infoTimer);
    this.info = text;
    this.#infoTimer = setTimeout(() => (this.info = ""), ms);
  }

  /** 没做成，一句话说得清 */
  fail(text: string, ms = FAIL_MS) {
    clearTimeout(this.#errTimer);
    this.error = text;
    this.#errTimer = setTimeout(() => (this.error = ""), ms);
  }

  /**
   * 没做成，说明是多行的 —— 走横幅，不自动消失。
   *
   * git 的错误就是这样：「你有未提交的改动，请先 commit 或 stash，
   * 涉及这些文件：…」。塞进状态栏那一格会被截断成一句没头没尾的话。
   */
  block(title: string, body: unknown) {
    this.banner = { title, body: String(body).replace(/^Error:\s*/, "") };
  }

  closeBanner() {
    this.banner = null;
  }

  /** 手动清掉一句话消息（比如开始一次新操作时） */
  clear() {
    clearTimeout(this.#errTimer);
    clearTimeout(this.#infoTimer);
    this.error = "";
    this.info = "";
  }
}

export const notify = new Notify();
