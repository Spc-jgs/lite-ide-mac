import { listScratches, type ScratchEntry } from "../ipc/commands";

/**
 * 侧边栏「草稿」列表的数据（issue #40）。
 *
 * 只有一份列表和一个 `refresh()`。**不监听目录**：草稿目录只有这个应用自己在写
 * （⌘N 新建、自动保存、废纸篓），每一处写完自己叫一声 `refresh` 就够了 ——
 * 为它再挂一个 FSEvents 监听，是拿一个新变量换一件已经确定的事。
 *
 * `refresh` 并发进来只跑一趟：自动保存每半秒可能来一次，而列表本身要打开
 * 每一份草稿读头 4KB。正在跑的那趟结束后如果又有人要，再补一趟。
 */
class Scratches {
  list = $state<ScratchEntry[]>([]);
  /** 至少成功拉过一次。没拉过时列表是空的，但那不是「一条都没有」 */
  loaded = $state(false);
  #inflight: Promise<void> | null = null;
  #again = false;

  refresh(): Promise<void> {
    if (this.#inflight) {
      this.#again = true;
      return this.#inflight;
    }
    this.#inflight = listScratches()
      .then((l) => {
        this.list = l;
        this.loaded = true;
      })
      .catch(() => {
        /* 列不出来就保持上一份；目录不存在 Rust 侧已经返回空列表，不会走到这 */
      })
      .finally(() => {
        this.#inflight = null;
        if (this.#again) {
          this.#again = false;
          void this.refresh();
        }
      });
    return this.#inflight;
  }
}

export const scratches = new Scratches();
