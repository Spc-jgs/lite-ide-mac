import { writeText, readText, fileStamp, type Stamp } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { textToSave, settled, stashed } from "./doc";
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
   * 每个文件上次停在第几行。
   *
   * **刻意不做成 `$state`**：它在编辑时每换一行就写一次，做成响应式等于
   * 每换行都惊动一次渲染，而界面上没有任何地方要显示它 —— 它只在存快照
   * 和恢复时被读。普通 Map 就够。
   */
  readonly posByPath = new Map<string, number>();
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
    if (!tab || tab.mode !== "edit") return false;
    try {
      // 保存返回新指纹，必须记下来，否则下次检查会把自己的保存当成外部修改
      tab.stamp = await writeText(tab.path, content, tab.encoding, tab.bom, tab.eol);
      // 磁盘那份成了准。草稿一起清掉 —— 三处「读回磁盘」共用 settled 这一个出口，
      // 原来各写一遍，其中一处漏了清草稿（见 doc.ts 的注释）
      Object.assign(tab, settled(content));
      tab.conflict = false;
      this.savedTick++;
      notify.ok(`已保存 ${tab.name}`, 1800);
      // 保存八成改变了 git 状态，顺手刷一下，文件树的标记才跟得上
      this.hooks.afterSave?.();
      return true;
    } catch (e) {
      notify.fail(String(e));
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
    tabs.pin(tab.id);
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
    this.posByPath.set(path, line);
    this.hooks.afterPos?.();
  }
}

export const docs = new Docs();
