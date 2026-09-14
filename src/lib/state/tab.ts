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
  /** 解码时有解不出的字节；带着它保存会把那些字节永久换成 U+FFFD */
  lossy?: boolean;
}
