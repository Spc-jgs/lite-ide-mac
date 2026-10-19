/**
 * 跑单测（docs/IDEA.md 第 17 节）里不碰编辑器的那一半：认出测试类和测试方法、拼任务名、拼「重跑失败的」的选择器。
 * 拆出来是为了在裸 node 里测（tests/java-tests.test.ts）—— 行号旁的 ▶ 在 java-test-gutter.ts。
 *
 * **不靠语言服务器**：按注解认（`@Test` 这几种，JUnit 4 / 5 都是这个名字），jdtls 没起好也能跑。代价：自己写的元注解（`@MyTest` 上面再标 `@Test`）认不出 ——
 * IDEA 认得，是因为它有完整的语义模型。
 */

/** JUnit 5 和 4 里标「这是一个测试」的注解。`@Nested` 的内部类这一版不认（surefire 要 `外部类$内部类`，行号旁的 ▶ 只给顶层的类和方法） */
const TEST_ANNOTATIONS = /@(?:org\.junit\.(?:jupiter\.api\.|jupiter\.params\.)?)?(Test|ParameterizedTest|RepeatedTest|TestFactory|TestTemplate)\b/;

export interface TestTarget {
  /** 第几行（从 1 数）：▶ 画在这一行 */
  line: number;
  /** 类的全名（有 package 的话带上） */
  cls: string;
  /** 方法名；整个类的那一个是 null */
  method: string | null;
}

/**
 * 一个 Java 文件里能跑的：顶层类（有测试方法才算）+ 每个测试方法。注解和方法名可以不在同一行（`@Test` 单独一行、
 * 中间还隔着 `@DisplayName(...)` 这类），往下找第一个「名字后面跟括号」的声明。注释和字符串里的 `@Test` 不算（整行注释跳过）
 */
export function testTargets(text: string): TestTarget[] {
  const lines = text.split("\n");
  const pkg = /^\s*package\s+([\w.]+)\s*;/m.exec(text)?.[1];
  let clsLine = -1;
  let clsName = "";
  let pending = false;
  const methods: { line: number; name: string }[] = [];
  let block = false;
  for (let i = 0; i < lines.length; i++) {
    let l = lines[i];
    // 块注释里的不算：粗略地跳过 /* … */ 之间的行（一行里开又关的只去掉那一段）
    if (block) {
      const e = l.indexOf("*/");
      if (e < 0) continue;
      l = l.slice(e + 2);
      block = false;
    }
    l = l.replace(/\/\*.*?\*\//g, "");
    const open = l.indexOf("/*");
    if (open >= 0) {
      block = true;
      l = l.slice(0, open);
    }
    const code = l.replace(/\/\/.*$/, "").replace(/"(?:[^"\\]|\\.)*"/g, '""');
    if (clsLine < 0) {
      const m = /\b(?:class|record)\s+([A-Za-z_$][\w$]*)/.exec(code);
      if (m) {
        clsLine = i;
        clsName = m[1];
      }
    }
    if (TEST_ANNOTATIONS.test(code)) pending = true;
    if (pending) {
      const m = /([A-Za-z_$][\w$]*)\s*\(/.exec(code.replace(/@[\w.]+(\s*\([^)]*\))?/g, ""));
      if (m && !/^(if|for|while|switch|return|new)$/.test(m[1])) {
        methods.push({ line: i + 1, name: m[1] });
        pending = false;
      }
    }
  }
  if (clsLine < 0 || methods.length === 0) return [];
  const cls = pkg ? `${pkg}.${clsName}` : clsName;
  return [{ line: clsLine + 1, cls, method: null }, ...methods.map((m) => ({ line: m.line, cls, method: m.name }))];
}

/**
 * 这个文件属于哪个模块：`src/test/java/` 前面那一段相对项目根的路径，根模块是 `.`。不在 `src/test/java/` 底下（不是测试源码）给 null。
 * 用目录不用 artifactId：从路径就算得出，不用先问 Rust（Rust 两种都认，taskdefs::find_module）
 */
export function moduleOf(root: string, path: string): string | null {
  if (!path.startsWith(`${root}/`)) return null;
  const rel = path.slice(root.length + 1);
  const i = rel.indexOf("src/test/java/");
  if (i < 0 || (i > 0 && rel[i - 1] !== "/")) return null;
  return i === 0 ? "." : rel.slice(0, i - 1);
}

/** 任务名：`模块 [test 类#方法]`（和 Maven 工具窗跑目标同一个形状，Rust 的 taskdefs 认它） */
export const testTaskName = (module: string, t: Pick<TestTarget, "cls" | "method">) => `${module} [test ${t.cls}${t.method ? `#${t.method}` : ""}]`;

/** 从任务名拆回模块和选择器；不是测试任务给 null */
export function parseTestTask(name: string): { module: string; selector: string } | null {
  const m = /^(.+) \[test (.+)\]$/.exec(name);
  return m ? { module: m[1], selector: m[2] } : null;
}

/** 重跑失败的：surefire 的选择器 `A#x+y,B#z`（同一个类的几个方法用 `+`，几个类用 `,`）。参数化测试的名字带括号（`adds(int)[1]`），只取括号前那段 */
export function failedSelector(cases: { class: string; name: string; outcome: string }[]): string | null {
  const byClass = new Map<string, Set<string>>();
  for (const c of cases) {
    if (c.outcome !== "failed" && c.outcome !== "error") continue;
    const m = c.name.replace(/[(\[].*$/, "");
    if (!/^[A-Za-z_$][\w$]*$/.test(m)) continue;
    const s = byClass.get(c.class) ?? new Set();
    s.add(m);
    byClass.set(c.class, s);
  }
  if (!byClass.size) return null;
  return [...byClass].map(([cls, ms]) => `${cls}#${[...ms].join("+")}`).join(",");
}
