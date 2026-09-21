import { audit } from "./invariant";
import { underPath, type Group, type TabState } from "./tab";

/**
 * 标签表：开了哪些标签、哪个在前、**分成几组**。
 *
 * 这一步（issue #9 第 4 步）只搬**表本身**和只碰表的操作：加、删、按 id / 路径找、
 * 「某条路径底下有几个」。打开一个文件要读盘、关一个要问「未保存怎么办」、
 * 切模式要开日志引擎 —— 那些流程还在 App 里，它们各自调这里的原语。
 *
 * # 分组（issue #35 分屏，设计见 docs/SPLIT.md）
 *
 * `list` 还是一维的、顺序就是标签条顺序；每个标签带一个 `group`（0 左 1 右），
 * 标签条按组过滤着画。`shown` 记每组正在显示谁：长度 1 是单栏，2 是分屏。
 * **`activeId` 语义不变** —— 它是「光标所在的那个标签」，也就是焦点组正在显示的；
 * 状态栏、面包屑、⌘S 那一百多处读它的地方一个字不用改。由此得出核心不变量：
 * `activeId === shown[active.group]`。所有「切到某个标签」的写入收口到 `show()`，
 * 直接给 `activeId` 赋值会绕过 `shown`，自检在下一个转换点报出来。
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
  /**
   * 每组正在显示的标签 id。长度 1 = 单栏，2 = 分屏。**没有空组**：一组关到空就收起
   * （`remove`），所以分屏时两格都不为 null；单栏时只有「一个标签都没有」才是 null。
   */
  shown = $state<(number | null)[]>([null]);
  /** 焦点组 = 活动标签所在的组；没有标签时 0 */
  activeGroup = $derived<Group>(this.active?.group ?? 0);
  split = $derived(this.shown.length === 2);
  #nextId = 1;

  byId(id: number): TabState | null {
    return this.list.find((t) => t.id === id) ?? null;
  }

  byPath(path: string): TabState | null {
    return this.list.find((t) => t.path === path) ?? null;
  }

  /** 某一组的标签，顺序即标签条顺序 */
  inGroup(g: Group): TabState[] {
    return this.list.filter((t) => t.group === g);
  }

  /** 某一组正在显示的标签 */
  shownIn(g: Group): TabState | null {
    const id = this.shown[g];
    return id === null || id === undefined ? null : this.byId(id);
  }

  /** 某一组此刻的预览标签（每组最多一个，见 `TabState.preview`）；没有就 null */
  previewIn(g: Group): TabState | null {
    return this.list.find((t) => t.preview && t.group === g) ?? null;
  }

  /**
   * 切到某个标签：它所在的组显示它，它成为活动标签（焦点跟到那一组）。
   * **所有切标签的写入都走这儿**，理由见文件头。id 不存在什么也不做。
   */
  show(id: number) {
    const t = this.byId(id);
    if (!t) return;
    this.shown[t.group] = id;
    this.activeId = id;
  }

  /** 焦点换到另一组（点进它的编辑器 / 标签条），显示的标签不变 */
  focusGroup(g: Group) {
    const id = this.shown[g];
    if (id !== null && id !== undefined) this.activeId = id;
  }

  /**
   * 追加一个标签，分配 id。**不切过去** —— 会话恢复时一次开十几个，
   * 每开一个就切一次是那个「一个文件一个文件地闪」的 bug（JOURNAL 2026-09-03）。
   * 要切的调用方自己 `show(id)`。不给 `group` 就落在焦点组。
   */
  add(t: Omit<TabState, "id" | "group"> & { group?: Group }, at?: number): number {
    const id = this.#nextId++;
    const tab = { ...t, group: t.group ?? this.activeGroup, id } as TabState;
    // `at` 只有「顶掉预览标签」用：新的要落在旧的那一格，不然标签条会跳一下
    this.list =
      at === undefined || at < 0 || at >= this.list.length
        ? [...this.list, tab]
        : [...this.list.slice(0, at), tab, ...this.list.slice(at)];
    return id;
  }

  /** 把预览标签保留下来（不再是预览）。不是预览的什么也不发生，所以调用方不用先判 */
  keep(id: number) {
    const t = this.byId(id);
    if (t?.preview) t.preview = false;
  }

  /**
   * 钉住 / 取消钉住（issue #33 ⑰）。钉住的排在最左，照 VS Code：钉的那个挪到
   * 钉住那组的末尾，取消的挪到那组后面第一格 —— 「钉住的都在左边」这条不变量
   * 由这里维护，渲染那边不排序。**按组算**：另一组的标签在 `list` 里穿插着也不影响。
   */
  setPinned(id: number, on: boolean) {
    const idx = this.list.findIndex((t) => t.id === id);
    if (idx < 0 || !!this.list[idx].pinned === on) return;
    const t = this.list[idx];
    t.pinned = on;
    if (on) t.preview = false;
    const rest = this.list.filter((x) => x.id !== id);
    this.list = this.#insertAfterPinned(rest, t);
  }

  /**
   * 把 `t` 插进 `rest`：钉住的落在它那组钉住区的末尾，没钉的落在那组末尾。
   * 「那组末尾」在一维表里是「那组最后一个标签之后」—— 组里一个都没有就落在表尾。
   */
  #insertAfterPinned(rest: TabState[], t: TabState): TabState[] {
    let at: number;
    if (t.pinned) {
      const firstUnpinned = rest.findIndex((x) => x.group === t.group && !x.pinned);
      at = firstUnpinned >= 0 ? firstUnpinned : this.#groupEnd(rest, t.group);
    } else {
      at = this.#groupEnd(rest, t.group);
    }
    return [...rest.slice(0, at), t, ...rest.slice(at)];
  }

  #groupEnd(list: TabState[], g: Group): number {
    for (let i = list.length - 1; i >= 0; i--) if (list[i].group === g) return i + 1;
    return list.length;
  }

  /**
   * 从表里拿掉。它所在的组正显示它的话，那一组落到它原来的位置（同组内数；最后一个
   * 则往前退一格）—— 和单栏时一样，只是「邻居」限于同组。那一组因此空了就**收起**：
   * 分屏收成单栏（另一组的标签全归组 0），单栏就是一个标签都没有了。
   */
  remove(id: number) {
    const idx = this.list.findIndex((t) => t.id === id);
    if (idx < 0) return;
    const g = this.list[idx].group;
    const gi = this.list.slice(0, idx).filter((t) => t.group === g).length;
    this.list = this.list.filter((t) => t.id !== id);
    if (this.shown[g] !== id) return;
    const mates = this.inGroup(g);
    const next = mates[Math.min(gi, mates.length - 1)] ?? null;
    if (next) {
      this.shown[g] = next.id;
      if (this.activeId === id) this.activeId = next.id;
      return;
    }
    this.#collapse(g);
  }

  /** 组 `g` 空了：收起它。剩下那组的标签全归 0，焦点落到它正在显示的 */
  #collapse(g: Group) {
    if (!this.split) {
      this.shown = [null];
      this.activeId = null;
      return;
    }
    const keep = this.shown[g === 0 ? 1 : 0];
    for (const t of this.list) t.group = 0;
    this.shown = [keep];
    this.activeId = keep;
  }

  /**
   * 把一个标签挪到另一组；单栏时就是**向右分屏**（组 1 从这一下诞生）。
   * 返回做没做成。不做的两种情况：
   * - 它所在的组只有它一个 —— 挪走之后原组空了立刻收起，等于白做。IDEA / VS Code
   *   这时是把同一个文件开两份，而这轮不做同一文件双实例（docs/SPLIT.md 第 8 节）
   * - 已经在目标组
   * 做成之后目标组显示它、焦点跟过去；原组正显示它的话按 `remove` 的规则落到邻居。
   */
  moveToGroup(id: number, g: Group): boolean {
    const idx = this.list.findIndex((t) => t.id === id);
    if (idx < 0) return false;
    const t = this.list[idx];
    const from = t.group;
    if (from === g) return false;
    if (this.inGroup(from).length < 2) return false;
    if (!this.split) {
      if (g !== 1) return false;
      this.shown = [this.shown[0], null];
    }
    if (this.shown[from] === id) {
      const gi = this.list.slice(0, idx).filter((x) => x.group === from).length;
      const mates = this.inGroup(from).filter((x) => x.id !== id);
      this.shown[from] = mates[Math.min(gi, mates.length - 1)].id;
    }
    const rest = this.list.filter((x) => x.id !== id);
    t.group = g;
    this.list = this.#insertAfterPinned(rest, t);
    this.show(id);
    return true;
  }

  /** 「移到另一组」：分屏时是对面那组，单栏时是向右分屏 */
  moveToOther(id: number): boolean {
    const t = this.byId(id);
    if (!t) return false;
    return this.moveToGroup(id, this.split && t.group === 1 ? 0 : 1);
  }

  /** 能不能把这个标签挪到另一组（菜单项灰不灰） */
  canMove(id: number): boolean {
    const t = this.byId(id);
    return !!t && this.inGroup(t.group).length >= 2;
  }

  /**
   * 合并分屏：右组的标签全并回左组。顺序重排成「左钉住、右钉住、左其余、右其余」——
   * 一维表里两组本来穿插着，直接改 `group` 会让一个钉住的落在没钉的后面。
   * 焦点组正在显示的那个留在屏上（VS Code 的 Join 也是保活动的那个）。
   */
  unsplit() {
    if (!this.split) return;
    const keep = this.shown[this.activeGroup];
    const pick = (g: Group, pinned: boolean) => this.list.filter((t) => t.group === g && !!t.pinned === pinned);
    const order = [...pick(0, true), ...pick(1, true), ...pick(0, false), ...pick(1, false)];
    for (const t of order) t.group = 0;
    this.list = order;
    this.shown = [keep];
    this.activeId = keep;
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
   * 此刻真的挂着编辑器的那些标签的路径。由 `docs.onEditorLive` 在编辑器
   * 挂载 / 销毁时写（认领规则在那边）。**不是响应式的**：只有 `audit` 读它。
   * 分屏之后可能有两个（每组一个）。
   *
   * 它和 `activeId` 不是一回事（issue #36）：`activeId` 一改，`{#key}` 要等
   * 下一次 flush 才销毁旧编辑器 —— 这中间旧标签仍然挂着编辑器、草稿还没交回，
   * 而新标签还没有编辑器。用 `activeId` 当「谁是活的」，正好在这一拍把两个都判错。
   */
  readonly livePaths = new Set<string>();

  /**
   * 在一个状态转换点上核一遍标签的不变量（issue #27，判据全在
   * `invariant.ts` 里，这里只负责「在哪些点上核」）。
   *
   * 第三个参数是「此刻哪些标签挂着活编辑器」：编辑器的 onChange 只改 `dirty`，
   * `draft` 要等 onStash 才回写，所以正在被编辑的那个标签本来就会
   * 短暂地 dirty 而无 draft —— 不告诉自检器这件事，它会在每次敲键盘时报假警。
   *
   * 原来传的是 `activeId`，于是「开标签」「切标签」这两个点上稳定报假警
   * （issue #36）：那一拍活的还是**上一个**标签。现在传的是真挂着编辑器的那些。
   */
  audit(where: string) {
    const live = new Set<number>();
    for (const p of this.livePaths) {
      const t = this.byPath(p);
      if (t) live.add(t.id);
    }
    audit(this.list, this.activeId, live, where, this.shown);
  }
}

export const tabs = new Tabs();
