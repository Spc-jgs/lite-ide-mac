/**
 * 文件树：把「展开着的目录树」拍平成一行一行。纯函数，零运行时 import。
 *
 * 从 FileTree.svelte 的 `rows` 搬出来（2026-09-24），为的是接着给它加「合并单层目录 / 包名」。
 * 分工是这样的：
 *
 * - **数据**（`children` + `expanded`）只存磁盘上的真相，key 全是真实路径 ——
 *   新建、改名、拖动、废纸篓、git 着色、定位当前文件都落在真实路径上；
 * - **行长什么样**全在这一步推导。
 *
 * 所以合并、点号这些「显示上的变形」只许在这里做，不许存回数据里。
 */
import type { DirEntry } from "../ipc/commands";

/**
 * 扁平化渲染：把展开的树拍平成一个带 depth 的列表，而不是递归组件。
 * 渲染就是一个 each，将来要给大仓库加虚拟滚动也直接可用。
 */
export interface Row {
  /** 这一行对应的**那个**真实条目的名字（合并行是最深那一段）—— 重命名的默认值就是它 */
  name: string;
  /** 画出来的字：普通行同 `name`；合并行是整串，`com.demo.order` 或 `src/main` */
  label: string;
  /** 真实路径。合并行是最深那一层 —— 展开、新建、拖进来、重命名、删除都落在它上面 */
  path: string;
  /** 这一行盖住的所有真实路径，从上到下，最后一个就是 `path`。普通行只有它自己 */
  chain: string[];
  isDir: boolean;
  depth: number;
  /** 生成物目录：压暗、不自动展开。**它在树里、点得开**（issue #13） */
  generated: boolean;
  /** JVM 的源码根（`src/main/java` 那种）：它以下的包名用点连 */
  srcRoot: boolean;
  /** 目录在项目里是什么角色 —— 决定画哪个文件夹图标（照 IDEA）。文件恒为 `folder`，不用它 */
  dirKind: DirKind;
}

/**
 * 目录的角色，照 IDEA 项目视图的图标分：源码根蓝、测试根绿、资源根带标、包、生成物（IDEA 叫 excluded）。
 * 判据全是路径约定（Maven / Gradle），不读 pom.xml —— 改了 `sourceDirectory` 的项目认不出，
 * 那种项目少，认不出也只是图标普通一点。
 */
export type DirKind = "folder" | "source" | "test" | "resources" | "testResources" | "package" | "excluded";

export interface TreeInput {
  root: string;
  /** path → 子项。未加载过的目录不在表里 */
  children: ReadonlyMap<string, readonly DirEntry[]>;
  expanded: ReadonlySet<string>;
  /** 这一条**自己**要不要压暗 —— 判据要看 git 的答案，留在 FileTree 里（见那边的 `dims`） */
  dims: (it: DirEntry) => boolean;
  /** 合并单层目录（IDEA 的 Compact Middle Packages）。关掉就是一层一行 */
  compact: boolean;
}

/**
 * JVM 的源码根：Maven / Gradle 约定的 `src/<源码集>/<语言>`。
 *
 * **语言知识只有这一张表**，文件树组件一个语言都不认识。只有「目录名就是包名」的语言才进来 ——
 * Python 的包也对应目录，但能被合并的那几层里恰好没有 `__init__.py`，写成点号反而误导；
 * Go、前端没有这个概念。它们照样能合并，只是用 `/` 连。加一门语言就是往这里加一个名字。
 *
 * 源码集名字不写死 `main|test`：Gradle 的 `integrationTest`、Android 的 `debug` 都是同一个约定。
 */
const SOURCE_ROOT = /\/src\/[^/]+\/(java|kotlin|scala|groovy)$/;

export const isSourceRoot = (path: string) => SOURCE_ROOT.test(path);

/** 源码集名里带 test 的算测试（`test`、`integrationTest`、`testFixtures`） */
const TEST_SET = /\/src\/[^/]*test[^/]*\/[^/]+$/i;
const RESOURCES_ROOT = /\/src\/[^/]+\/resources$/;

