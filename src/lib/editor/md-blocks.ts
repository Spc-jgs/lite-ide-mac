/**
 * 让 markdown 解析器把日志段 / diff 段当成**不做 inline 解析的叶子块**。
 *
 * # 为什么要有这个（2026-09-17 量出来的）
 *
 * 粘 2000 行日志进草稿之后，每敲一个键要 ~20ms —— JS 自采样看到的热点全是
 * `@lezer/markdown` 的 `parseInline`：日志行之间没有空行，在 markdown 眼里那是
 * **一个 200KB 的 Paragraph**，改动只要碰到它（在里面打、或离它 128 字符以内），
 * 整段 inline 语法（链接、强调、转义……）重解析一遍。`@codemirror/language`
 * 在事务里同步解析的预算正好是 20ms，所以每键不多不少 20ms，剩下的在空闲帧里
 * 继续烧，下一键又来。把光标挪到日志上方 600 字符外，回到 1ms。
 *
 * 这里定义两种块节点 `LogBlock` / `DiffBlock`：块级解析器认出连续的日志行 / diff 行，
 * 整段产一个没有子节点的叶子 —— 不进 `parseInline`，改一个字重扫的只是几千次行首正则。
 *
 * # 只管解析成本，不管段的边界
 *
 * 着色和段头仍然以 `log-segments.ts` 的 `findLogSegments` / `findDiffSegments` 为准
 * （它们能跨空行、要求至少三行）。这里的块可以和那边的段**不完全一致**：块级解析器
 * 只能往前看一行（`peekLine`），所以「两行像日志」也会成一个块 —— 后果只是那两行
 * 不做 markdown 渲染，看起来是纯文本。反过来，段里夹的空行会把块切成两个，
 * 对着色没影响（段不是从树上读的）。两边共用同一批判据函数（`formatOfLine` /
 * `isStackLine` / `diffLineKind`），不会认成不同的东西。
 *
 * # 为什么 diff 必须是「抢先」的块解析器
 *
 * 删除行 `-    foo` 和新增行 `+ bar` 在 markdown 眼里是列表项 —— 列表能打断段落，
 * 用「段落式叶子」（像 GFM Table 那样的 `LeafBlockParser`）做的话，diff 走到第一个
 * 这样的行就被列表截走了。抢先的块解析器排在所有内置解析器**之前**跑，自己把整段
 * 吃掉，列表就轮不到。日志用同一条路，图个一致。
 *
 * # 类型
 *
 * 不从 `@lezer/markdown` 导入类型：它不是直接依赖（pnpm 下解析不到），而为了几个
 * 类型名加一条依赖不值。下面按用到的成员写了最小的结构类型，`markdown({ extensions })`
 * 按结构检查，对得上就行。
 *
 * # 判据从外面传进来
 *
 * 这个文件**没有相对导入**，为的是能被 node 直跑的测试拿一个假的 `Cx` 验
 * （tests/md-blocks.test.ts）—— 仓库里被这么测的模块一个相对值导入都没有。
 * 真判据（`formatOfLine` / `isStackLine` / `diffLineKind`）在 `log-segments.ts` 里绑。
 */

export interface Elt {}
interface Line {
  text: string;
  /** 越过列表 / 引用标记之后、**保留缩进**的位置。diff 的上下文行以空格起，不能用 `pos` */
  basePos: number;
}
interface Cx {
  lineStart: number;
  /** 下一行的文本，不动当前行 */
  peekLine(): string;
  /** 移到下一行；到底了返回 false */
  nextLine(): boolean;
  elt(type: string, from: number, to: number): Elt;
  addElement(e: Elt): void;
}

export interface Preds {
  /** 这一行像日志 */
  logLine(t: string): boolean;
  /** 段中间的堆栈行续段（`at com.foo(Bar.java:12)` / `Caused by:`），但不能起段 */
  stackLine(t: string): boolean;
  /** 这一行在 diff 里是什么；不像 diff 的 null */
  diffKind(t: string): unknown;
}

/** 从当前行起吃掉所有满足 `cont` 的后续行，产一个 `type` 块。当前行已经验过 */
export function eat(cx: Cx, line: Line, type: string, cont: (t: string) => boolean): true {
  const from = cx.lineStart + line.basePos;
  for (;;) {
    const next = cx.peekLine();
    // 空行不续：markdown 的段落也在这儿断，两边一致。段的边界另算（见文件头）
    if (next.trim() === "" || !cont(next)) break;
    if (!cx.nextLine()) break;
  }
  const to = cx.lineStart + line.text.length;
  cx.addElement(cx.elt(type, from, to));
  cx.nextLine();
  return true;
}

export function makeLogDiffBlocks(p: Preds) {
  const logLine = (t: string) => p.logLine(t);
  const logCont = (t: string) => p.logLine(t) || p.stackLine(t);
  const diffStart = (t: string) => t.startsWith("diff --git ") || t.startsWith("@@");
  const diffCont = (t: string) => p.diffKind(t) !== null;
  return {
    defineNodes: [
      { name: "LogBlock", block: true },
      { name: "DiffBlock", block: true },
    ],
    parseBlock: [
      {
        name: "LogBlock",
        parse(cx: Cx, line: Line) {
          const t = line.text.slice(line.basePos);
          // 至少两行才起块：一行像日志的多半是正文里引用了一句。三行的判据在段那边
          if (!logLine(t) || !logLine(cx.peekLine())) return false;
          return eat(cx, line, "LogBlock", logCont);
        },
        // 前面是正文、没空行直接接日志：也要能打断段落，不然整段还是一个 Paragraph
        endLeaf(cx: Cx, line: Line) {
          return logLine(line.text.slice(line.basePos)) && logLine(cx.peekLine());
        },
        before: "LinkReference",
      },
      {
        name: "DiffBlock",
        parse(cx: Cx, line: Line) {
          if (!diffStart(line.text.slice(line.basePos))) return false;
          return eat(cx, line, "DiffBlock", diffCont);
        },
        endLeaf(_cx: Cx, line: Line) {
          return diffStart(line.text.slice(line.basePos));
        },
        before: "LinkReference",
      },
    ],
  };
}
