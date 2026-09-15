import type { JumpHit } from "../editor/jump";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";
import { tabflow } from "./tabflow.svelte";
import { project } from "./project.svelte";

/**
 * 跳转与跳转历史：⌘B 落点、⌥⌘← / ⌥⌘→、搜索结果点击，以及给编辑器的「跳到某行」信号。
 *
 * 从 App.svelte 搬出来（issue #9 第 6a 步），逻辑一个字不改。
 */
interface NavSpot {
  path: string;
  line: number;
}

/** 上限。留着几百条既没人用，也让 localStorage 那份快照白胖一圈 */
const NAV_MAX = 50;

class Nav {
  /** 待跳转的行号（可带列）；带 nonce，连点同一条结果也能重新定位 */
  gotoLine = $state<{ line: number; col?: number; nonce: number } | null>(null);
  #gotoNonce = 0;

  /** 活动编辑器里光标的行:列。没有编辑器（日志 / 差异 / 空）时 null。状态栏那格读 */
  caret = $state<{ line: number; col: number } | null>(null);

  /**
   * 跳转历史。⌥⌘← 回去、⌥⌘→ 再回来（IDEA 的键位）。
   *
   * **跳出去回不来比不能跳更难受**，所以这两条和跳转本身是同一批东西，
   * 不是后续增强。
   *
   * 存的是「路径 + 行号」而不是标签 id：跳到的文件可能在中途被关掉，
   * 而按下 ⌥⌘← 的意思是「回到我刚才看的那个地方」，标签在不在无所谓。
   */
  back = $state<NavSpot[]>([]);
  fwd = $state<NavSpot[]>([]);

  /** 让编辑器跳到某一行（可带列，1-based） */
  goto(line: number, col?: number) {
    this.gotoLine = col === undefined ? { line, nonce: ++this.#gotoNonce } : { line, col, nonce: ++this.#gotoNonce };
  }

  /** 此刻在哪儿。`docs.posByPath` 里存的是编辑器最后报上来的光标行 */
  hereNow(): NavSpot | null {
    const t = tabs.active;
    if (!t) return null;
    return { path: t.path, line: docs.posByPath.get(t.path) ?? 1 };
  }

  /**
   * 搜索结果点击 / 跳转落点 / 历史回退：打开文件，带行号则跳过去。
   * 默认开成预览标签（issue #33 ⑯）—— 这几条路都是「看一眼」，沿着 ⌘B
   * 追十个文件不该留下十个标签。只有 ⌘P 按文件名开的传 false。
   */
  async openAt(path: string, line?: number, preview = true) {
    const full = path.startsWith("/") ? path : `${project.root ?? ""}/${path}`;
    await tabflow.openPath(full, { preview });
    if (line !== undefined) this.goto(line);
  }

  /**
   * 跳转落点。**先把当前位置压栈再走** —— 顺序反了的话，
   * 压进去的就是目的地，⌥⌘← 会把你留在原地。
   */
  async jumpTo(hit: JumpHit) {
    const from = this.hereNow();
    if (from) {
      this.back = [...this.back.slice(-(NAV_MAX - 1)), from];
      // 新的跳转让「前进」失效 —— 和浏览器一样，历史分叉时旧的那一支作废
      this.fwd = [];
    }
    const target = hit.target;
    if (target.rel === "") {
      // 本文件：不重新打开，直接跳行
      if (target.line !== undefined) this.goto(target.line);
      return;
    }
    await this.openAt(target.rel, target.line);
  }

  /** ⌥⌘← / ⌥⌘→。两条对称，合成一个函数免得两边漏改 */
  async go(dir: "back" | "fwd") {
    const from = dir === "back" ? this.back : this.fwd;
    if (from.length === 0) {
      notify.ok(dir === "back" ? "没有可回退的位置" : "没有可前进的位置", 1600);
      return;
    }
    const spot = from[from.length - 1];
    const here = this.hereNow();
    if (dir === "back") {
      this.back = this.back.slice(0, -1);
      if (here) this.fwd = [...this.fwd.slice(-(NAV_MAX - 1)), here];
    } else {
      this.fwd = this.fwd.slice(0, -1);
      if (here) this.back = [...this.back.slice(-(NAV_MAX - 1)), here];
    }
    await this.openAt(spot.path, spot.line);
  }
}

export const nav = new Nav();
