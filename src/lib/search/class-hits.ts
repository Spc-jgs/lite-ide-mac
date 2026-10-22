/**
 * ⇧⇧ 的「类」（docs/TAKEOVER.md G2）：jdtls 找回来的类型怎么排、怎么画；jdtls 没在跑时按文件名顶上。
 *
 * jdtls 回来的次序不作数（它存在一个 HashSet 里），排序在这儿做：项目里的整组排在 jar 里的前面（IDEA 也是 ——
 * 找 `Order` 时第一眼要的是自己的 `OrderService`，不是 JDK 的 `Ordering`），组内按名字的匹配度。
 * 名字怎么匹配是 jdtls 定的（包含、驼峰缩写），这里的 `rank` 只用来排序和标高亮；
 * 带 `*` `?` 的通配写法 `rank` 不认，照 jdtls 的结果按名字排、不标高亮 —— 不能因为我们不认就把它找到的丢掉。
 */
import type { JavaType } from "../ipc/commands";
import { rank, segments } from "./fuzzy";

type Seg = { t: string; hit: boolean };

export interface ClassHit {
  name: string;
  /** 包名；按文件名顶上的那种是目录 */
  container: string;
  /** 类 / 接口 / 枚举；按文件名顶上的不知道是哪种，空串 */
  kind: string;
  seg: Seg[];
  /** jar 里的（没有源文件，开成只读标签） */
  lib: string | null;
  /** 源文件（jdtls 给的是绝对路径，按文件名找的是相对项目根的）；jar 里的为空 */
  path: string | null;
  uri: string;
  label: string;
  /** 从 1 数；按文件名找的没有 */
  line?: number;
  col?: number;
}

/** jdtls 只回这三种（`java.rs` 的 `TypeHit::kind`）。注解带着接口标记，报成接口 —— 照实写，不猜 */
const KIND: Record<number, string> = { 5: "类", 11: "接口", 10: "枚举" };
/** 按文件名顶上时认哪些文件（一个文件一个公开类型，文件名就是类名） */
const JVM = /\.(java|kt|groovy|scala)$/;

/** `jdt://contents/spring-core-6.2.jar/org.springframework.core/Ordered.class?…` → `spring-core-6.2.jar` */
export function jarOf(uri: string): string {
  const m = /^jdt:\/\/contents\/([^/]+)\//.exec(uri);
  return m ? decodeURIComponent(m[1]) : "";
}

export function rankTypes(items: JavaType[], query: string, n: number): ClassHit[] {
  const q = query.trim();
  const dotted = q.includes(".");
  const key = (t: JavaType) => (dotted ? `${t.container}.${t.name}` : t.name);
  const hit = (t: JavaType, positions: number[]): ClassHit => {
    // 带包名排的时候，高亮只标名字那一段（包名在右边那列，不标）
    const off = dotted ? t.container.length + 1 : 0;
    const pos = positions.filter((p) => p >= off).map((p) => p - off);
    return {
      name: t.name,
      container: t.container,
      kind: KIND[t.kind] ?? "类",
      seg: segments(t.name, pos),
      lib: t.at.path ? null : jarOf(t.at.uri) || "jar",
      path: t.at.path,
      uri: t.at.uri,
      label: t.at.label,
      line: t.at.line + 1,
      col: t.at.character + 1,
    };
  };
  const group = (list: JavaType[], lim: number) => {
    if (lim <= 0) return [];
    if (/[*?]/.test(q)) return [...list].sort((a, b) => a.name.localeCompare(b.name)).slice(0, lim).map((t) => hit(t, []));
    return rank(list, q, key, lim).map((r) => hit(r.item, r.positions));
  };
  const mine = group(items.filter((t) => t.at.path), n);
  return [...mine, ...group(items.filter((t) => !t.at.path), n - mine.length)];
}

/** jdtls 没在跑：按文件名找（`OrderService.java` 就是 `OrderService`）。不知道是类还是接口，不写种类 */
export function fileTypes(files: string[], query: string, n: number): ClassHit[] {
  const q = query.trim();
  const stem = (f: string) => f.slice(f.lastIndexOf("/") + 1).replace(JVM, "");
  return rank(files.filter((f) => JVM.test(f)), q, stem, n).map((r) => {
    const name = stem(r.item);
    const cut = r.item.lastIndexOf("/");
    return { name, container: cut < 0 ? "" : r.item.slice(0, cut), kind: "", seg: segments(name, r.positions), lib: null, path: r.item, uri: "", label: name };
  });
}

/**
 * jar 里的类 jdtls 给的位置是文件开头（`JDTUtils.toLocation(classFile)`：二进制类没有源码位置），开出来停在 1:1；
 * ⌘B 进去却停在声明那行（那条路问的是 definition）。在内容里找 `class 名字` 那行，停在同一个地方。注释里的不算。找不到给 null
 */
export function declLine(text: string, name: string): { line: number; col: number } | null {
  const re = new RegExp(`\\b(?:class|interface|enum|record|@interface)\\s+(${name.replace(/[$]/g, "\\$")})\\b`);
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (/^\s*(\*|\/\/|\/\*)/.test(lines[i])) continue;
    const m = re.exec(lines[i]);
    if (m) return { line: i + 1, col: m.index + m[0].length - m[1].length + 1 };
  }
  return null;
}
