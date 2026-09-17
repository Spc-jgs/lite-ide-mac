import { writeText, readText, fileStamp, type Stamp } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { project } from "./project.svelte";
import { textToSave, settled, stashed } from "./doc";
import { scratchTitle } from "./frontmatter";
import { autosaveDue, AUTOSAVE_IDLE_MS } from "./autosave";
import type { TabState } from "./tab";

/**
 * 文档生命周期：编辑器里那份文本和盘上那份之间的关系 ——
 * 保存、外部改动、冲突裁决、草稿回写、光标位置、换编码。
 *
 * 字段之间的**判据**在 `doc.ts`（纯函数，有测试）；这里是**流程**：谁在什么
 * 时候调那些判据、结果写回哪个标签、和 IPC 怎么接。从 App.svelte 搬出来
 * （issue #9 第 4c 步），逻辑一个字不改。
 *
 * # 两个往外的钩子
 *
 * 保存成功后要刷 git 状态（文件树的标记才跟得上），光标动了要安排存快照 ——
 * 这两件事的主人（git 动作、会话）还在 App 里，所以以 `hooks` 的形式让 App
 * 装上。**不用 `$effect` 盯 `savedTick`**：它在外部重读、冲突裁决时也会加一，
 * 那两种情况原来并不刷 git，改成 effect 就不是零功能改动了。
 */
class Docs {
  /**
   * 「磁盘那份成了准」的计数。编辑器用它对齐 dirty 基线 —— 保存成功、
   * 外部改动后重读、冲突时选「用磁盘上的」、换编码重开，都加一。
   */
  savedTick = $state(0);
  /**
   * 只在**我们自己**写盘成功时加一（`saveTab`）；外部重读、冲突选「用磁盘上的」不加。
   * 编辑器靠它分辨「initial 变了是因为我刚存的落盘了」和「盘上真的换了内容」——
   * 前者不能换文档（写盘期间人还在打字，换了就把那几个字吞掉），后者必须换。
   */
  selfSaveTick = $state(0);
  /**
   * 「把光标放进编辑器」的请求计数。挂载时靠 `autofocus`，**已经开着**的标签再被
   * 显式打开一次（⌘P、双击树、系统送进来同一个文件）时编辑器不会重建，
   * 靠这个计数让它把焦点收回去。Editor 挂载时先对齐一次，只对之后的变化响应
   * （累计计数器当 prop 的那条坑，见 frontend.md）。
   */
  focusTick = $state(0);
  focusEditor() {
    this.focusTick++;
  }

  /**
   * 每个文件上次停在第几行。
   *
   * **刻意不做成 `$state`**：它在编辑时每换一行就写一次，做成响应式等于
   * 每换行都惊动一次渲染，而界面上没有任何地方要显示它 —— 它只在存快照
   * 和恢复时被读。普通 Map 就够。
   */
  readonly posByPath = new Map<string, ViewPos>();
  /** 「读活动编辑器此刻的视口」的口子，认领规则同 `#live`。快照要的是此刻，不是上次换行时 */
  #viewProbe: { path: string; get: () => ViewPos } | null = null;
  /**
   * 还没兑现的恢复位置。标签被恢复出来时不能立刻跳 ——
   * 那时组件还没挂上。等它第一次成为活动标签再跳，跳完就从这里删掉，
   * 否则之后每次切回这个标签都会被拽回那一行。
   */
  readonly pendingPos = new Map<string, number>();

  hooks: {
    /** 保存成功之后。App 装的是 refreshGit */
    afterSave?: () => void;
    /** 光标位置变了。App 装的是 scheduleSave —— `posByPath` 不是响应式的，没人替它触发 */
    afterPos?: () => void;
    /** 一份草稿自动落盘之后。侧边栏的草稿列表要刷「第一行」摘要 */
    afterAutosave?: (path: string) => void;
  } = {};

