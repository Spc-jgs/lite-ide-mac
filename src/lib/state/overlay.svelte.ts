import type { Sym } from "../editor/outline";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";

/**
 * 浮层的开合：随处搜索、文件结构大纲、快捷键速查、编码选择器、分支与工作树。
 *
 * 开合状态放 store 而不是放在渲染它们的 `Overlays.svelte` 里，是因为**开它们的人
 * 不在那个组件里**：键盘分派、菜单、导轨、状态栏、Git 栏、标题栏各有一条路进来。
 * 从 App.svelte 搬出来（issue #9 第 6 步），逻辑一个字不改。
 */
class Overlay {
  quickOpen = $state(false);
  quickScope = $state<"all" | "file" | "content" | "action">("all");
  /**
   * 打开随处搜索时预填的词。只有「在项目里找这个名字」会设它，
   * **每条打开浮层的路都要把它清掉** —— 不清的话，上次找过的名字
   * 会莫名其妙地出现在下一次 ⌘P 里。
   */
  quickSeed = $state("");

  outlineOpen = $state(false);
  /** 大纲浮层里点了一条，让编辑器重算一次符号 */
  outlineTick = $state(0);
  /** 编辑器报上来的符号表，大纲浮层读 */
  symbols = $state<Sym[]>([]);

  keysOpen = $state(false);
  encOpen = $state(false);
  /** 跳到行（⌘L）。只在有编辑器时有意义，开它的人自己判 */
  gotoOpen = $state(false);

  branchOpen = $state(false);
  /**
   * 分支浮层挂在挂件底下，所以要把挂件的位置一起交出去。
   *
   * **位置在打开之前就得定下来**，和 `git_fetch` 的 op_id 是同一条判据：
   * 凡是「先开始、后返回句柄」的东西，句柄必须早于用它的人。这里更直接 ——
   * 浮层渲染的那一帧就要知道往哪儿掉。
   */
  branchAnchor = $state<{ x: number; y: number } | null>(null);

  /** 开随处搜索。`seed` 不传就是清空 —— 见 `quickSeed` 的注释 */
  openQuick(scope: "all" | "file" | "content" | "action", seed = "") {
    this.quickScope = scope;
    this.quickSeed = seed;
    this.quickOpen = true;
  }

  openOutline() {
    if (tabs.active?.mode !== "edit") return;
    this.symbols = [];
    this.outlineTick++;
    this.outlineOpen = true;
  }

  /**
   * 「在项目里找这个名字」—— 跳转够不着时的退路。
   *
   * 它**不伪装成跳转**：拿光标处的词跑一次现成的全局搜索，结果照常列在
   * 搜索面板里让人自己挑。省掉的只是「选中、复制、⇧⌘F、粘贴」这四下，
   * 而不是给一个精度可疑的下划线。
   */
  findWordAtCursor() {
    const w = docs.wordUnderCursor();
    if (!w) {
      notify.ok("把光标放到一个名字上再按", 2000);
      return;
    }
    this.openQuick("content", w);
  }

  /**
   * 一律从挂件底下掉下来，**不管是谁开的**：点挂件、Git 栏里的分支行、
   * 菜单里的「分支与工作树」，三条路都读同一个元素的位置。
   * 三处各写各的话，从菜单开出来的那次就会掉在别的地方。
   * 挂件的元素在标题栏里，App 拿到它的矩形传进来。
   */
  openBranches(anchor: DOMRect | undefined) {
    this.branchAnchor = anchor ? { x: anchor.left, y: anchor.bottom + 4 } : null;
    this.branchOpen = true;
  }
}

export const overlay = new Overlay();
