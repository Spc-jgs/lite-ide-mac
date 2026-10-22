/**
 * 层级工具窗（docs/TAKEOVER.md G10，IDEA 的 Call Hierarchy ⌃⌥H / Type Hierarchy ⌃H）的状态：一棵懒展开的树。
 *
 * - **懒**：根一打开就问第一层；别的节点点开才问它的下一层（「谁调了它」往上追十层，每层几十个，全问一遍是几千次请求）
 * - **换方向就是换一棵树**：「谁调了它」和「它调了谁」答的是两个问题，展开过的状态不搬
 * - **递归不往下追**：一个节点和它的某个祖先是同一个方法（递归调用、互相调用），标「递归」、不让展开 —— 不然点几下就是无底洞
 * - **晚到的答案不用特意扔**：换方向 / 换根是换一棵新树，回来的那份写进的是旧树的节点，屏幕上已经不画它了
 *   （原来还有一道「代数对不上就扔」的检查，删了：把它去掉测试照样绿 —— 证明不了自己有用的防御代码不留）
 *
 * 只被懒块引（Java 的编辑器扩展、底部面板里的层级视图），不进入口
 */
import type { JavaHierNode } from "../ipc/commands";

export type HierDir = "in" | "out" | "super" | "sub";

export interface HNode {
  id: number;
  node: JavaHierNode;
  depth: number;
  open: boolean;
  /** null = 还没问过 */
  kids: HNode[] | null;
  /** 真个数（比 kids 多就是截断了，界面要说出来） */
  total: number;
  loading: boolean;
  error: string;
  /** 和某个祖先是同一个方法 / 类型：不让展开 */
  cycle: boolean;
  /** 祖先们的身份（判递归用） */
  path: string[];
}

/** 一个方法 / 类型的身份：同一个文件里同一个位置的同一个名字 */
export const keyOf = (n: JavaHierNode) => `${n.at.uri}#${n.at.line}:${n.at.character}#${n.name}`;

let seq = 0;
function mk(node: JavaHierNode, depth: number, path: string[]): HNode {
  return { id: ++seq, node, depth, open: false, kids: null, total: 0, loading: false, error: "", cycle: path.includes(keyOf(node)), path };
}

/** 看得见的那几行：展开了的往下摊 */
export function flatten(top: HNode | null): HNode[] {
  const out: HNode[] = [];
  const walk = (n: HNode) => {
    out.push(n);
    if (n.open && n.kids) for (const k of n.kids) walk(k);
  };
  if (top) walk(top);
  return out;
}

type Fetch = (root: string, raw: string, dir: HierDir) => Promise<{ items: JavaHierNode[]; total: number }>;

class Hierarchy {
  top = $state<HNode | null>(null);
  /** 类型层级（⌃H）；否则调用层级 */
  types = $state(false);
  dir = $state<HierDir>("in");
  /** 选中的那一行（节点 id） */
  cur = $state(0);
  rows = $derived(flatten(this.top));
  #root = "";
  /** 问下一层。默认走 jdtls；状态测试换成假的 */
  fetch: Fetch = async (root, raw, dir) => (await import("../ipc/java")).javaHierChildren(root, raw, dir);

  /** ⌃⌥H / ⌃H：以这个节点为根开一棵新树。调用层级默认看「谁调了它」、类型层级默认看「子类型」（接口上按 ⌃H 最常见的是找实现）——同 IDEA */
  open(root: string, node: JavaHierNode, types: boolean) {
    this.#root = root;
    this.types = types;
    this.dir = types ? "sub" : "in";
    this.#reset(node);
  }

  setDir(d: HierDir) {
    if (!this.top || d === this.dir) return;
    this.dir = d;
    this.#reset(this.top.node);
  }

  #reset(node: JavaHierNode) {
    this.top = mk(node, 0, []);
    this.cur = this.top.id;
    void this.expand(this.top);
  }

  /** 展开（第一次展开时问下一层）。`n` 要是从 `top` / `rows` 里拿到的那份（代理），异步回来改它才会重画（frontend.md 第一条） */
  async expand(n: HNode) {
    if (n.cycle) return;
    n.open = true;
    if (n.kids || n.loading) return;
    n.loading = true;
    n.error = "";
    try {
      const r = await this.fetch(this.#root, n.node.raw, this.dir);
      const path = [...n.path, keyOf(n.node)];
      n.kids = r.items.map((i) => mk(i, n.depth + 1, path));
      n.total = r.total;
    } catch (e) {
      n.error = e instanceof Error ? e.message : String(e);
    } finally {
      n.loading = false;
    }
  }

  collapse(n: HNode) {
    n.open = false;
  }

  /** 关掉工具窗之外的「清掉」：换项目时用 */
  clear() {
    this.top = null;
  }
}

export const hierarchy = new Hierarchy();
