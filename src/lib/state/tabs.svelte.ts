import { audit } from "./invariant";
import { underPath, type TabState } from "./tab";

/**
 * 标签表：开了哪些标签、哪个在前。
 *
 * 这一步（issue #9 第 4 步）只搬**表本身**和只碰表的操作：加、删、按 id / 路径找、
 * 「某条路径底下有几个」。打开一个文件要读盘、关一个要问「未保存怎么办」、
 * 切模式要开日志引擎 —— 那些流程还在 App 里，它们各自调这里的原语。
 *
 * # 两条老规矩，搬过来一个字不改
 *
 * 1. **异步路径上按 id 重新取。** `list` 是深层 `$state`，元素**读取时**才包成
 *    代理；`await` 回来手上那个引用可能已经不是响应式的那份了。
 *    `byId(id)` 就是为这条规矩存在的（frontend.md 第一条）。
 * 2. **不变量自检显式调，不做成 effect。** effect 会跟着 `list` 里任何一个字段动，
 *    包括每敲一个键就翻一次的 `dirty`（issue #27）。
 */

class Tabs {
  list = $state<TabState[]>([]);
  activeId = $state<number | null>(null);
  active = $derived(this.list.find((t) => t.id === this.activeId) ?? null);
  /** 此刻那个预览标签（最多一个，见 `TabState.preview`）；没有就 null */
  preview = $derived(this.list.find((t) => t.preview) ?? null);
  #nextId = 1;

  byId(id: number): TabState | null {
    return this.list.find((t) => t.id === id) ?? null;
  }

  byPath(path: string): TabState | null {
    return this.list.find((t) => t.path === path) ?? null;
  }

  /**
   * 追加一个标签，分配 id。**不切过去** —— 会话恢复时一次开十几个，
   * 每开一个就切一次是那个「一个文件一个文件地闪」的 bug（JOURNAL 2026-09-03）。
   * 要切的调用方自己 `activeId = id`。
   */
  add(t: Omit<TabState, "id">, at?: number): number {
    const id = this.#nextId++;
    const tab = { ...t, id } as TabState;
    // `at` 只有「顶掉预览标签」用：新的要落在旧的那一格，不然标签条会跳一下
    this.list =
      at === undefined || at < 0 || at >= this.list.length
        ? [...this.list, tab]
        : [...this.list.slice(0, at), tab, ...this.list.slice(at)];
    return id;
  }

  /** 把预览标签钉住。不是预览的什么也不发生，所以调用方不用先判 */
  pin(id: number) {
    const t = this.byId(id);
    if (t?.preview) t.preview = false;
  }

  /** 从表里拿掉，活动标签落到它原来的位置（最后一个则往前退一格） */
  remove(id: number) {
    const idx = this.list.findIndex((t) => t.id === id);
    this.list = this.list.filter((t) => t.id !== id);
    if (this.activeId === id) {
      this.activeId = this.list[Math.min(idx, this.list.length - 1)]?.id ?? null;
    }
  }

  /** 在 `p` 底下（含它自己）的标签 */
  under(p: string, isDir: boolean): TabState[] {
    return this.list.filter((t) => underPath(t.path, p, isDir));
  }

  /** 传给文件树：这条路径底下有几个未保存的标签（删除确认框要说清楚） */
  dirtyUnder(p: string): number {
    return this.list.filter((t) => t.dirty && underPath(t.path, p, true)).length;
  }

  /**
   * 在一个状态转换点上核一遍标签的不变量（issue #27，判据全在
   * `invariant.ts` 里，这里只负责「在哪些点上核」）。
   *
   * 第三个参数是「此刻哪个标签挂着活编辑器」：编辑器的 onChange 只改 `dirty`，
   * `draft` 要等 onStash 才回写，所以正在被编辑的那个标签本来就会
   * 短暂地 dirty 而无 draft —— 不告诉自检器这件事，它会在每次敲键盘时报假警。
   */
  audit(where: string) {
    audit(this.list, this.activeId, this.activeId, where);
  }
}

export const tabs = new Tabs();
