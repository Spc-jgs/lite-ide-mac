import type { Stamp } from "../ipc/commands";

/**
 * 一个打开的标签。
 *
 * 从 App.svelte 搬出来（issue #9 第 3 步）：状态栏要按它渲染，而组件里
 * 声明的 interface 别的文件引不到。只有类型，没有运行时 —— 标签表本身
 * （开 / 关 / 切 / 草稿）还在 App 里，那是第 4 步的事。
 */
export interface TabState {
  id: number;
  path: string;
  name: string;
  mode: "edit" | "log" | "diff" | "merge";
  dirty: boolean;
  /** log 模式的引擎句柄 */
  handle?: number;
  /** edit 模式打开时的磁盘内容 —— **dirty 的基线**，不是编辑器里的实时文本 */
  content?: string;
  /**
   * 未保存的实时文本。只有改过才有。
   *
   * 为什么要单独存一份：编辑器是 `{#key active.id}` 包着的，切标签就销毁重建，
   * 而重建时拿的是这里的字段。以前只有 `content` 一个字段，编辑器里的改动
   * 从来没回写过 —— 切走再切回来，改动和「有未保存改动」的标记**一起**消失，
   * 人完全察觉不到自己丢了东西。
   *
   * 存两份而不是一份，是因为 dirty 要靠「实时文本 ≠ 磁盘那份」算出来；
   * 只留一个字段的话基线会被草稿顶掉，标记就再也亮不起来了。
   */
  draft?: string;
  /** 被判为 log 模式的原因 */
  reason?: string;
  /** 文件字节数，用于判断切到编辑模式是否有风险 */
  size: number;
  /** 用户手动指定过模式；自动判定只是默认值，不该是死判决 */
  forced?: "edit" | "log";
  /** 打开或保存时的文件指纹，用来发现外部改动 */
  stamp?: Stamp;
  /** 外部改动了，但本地也有未保存改动 —— 需要用户裁决 */
  conflict?: boolean;
  /** 差异标签：相对仓库根的路径 */
  rel?: string;
  /** 看的是暂存区还是工作区 */
  diffStaged?: boolean;
  diffUntracked?: boolean;
  diffRaw?: string;
  /** 差异被 Rust 侧的 1MB 上限掐断了，界面要说出来 */
  diffCapped?: boolean;
  /** 非空表示这是「某次提交里的差异」，只读历史，不是工作区 */
  diffSha?: string;
  diffShort?: string;
  /** 冲突标签：带冲突标记的工作区原文 */
  mergeText?: string;
  /**
   * 文件编码标签（WHATWG，如 `UTF-8` / `GBK`）。
   * 读进来是什么就用什么存回去 —— 保存不该顺手改变文件的编码。
   */
  encoding?: string;
  bom?: boolean;
  /** 盘上的换行符（`LF` / `CRLF` / `CR` / `mixed`），保存时原样写回；理由同 `encoding` */
  eol?: string;
  /** 解码时有解不出的字节；带着它保存会把那些字节永久换成 U+FFFD */
  lossy?: boolean;
  /**
   * 预览标签（VS Code 的 preview editor）：单击树、搜索结果、⌘B 跳过去的
   * 都是「看一眼」，不该每看一个就永久占一格。**同时最多一个**，再开一个
   * 预览就把它顶掉；改了内容、双击标签或树、显式打开（⌘P、拖进来）都会
   * 把它**保留**下来（`tabs.keep`）—— 保留是单向的，保存之后不会退回预览。
   * 「保留」不是「钉住」：钉住是下面那个 `pinned`，另一件事。
   * 名字斜体显示，除此之外和普通标签没有区别。
   */
  preview?: boolean;
  /**
   * 钉住的标签（issue #33 ⑰）：排在最左、⌘W 和「关闭其他 / 右侧 / 全部」都跳过它，
   * ✕ 的位置换成图钉（点了取消钉住）。右键「关闭」仍然能关 —— 钉住防的是误关，
   * 不是不许关。钉住和预览互斥：钉的时候顺手保留。
   */
  pinned?: boolean;
  /**
   * 自动换行的手动开关（视图 → 自动换行）。没设过就按 `wrapsByDefault(path)`。
   * 不进会话快照：它是「这一次看着不顺手切一下」的东西，不是文件的属性。
   */
  wrap?: boolean;
  /**
   * 标签栏上显示的名字，没有就显示 `name`。草稿用它显示第一行（`2026-09-16 1103.md`
   * 在标签栏上什么都说明不了，「周会要点」才是人记得住的）—— Sublime 的 untitled
   * 也是这么做的。由 `docs` 在草稿落盘 / 交回草稿时更新。
   */
  title?: string;
}

/**
 * 这个文件默认要不要软换行。
 *
 * 笔记和纯文本按段落写，一行几百字不换行就得横着滚 —— 那是记笔记最不能忍的一种
 * 「鸡肋」。代码不换行：缩进结构靠对齐，折行会把它揉乱。判据只看扩展名：
 * markdown、txt、log、没有扩展名的（README、NOTES）算文本，其余算代码。
 * 不走 `langs.ts`（它是懒的），一个正则就够。
 */
export function wrapsByDefault(path: string): boolean {
  const name = path.slice(path.lastIndexOf("/") + 1);
  if (/\.(md|markdown|mdx|txt|text|log|rst|adoc)$/i.test(name)) return true;
  return !/\.[^.]+$/.test(name);
}

/** `tabPath` 是不是 `p` 本身，或（`p` 是目录时）在它底下 */
export function underPath(tabPath: string, p: string, isDir: boolean): boolean {
  return tabPath === p || (isDir && tabPath.startsWith(`${p}/`));
}
