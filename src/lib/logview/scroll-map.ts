/**
 * 行号 ⇄ 滚动位置映射。
 *
 * 坑：WebKit 的元素高度有上限（实测量级在 3000 万 px），而 1GB 日志约 800 万行，
 * 按 20px 行高算是 1.6 亿 px —— 直接设这个高度，滚动条会被悄悄截断，
 * 滚到后面行号全错。这是虚拟滚动做 GB 级数据的经典翻车点。
 *
 * 解法：行数超过阈值时，容器只给一个安全高度，scrollTop 与行号之间走比例映射。
 * 代价是滚动精度下降到「一像素跨多行」—— 但 800 万行本来就不可能像素级定位，
 * 精确跳转交给行号输入与搜索。
 */

/** 保守取值，远低于 WebKit 实际上限 */
export const MAX_SAFE_PX = 10_000_000;

export class ScrollMap {
  readonly lineHeight: number;
  private _lineCount = 0;

  constructor(lineHeight: number) {
    this.lineHeight = lineHeight;
  }

  set lineCount(n: number) {
    this._lineCount = Math.max(0, n);
  }
  get lineCount(): number {
    return this._lineCount;
  }

  /** 内容层实际要撑开的高度 */
  get scrollHeight(): number {
    return Math.min(this._lineCount * this.lineHeight, MAX_SAFE_PX);
  }

  /** 是否已进入压缩映射模式（行数太多，像素不够用了） */
  get compressed(): boolean {
    return this._lineCount * this.lineHeight > MAX_SAFE_PX;
  }

  /** scrollTop → 视口顶部对应的行号 */
  topLineAt(scrollTop: number, viewportHeight: number): number {
    if (this._lineCount === 0) return 0;
    if (!this.compressed) {
      return Math.floor(scrollTop / this.lineHeight);
    }
    // 压缩模式：可滚动行程按比例映射到行号空间
    const visible = Math.floor(viewportHeight / this.lineHeight);
    const maxTopLine = Math.max(0, this._lineCount - visible);
    const range = Math.max(1, this.scrollHeight - viewportHeight);
    const ratio = Math.min(1, Math.max(0, scrollTop / range));
    return Math.round(ratio * maxTopLine);
  }

  /** 行号 → scrollTop（用于「跳到某行」与 tail 吸底） */
  scrollTopFor(line: number, viewportHeight: number): number {
    if (this._lineCount === 0) return 0;
    if (!this.compressed) {
      return line * this.lineHeight;
    }
    const visible = Math.floor(viewportHeight / this.lineHeight);
    const maxTopLine = Math.max(1, this._lineCount - visible);
    const range = Math.max(1, this.scrollHeight - viewportHeight);
    return (Math.min(line, maxTopLine) / maxTopLine) * range;
  }
}
