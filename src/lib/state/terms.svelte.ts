import { layout } from "./layout.svelte";

/**
 * 底部终端列表：开了哪几个 shell、哪个在前面。
 *
 * 原来是 App.svelte 里的三个变量加两个函数，面板头、面板体、菜单命令、
 * 菜单栏灰显、预算行都直接碰它们（issue #9）。搬出来之后面板组件自己读写，
 * App 只剩菜单命令那两行。写法同 `layout` / `notify`：class 上的 `$state` 字段。
 *
 * **不存进快照。** pty 是活的子进程，跨进程恢复不了，假装能恢复只会更糟
 * （`session.ts` 文件头那条）。
 */
export interface TermTab {
  id: number;
  /** 工作目录在创建时快照一次，之后不跟着 root 走 */
  cwd: string;
  title: string;
}

class Terms {
  /**
   * 多个终端并存。切换标签时**不能卸载**未激活的那些 ——
   * 组件一销毁 Session 就 drop，shell 直接被 kill，正在跑的命令全没了。
   * 所以面板那边用 CSS 隐藏，实例一直活着。
   */
  list = $state<TermTab[]>([]);
  activeId = $state<number | null>(null);
  #nextId = 1;

  /** 新开一个，落在 `dir`，并把面板亮出来 */
  open(dir: string): TermTab {
    const base = dir === "~" ? "~" : dir.slice(dir.lastIndexOf("/") + 1) || dir;
    /*
     * 重名要带序号。终端的标题取自工作目录名，而绝大多数时候几个终端开的
     * 是**同一个**目录（项目根）—— 于是三个标签页全写着 `proj`，
     * 标签栏和「全部终端」下拉都变成「随便点一个」。
     */
    let title = base;
    for (let n = 2; this.list.some((t) => t.title === title); n++) title = `${base} (${n})`;
    const t: TermTab = { id: this.#nextId++, cwd: dir, title };
    this.list = [...this.list, t];
    this.activeId = t.id;
    layout.panel = true;
    return t;
  }

  close(id: number) {
    const idx = this.list.findIndex((t) => t.id === id);
    this.list = this.list.filter((t) => t.id !== id);
    if (this.activeId === id) {
      this.activeId = this.list[Math.min(idx, this.list.length - 1)]?.id ?? null;
    }
    // 最后一个终端关掉就把面板一起收起，省得留个空壳
    if (this.list.length === 0) layout.panel = false;
  }

  /** 关掉当前之外的。先拷一份再遍历 —— `close` 会改 `list` */
  closeOthers() {
    const keep = this.activeId;
    for (const t of [...this.list]) if (t.id !== keep) this.close(t.id);
  }

  closeAll() {
    for (const t of [...this.list]) this.close(t.id);
  }
}

export const terms = new Terms();