/**
 * 这个目录画哪种文件夹。`own` 是它**自己**被判成生成物（不是继承来的）——
 * IDEA 只给 `target/` 本身画排除图标，里面的子目录是普通文件夹（压暗是另一回事，跟着整棵子树走）。
 */
export function dirKindOf(path: string, inSrc: boolean, ownGenerated: boolean): DirKind {
  if (ownGenerated) return "excluded";
  const test = TEST_SET.test(path);
  if (SOURCE_ROOT.test(path)) return test ? "test" : "source";
  if (RESOURCES_ROOT.test(path)) return test ? "testResources" : "resources";
  return inSrc ? "package" : "folder";
}

/**
 * 深度优先展开成扁平列表。
 *
 * `inGen` 往下传：**生成物是整棵子树的性质，不是那一行的性质**。
 * 只标记顶上那一行的话，展开 `node_modules/` 往下滚两屏，那行早就滚没了 ——
 * 剩下的是一片看着和自己代码一模一样的东西。
 *
 * `inSrc` 同理往下传：在源码根底下，合并行用点连（`com.demo.order`），别处用 `/`。
 *
 * # 合并（`compact`）
 *
 * 一个目录往下只有一个子目录、别的什么都没有，就和那个子目录并成一行 —— 链是 Rust 侧读好的
 * （`DirEntry.chain`），这里只决定怎么并：
 *
 * - **源码根自己占一行，链在它前面断开**：`main` 底下只有 `java` 时是 `main` 一行、`java` 一行，
 *   不并成 `main/java` —— IDEA 就是这么画的，源码根是一个有意义的边界；
 * - 合并行展开的是**最深那一层**，子项从它底下接着画；
 * - 压暗的（生成物）不并 —— Rust 侧本来也不给它们探链，这里再守一道。
 */
export function flatten({ root, children, expanded, dims, compact }: TreeInput): Row[] {
  const out: Row[] = [];
  const walk = (dir: string, depth: number, inGen: boolean, inSrc: boolean) => {
    const items = children.get(dir);
    if (!items) return;
    for (const it of items) {
      const own = dims(it);
      const gen = inGen || own;
      const names = [it.name];
      const chain = [it.path];
      if (compact && it.isDir && !gen && !isSourceRoot(it.path)) {
        for (const c of it.chain ?? []) {
          const next = `${chain[chain.length - 1]}/${c}`;
          if (isSourceRoot(next)) break;
          names.push(c);
          chain.push(next);
        }
      }
      const path = chain[chain.length - 1];
      const srcRoot = it.isDir && isSourceRoot(path);
      out.push({
        name: names[names.length - 1],
        label: names.join(inSrc ? "." : "/"),
        path,
        chain,
        isDir: it.isDir,
        depth,
        generated: gen,
        srcRoot,
        dirKind: it.isDir ? dirKindOf(path, inSrc, own) : "folder",
      });
      if (it.isDir && expanded.has(path)) walk(path, depth + 1, gen, inSrc || srcRoot);
    }
  };
  walk(root, 0, false, false);
  return out;
}

// ─────────────────── 打字定位（issue #33 ⑧）的匹配 ───────────────────
// 从 FileTree.svelte 搬出来（2026-09-24）。`q` 是已经小写化的那串字（组件里的 `speedLower`）。

/** 名字里命中的那一段 [起, 止)，没命中 null。大小写不敏感 */
export function speedHit(name: string, q: string): [number, number] | null {
  if (q === "") return null;
  const k = name.toLowerCase().indexOf(q);
  return k < 0 ? null : [k, k + q.length];
}

/** 从 `from` 起（含）往 `dir` 方向找下一个命中的行，绕圈；没有给 -1。按画出来的字（`label`）找 */
export function speedNext(rows: readonly { label: string }[], from: number, dir: 1 | -1, q: string): number {
  const n = rows.length;
  for (let k = 0; k < n; k++) {
    const j = (((from + dir * k) % n) + n) % n;
    if (speedHit(rows[j].label, q)) return j;
  }
  return -1;
}
