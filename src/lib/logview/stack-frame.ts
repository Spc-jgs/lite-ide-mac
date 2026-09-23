/**
 * 堆栈行 → 项目里的源文件：`at com.liteide.OrderService.persist(OrderService.java:142)` 点了跳过去。
 *
 * # 凭什么敢跳
 *
 * 和编辑器的 ⌘Click 第二层同一个依据：**Java 的包路径就是目录路径**（语言规范强制）。
 * 一帧里有全限定类名和文件名，拼出 `com/liteide/OrderService.java`，拿去和 ⌘P 那份文件索引
 * 做后缀匹配（模块前缀 `order-api/src/main/java/` 不知道，但包那一段是确定的）。
 *
 * **唯一命中才画成链接。** 找不到 = 在 jar 里（Spring、JDK、Hikari）—— 这正好把堆栈里
 * 「我们自己的代码」和「框架的代码」分开，排查时一眼看到第一行自己的帧；
 * 撞上两份（多模块共用包名、src/main 和 src/test）= 认不准，也不画 ——
 * 跳错了人不会怀疑，比跳不了严重（jump.ts 的 `findIn` 同一条）。
 *
 * # 不用类名，用括号里的文件名
 *
 * 内部类 / lambda 的帧是 `com.a.Outer$Inner.lambda$run$0(Outer.java:10)`：类名是
 * `Outer$Inner`，文件是 `Outer.java`。包取全限定名去掉最后两段（方法、类），文件名取括号里的。
 */

import type { Part } from "./parse";

export interface Frame {
  /** 链接画在哪一段（`OrderService.java:142`，不含括号），行内偏移 */
  from: number;
  to: number;
  /** 拿去做后缀匹配的相对路径：`com/liteide/OrderService.java` */
  suffix: string;
  line: number;
}

/**
 * `at` 之后：可选的模块 / 类加载器前缀（`java.base/`、`app//`）、全限定的 `类.方法`、
 * 括号里 `文件:行号`。`(Native Method)` / `(Unknown Source)` 没有行号，不匹配 —— 本来也跳不了
 */
const FRAME = /^\s*at\s+(?:[\w.$@-]*\/+)?([\w$]+(?:\.[\w$<>]+)+)\(([\w$-]+\.(?:java|kt|scala|groovy)):(\d+)\)/;

export function stackFrame(text: string): Frame | null {
  const m = FRAME.exec(text);
  if (!m) return null;
  const segs = m[1].split(".");
  // 最后一段是方法，倒数第二段是类（可能带 $），前面是包
  const pkg = segs.slice(0, -2);
  const file = m[2];
  const line = Number(m[3]);
  if (!line) return null;
  const from = m.index + m[0].lastIndexOf(`(${file}:`) + 1;
  return { from, to: from + file.length + 1 + m[3].length, suffix: [...pkg, file].join("/"), line };
}

/**
 * 在文件索引里坐实。**只认唯一**：一份都没有（在 jar 里）和撞上两份（认不准）都是 null。
 *
 * 索引可能几万条，而屏幕上每一条堆栈行都要问 —— 按「索引这份数组」缓存，
 * 同一个后缀只扫一遍；索引换了（终端里建了新文件、换了项目）缓存跟着扔。
 */
export function frameResolver(files: string[]): (suffix: string) => string | null {
  const memo = new Map<string, string | null>();
  return (suffix) => {
    const hit = memo.get(suffix);
    if (hit !== undefined) return hit;
    let found: string | null = null;
    let n = 0;
    for (const f of files) {
      if (f === suffix || f.endsWith(`/${suffix}`)) {
        found = f;
        if (++n > 1) break;
      }
    }
    const out = n === 1 ? found : null;
    memo.set(suffix, out);
    return out;
  };
}

/**
 * 把一行的分段在 [from, to) 处切开，中间那段标成 `link`。分段拼起来就是整行
 * （解析器的约定），所以按累计偏移切就行，不用知道每段是什么。
 */
export function withLink(parts: Part[], from: number, to: number): Part[] {
  const out: Part[] = [];
  let off = 0;
  for (const p of parts) {
    const a = off;
    const z = off + p.text.length;
    off = z;
    if (z <= from || a >= to) {
      out.push(p);
      continue;
    }
    if (a < from) out.push({ text: p.text.slice(0, from - a), cls: p.cls });
    out.push({ text: p.text.slice(Math.max(from, a) - a, Math.min(to, z) - a), cls: "link" });
    if (z > to) out.push({ text: p.text.slice(to - a), cls: p.cls });
  }
  return out;
}