  /**
   * 当前挂载着的那个编辑器，以及从它里面读实时文本的口子。
   *
   * 只可能有一个 —— 编辑器是 `{#key tabs.active.id}` 包着的，同一时刻只挂一个。
   * 记路径是为了**认领**：切标签时新实例可能先挂、旧实例后卸，
   * 旧实例交回的那个 null 不能把新实例的口子抹掉。
   */
  #live: { path: string; get: () => string } | null = null;
  /** 「读出光标底下那个词」的口子。认领规则同 `#live` */
  #wordProbe: { path: string; get: () => string | null } | null = null;

  onEditorLive(path: string, get: (() => string) | null) {
    if (get) this.#live = { path, get };
    else if (this.#live?.path === path) this.#live = null;
    // 自检器要知道谁真的挂着编辑器（issue #36），和这里是同一份答案
    tabs.livePath = this.#live?.path ?? null;
  }

  onEditorView(path: string, get: (() => ViewPos) | null) {
    if (get) this.#viewProbe = { path, get };
    else if (this.#viewProbe?.path === path) this.#viewProbe = null;
  }

  /** 某个标签的视口：活着的编辑器给此刻的，别的给上次离开时记下的 */
  viewOf(t: TabState): ViewPos | undefined {
    const p = this.#viewProbe;
    return p?.path === t.path && t.mode === "edit" ? p.get() : this.posByPath.get(t.path);
  }

  /** 只要行号的调用方（锚点、导航、工作树）用这个 */
  lineOf(path: string): number | undefined {
    return this.posByPath.get(path)?.line;
  }

  onEditorWordProbe(path: string, get: (() => string | null) | null) {
    if (get) this.#wordProbe = { path, get };
    else if (this.#wordProbe?.path === path) this.#wordProbe = null;
  }

  /** 活动标签的编辑器里光标底下那个词；没有编辑器或没在词上就是 null */
  wordUnderCursor(): string | null {
    const t = tabs.active;
    return t && this.#wordProbe?.path === t.path ? this.#wordProbe.get() : null;
  }

  /**
   * 编辑器交回来的实时文本 —— 换文件或销毁前调一次。
   *
   * **按 path 找标签，不能用 `active`**：这个回调发生在切标签之后，
   * 那时 `active` 已经是新的那个了，写回去就是把 A 的内容盖到 B 头上。
   */
  stashDraft(path: string, text: string) {
    const t = tabs.list.find((x) => x.path === path && x.mode === "edit");
    if (!t) return; // 标签已经被关掉了，草稿跟着作废
    Object.assign(t, stashed(t, text));
    this.retitle(t, text);
    // 编辑器刚交出草稿 = 人切走了。草稿在这一刻落盘，不等空闲期
    if (t.dirty && project.isScratch(path)) void this.autosaveSweep(true);
  }

  /**
   * 草稿的标签名跟着第一行走（`TabState.title`）。只对草稿做：项目文件的标签
   * 就该显示文件名，那是它在树里的身份。
   */
  retitle(t: TabState, text: string) {
    if (!project.isScratch(t.path)) return;
    t.title = scratchTitle(text);
  }

  /**
   * 这个标签当前该保存的文本。
   *
   * 编辑器还活着就以它为准 —— `draft` 只在换文件/销毁时回写一次，
   * `content` 是磁盘那份，两个都可能停在几步之前。
   * 判据和取值都在 `doc.ts` 里，那边有测试。
   */
  liveText(t: TabState): string {
    const live = this.#live;
    return textToSave(t, live?.path === t.path && t.mode === "edit" ? live.get() : null);
  }

  /** ⌘S 之外的保存入口（命令面板）。编辑器里的 ⌘S 走 CM6 自己的 keymap */
  saveActive() {
    const t = tabs.active;
    if (t?.mode === "edit") void this.save(this.liveText(t));
  }

  /**
   * 保存当前编辑标签。**返回是否真的写成了。**
   *
   * 以前是 `Promise<void>` 而错误在这里就被 notify 吃掉了，于是
   * 「保存并关闭」写成 `save(...).then(() => doClose(t))` —— 磁盘写失败
   * （满了、没权限、文件被外部删了）时它照样把标签关掉，改动当场就没。
   * 批量关闭把这条路走得多得多，所以先把成败传出去。
   */
  async save(content: string): Promise<boolean> {
    const tab = tabs.active;
    if (!tab) return false;
    return this.saveTab(tab, content);
  }

  /**
   * 把 `content` 写进 `tab` 的路径。`save` 和自动保存都走这里 ——
   * 写盘、记指纹、清草稿、清冲突这一串只能有一份。
   *
   * `quiet`（自动保存）只少两样：不弹「已保存」、不刷 git。草稿不在仓库里，
   * 刷了也是白跑一次子进程；而每半秒闪一次「已保存」就是把隐形的事变成噪音。
   * 失败照样要说 —— 但由调用方决定说几次（见 `autosaveSweep`）。
   */
  async saveTab(tab: TabState, content: string, opts: { quiet?: boolean } = {}): Promise<boolean> {
    if (tab.mode !== "edit") return false;
    try {
      // 保存返回新指纹，必须记下来，否则下次检查会把自己的保存当成外部修改
      tab.stamp = await writeText(tab.path, content, tab.encoding, tab.bom, tab.eol);
      // 磁盘那份成了准。草稿一起清掉 —— 三处「读回磁盘」共用 settled 这一个出口，
      // 原来各写一遍，其中一处漏了清草稿（见 doc.ts 的注释）
      Object.assign(tab, settled(content));
      tab.conflict = false;
      tab.saveFailed = false;
      this.selfSaveTick++;
      this.savedTick++;
      this.retitle(tab, content);
      if (opts.quiet) return true;
      notify.ok(`已保存 ${tab.name}`, 1800);
      // 保存八成改变了 git 状态，顺手刷一下，文件树的标记才跟得上
      this.hooks.afterSave?.();
      return true;
    } catch (e) {
      // 自动保存的失败由调用方决定说几次（见 `autosaveSweep`），这里原样抛出去
      if (opts.quiet) throw e;
      notify.fail(String(e));
      return false;
    }
  }

  // ─────────────── 草稿自动保存（issue #40 第一层） ───────────────

  /** 每个路径上次输入的时刻。普通 Map，理由同 `posByPath`：没人要显示它 */
  readonly #lastEdit = new Map<string, number>();
  /** 上次写失败的时刻，退避用；写成功就删 */
  readonly #lastFail = new Map<string, number>();
  /** 已经为写失败说过一次的路径 —— 同一份草稿连续失败只说一次 */
  readonly #warnedFail = new Set<string>();
  #autosaveTimer: ReturnType<typeof setTimeout> | null = null;
  /** 正在写的路径。写盘是 await 的，半秒内第二次扫到它不能再发一次 */
  readonly #saving = new Set<string>();

  /**
   * 编辑器报「文档变了」时叫一次。只记时刻并安排一次扫描 ——
   * 判据（草稿？脏？空闲够久？）在扫描里统一算，这里不判。
   */
  noteEdit(path: string) {
    this.#lastEdit.set(path, Date.now());
    if (this.#autosaveTimer) clearTimeout(this.#autosaveTimer);
    this.#autosaveTimer = setTimeout(() => {
      this.#autosaveTimer = null;
      void this.autosaveSweep();
    }, AUTOSAVE_IDLE_MS);
  }

  /**
   * 把该存的草稿都写进盘。判据在 `autosave.ts`（纯函数，有测试）。
   *
   * 三个入口共用：停止输入半秒后的那次、App 里 4 秒一次的 tick（兜住
   * 「恢复出来就是脏的、之后一个字没敲」的草稿）、以及 `force` 的那几处
   * （失焦、切走、关闭、退出）。
   *
   * 退出那次多半写不完 —— pagehide 是同步的，IPC 回不来进程就没了。
   * 那不是问题：会话快照已经把脏草稿 stash 住了，下次启动恢复成脏标签，
   * 4 秒 tick 一到就补上。判据里 `idleMs: Infinity` 那条就是给它的。
   */
  async autosaveSweep(force = false) {
    const now = Date.now();
    for (const tab of tabs.list) {
      if (this.#saving.has(tab.path)) continue;
      const edited = this.#lastEdit.get(tab.path);
      const failed = this.#lastFail.get(tab.path);
      const due = autosaveDue({
        scratch: project.isScratch(tab.path),
        editing: tab.mode === "edit",
        dirty: tab.dirty,
        conflict: tab.conflict === true,
        idleMs: edited === undefined ? Infinity : now - edited,
        failedMs: failed === undefined ? null : now - failed,
        force,
      });
      if (!due) continue;
      this.#saving.add(tab.path);
      try {
        await this.saveTab(tab, this.liveText(tab), { quiet: true });
        this.#lastFail.delete(tab.path);
        this.#warnedFail.delete(tab.path);
        this.hooks.afterAutosave?.(tab.path);
      } catch (e) {
        this.#lastFail.set(tab.path, Date.now());
        // 常驻的那个提醒：标签圆点转警示色、状态栏写「⌘S 重试」，直到写成功
        tab.saveFailed = true;
        // 说一次就够：盘满、没权限不会自己好，半秒一条红字只会把别的消息淹掉。
        // 标签留在脏状态，圆点还亮着，⌘S 那条路照常兜底
        if (!this.#warnedFail.has(tab.path)) {
          this.#warnedFail.add(tab.path);
          notify.fail(`草稿自动保存失败：${String(e)} —— 已保留在编辑器里，可 ⌘S 重试`, 6000);
        }
      } finally {
        this.#saving.delete(tab.path);
      }
    }
  }

  /**
   * 关草稿标签之前的那一次：写成了返回 true（可以直接关），写不成返回 false
   * （那时是真的有东西会丢，走现有的「保存并关闭 / 丢弃」确认）。
   * 非草稿一律 false —— 它们从来就该问。
   */
  async autosaveBeforeClose(tab: TabState): Promise<boolean> {
    if (!project.isScratch(tab.path) || tab.mode !== "edit" || !tab.dirty) return false;
    if (tab.conflict) return false;
    try {
      await this.saveTab(tab, this.liveText(tab), { quiet: true });
      this.hooks.afterAutosave?.(tab.path);
      return true;
    } catch {
      return false;
    }
  }

  /**
   * 检查打开的编辑标签是否被外部改动。
   *
   * 时机选在窗口获得焦点时 —— 用户从别处切回来才是他关心这件事的时刻，
   * 也不必为了这个常年跑一个轮询。另配一个 10 秒的兜底轮询，
   * 应付「一直没离开窗口但文件被后台进程改了」的情况。（两个时机都在 App 里）
   */
  async checkExternalChanges() {
    for (const tab of tabs.list) {
      if (tab.mode !== "edit") continue;
      let now: Stamp;
      try {
        now = await fileStamp(tab.path);
      } catch {
        // 文件没了或读不到：不打扰，用户保存时自然会报错
        continue;
      }
      const before = tab.stamp;
      if (!before || (before.mtimeMs === now.mtimeMs && before.size === now.size)) continue;

      if (tab.dirty) {
        // 两边都改了，只能让用户裁决
        tab.conflict = true;
        tab.stamp = now;
      } else {
        // 本地没动过，直接跟上外部的版本 —— 这是最常见也最无害的情况
        try {
          // 沿用已知编码重读，不重新探测 —— 文件只是内容变了，编码没道理换
          const t = await readText(tab.path, tab.encoding);
          Object.assign(tab, settled(t.content));
          tab.lossy = t.lossy;
          tab.eol = t.eol;
          tab.stamp = now;
          this.savedTick++;
          notify.ok(`${tab.name} 已被外部修改，已重新加载`, 2600);
        } catch (e) {
          notify.fail(String(e));
        }
      }
    }
  }

  async resolveConflict(tab: TabState, take: "disk" | "mine") {
    tab.conflict = false;
    if (take === "disk") {
      try {
        const t = await readText(tab.path, tab.encoding);
        Object.assign(tab, settled(t.content));
        tab.eol = t.eol;
        tab.stamp = await fileStamp(tab.path);
        this.savedTick++;
      } catch (e) {
        notify.fail(String(e));
      }
    }
    // take === "mine"：什么都不做，保留编辑器里的内容，
    // 下次 ⌘S 会覆盖磁盘 —— 指纹已经更新过，不会再重复告警
  }

  /** 按新编码重新解码当前文件 */
  async reopenWith(label: string) {
    const tab = tabs.active;
    if (!tab) return;
    try {
      if (tab.mode === "log") {
        // 日志模式只是换个 TextDecoder 标签，不用重开句柄
        tab.encoding = label;
        return;
      }
      if (tab.dirty) {
        notify.fail("有未保存的改动，请先保存（⌘S）再换编码重新打开", 3000);
        return;
      }
      const t = await readText(tab.path, label);
      tab.content = t.content;
      tab.encoding = t.encoding;
      tab.bom = t.bom;
      tab.lossy = t.lossy;
      tab.eol = t.eol;
      this.savedTick++;
      notify.ok(`已按 ${t.encoding} 重新打开${t.lossy ? "（仍有解不出的字节）" : ""}`, 3000);
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /** 只改「将来存成什么编码」，不动当前内容 */
  saveAsEncoding(label: string, bom: boolean) {
    const tab = tabs.active;
    if (!tab || tab.mode !== "edit") return;
    tab.encoding = label;
    tab.bom = bom;
    // 内容没变但目标编码变了，得让用户知道要按 ⌘S 才会真的落盘
    tab.dirty = true;
    tabs.keep(tab.id);
    notify.ok(`下次保存将写成 ${label}${bom ? " + BOM" : ""}，按 ⌘S 生效`, 3600);
  }

  /**
   * 记下某个文件当前停在哪一行。
   *
   * **必须自己叫 `afterPos`**（App 装的是 `scheduleSave`），不能指望存快照的
   * effect —— `posByPath` 是普通 Map（故意的，见它的声明），改它不产生任何信号。
   * 少了这一句，「开文件 → 滚到第 5000 行 → 退出」这条最典型的路径
   * 就什么都没存下来，而快照看着还挺正常，最难查。
   */
  markPos(path: string, line: number) {
    if (line < 1) return;
    // 只记行：列和视口这时已经不准了，留着会在恢复时把人摆到错的地方。
    // 精确的那份由编辑器销毁时 `markView` 交回来，或者快照时从 `viewOf` 现读
    this.posByPath.set(path, { line });
    this.hooks.afterPos?.();
  }

  /**
   * 编辑器销毁（切标签、关标签）时交回来的完整视口（2026-09-17）。
   * 编辑器是 `{#key active.id}` 包着的，切走就销毁 —— 原来只有行号，切回来
   * 光标回到第一行、滚动条回到顶上，每切一次标签就丢一次「我看到哪儿了」。
   */
  markView(path: string, pos: ViewPos) {
    if (pos.line < 1) return;
    this.posByPath.set(path, pos);
    this.hooks.afterPos?.();
  }
}

export const docs = new Docs();

/**
 * 一个编辑标签的视口：光标在哪、视口顶上是哪一行、那一行露出多少。
 * `line` 必有；其余可选 —— 只记了行的（老快照、换行时的粗记录）恢复时把那一行居中。
 */
export interface ViewPos {
  line: number;
  col?: number;
  /** 视口顶上那一行（1-based）；`toff` 是它被滚过去的像素数（折行的长行才会大于 0） */
  top?: number;
  toff?: number;
}
