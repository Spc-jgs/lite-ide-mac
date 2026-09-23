import { writeText, readText, fileStamp, type Stamp } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { project } from "./project.svelte";
import { textToSave, settled, stashed } from "./doc";
import { scratchTitle } from "./frontmatter";
import { autosaveDue, AUTOSAVE_IDLE_MS } from "./autosave";
import type { TabState } from "./tab";

/** 冲突裁决、换编码 / 换行符在 `docs-ops.ts`，按需加载 —— 理由见那边的文件头 */
const ops = () => import("./docs-ops");

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
  /** 「读某个编辑器此刻的视口」的口子，按路径认领（同 `#live`）。快照要的是此刻，不是上次换行时 */
  readonly #viewProbe = new Map<string, () => ViewPos>();
  hooks: {
    /** 保存成功之后。App 装的是 refreshGit */
    afterSave?: () => void;
    /** 光标位置变了。App 装的是 scheduleSave —— `posByPath` 不是响应式的，没人替它触发 */
    afterPos?: () => void;
    /** 一份草稿自动落盘之后。侧边栏的草稿列表要刷「第一行」摘要 */
    afterAutosave?: (path: string) => void;
  } = {};

  /**
   * 挂载着的编辑器，以及从它们里面读实时文本的口子，按路径记。
   *
   * 分屏之前只可能有一个（编辑器是 `{#key tabs.active.id}` 包着的），分屏之后每组一个
   * （issue #35）。按路径**认领**：同一组切标签时新实例可能先挂、旧实例后卸，
   * 旧实例交回的那个 null 只删自己那条，不会把新实例的口子抹掉。
   * 一个文件只在一个组里（docs/SPLIT.md 第 2 节）是这里按路径键的前提。
   */
  readonly #live = new Map<string, () => string>();
  /** 「读出光标底下那个词」的口子。认领规则同 `#live` */
  readonly #wordProbe = new Map<string, () => string | null>();

  onEditorLive(path: string, get: (() => string) | null) {
    if (get) this.#live.set(path, get);
    else this.#live.delete(path);
    // 自检器要知道谁真的挂着编辑器（issue #36），和这里是同一份答案
    if (get) tabs.livePaths.add(path);
    else tabs.livePaths.delete(path);
  }

  onEditorView(path: string, get: (() => ViewPos) | null) {
    if (get) this.#viewProbe.set(path, get);
    else this.#viewProbe.delete(path);
  }

  /** 某个标签的视口：活着的编辑器给此刻的，别的给上次离开时记下的 */
  viewOf(t: TabState): ViewPos | undefined {
    const get = t.mode === "edit" ? this.#viewProbe.get(t.path) : undefined;
    return get ? get() : this.posByPath.get(t.path);
  }

  /** 只要行号的调用方（锚点、导航、工作树）用这个 */
  lineOf(path: string): number | undefined {
    return this.posByPath.get(path)?.line;
  }

  onEditorWordProbe(path: string, get: (() => string | null) | null) {
    if (get) this.#wordProbe.set(path, get);
    else this.#wordProbe.delete(path);
  }

  /** 活动标签的编辑器里光标底下那个词；没有编辑器或没在词上就是 null */
  wordUnderCursor(): string | null {
    const t = tabs.active;
    const get = t ? this.#wordProbe.get(t.path) : undefined;
    return get ? get() : null;
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
    const get = t.mode === "edit" ? this.#live.get(t.path) : undefined;
    return textToSave(t, get ? get() : null);
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
  async saveTab(tab: TabState, content: string, opts: { quiet?: boolean; to?: string } = {}): Promise<boolean> {
    if (tab.mode !== "edit") return false;
    try {
      // 保存返回新指纹，必须记下来，否则下次检查会把自己的保存当成外部修改。
      // `to`（另存为）：写到别处去，**写成了才换路径** —— 写失败时标签还指着原来那份
      const stamp = await writeText(opts.to ?? tab.path, content, tab.encoding, tab.bom, tab.eol);
      if (opts.to !== undefined && opts.to !== tab.path) {
        const pos = this.posByPath.get(tab.path);
        if (pos !== undefined) {
          this.posByPath.delete(tab.path);
          this.posByPath.set(opts.to, pos);
        }
        tab.path = opts.to;
        tab.name = opts.to.slice(opts.to.lastIndexOf("/") + 1);
        // 草稿毕业成了普通文件：标签栏不再显示第一行，显示文件名
        if (!project.isScratch(opts.to)) tab.title = undefined;
      }
      tab.stamp = stamp;
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
   * 把该自动存的都写进盘：草稿停手就存，项目文件只在「离开」时存。判据在 `autosave.ts`（纯函数，有测试）。
   *
   * 三个入口共用：停止输入半秒后的那次、App 里 4 秒一次的 tick（兜住
   * 「恢复出来就是脏的、之后一个字没敲」的草稿）、以及 `force` 的那几处
   * （失焦、切走、关闭、退出）。`leave`（窗口失焦、焦点进终端）连项目文件一起存 ——
   * 「改完去终端跑 mvn」时编译的得是改完的那份（autosave.ts 头上有来由）。
   *
   * 退出那次多半写不完 —— pagehide 是同步的，IPC 回不来进程就没了。
   * 那不是问题：会话快照已经把脏草稿 stash 住了，下次启动恢复成脏标签，
   * 4 秒 tick 一到就补上。判据里 `idleMs: Infinity` 那条就是给它的。
   */
  /**
   * 原生面板（另存为）开着的那段时间里，窗口失焦不算「离开」。
   *
   * 不压住的话：普通文件做另存为 → 面板一弹主窗口 resign key、WebView 收到 blur →
   * 离开就存把改动先写回**原文件** → 再存到新路径。人要的是原文件不动（code review 2026-09-23）。
   */
  #leaveMuted = 0;
  async muteLeave<T>(fn: () => Promise<T>): Promise<T> {
    this.#leaveMuted++;
    try {
      return await fn();
    } finally {
      this.#leaveMuted--;
    }
  }

  async autosaveSweep(force = false, leave = false) {
    // 另存为的面板开着时窗口也会失焦 —— 那一下不算「离开」，见 `muteLeave`
    if (this.#leaveMuted > 0) leave = false;
    const now = Date.now();
    const root = project.root;
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
        leave,
        inProject: !!root && tab.path.startsWith(`${root}/`),
        lossy: tab.lossy === true,
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
          const what = project.isScratch(tab.path) ? "草稿" : tab.name;
          notify.fail(`${what} 自动保存失败：${String(e)} —— 已保留在编辑器里，可 ⌘S 重试`, 6000);
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

  // ── 动作：转发到 docs-ops.ts（按需加载，入口包瘦身 2026-09-21）。签名和文档见那边 ──
  async resolveConflict(tab: TabState, take: "disk" | "mine") {
    return (await ops()).resolveConflict(tab, take);
  }
  async reopenWith(label: string) {
    return (await ops()).reopenWith(label);
  }
  async setEol(eol: "LF" | "CRLF") {
    return (await ops()).setEol(eol);
  }
  async saveAsEncoding(label: string, bom: boolean) {
    return (await ops()).saveAsEncoding(label, bom);
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
