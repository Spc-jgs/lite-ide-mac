/**
 * Java 智能（docs/IDEA.md 第 2 步）的命令包装。只有 Java 的编辑器扩展（懒加载）用：不进入口包。
 * 行、列从 0 数，列按 UTF-16 —— LSP 的默认，正好是 CM6 / JS 字符串的下标。
 */
import { invoke } from "@tauri-apps/api/core";
import type { SpringProp, JavaAction, JavaActionResult, JavaCompletion, JavaEdit, JavaGenChoices, JavaHierChildren, JavaHierNode, JavaHighlight, JavaInlay, JavaLocation, JavaMethodSig, JavaMoveDest, JavaMovePreview, JavaRefs, JavaTypes, JavaRenameTarget, JavaResolved, JavaStatus, MybatisLinks, ReplaceOutcome, SpringConfigHit, SpringEndpoint, JavaSignatures } from "./commands";
import type { OpenDoc } from "./replace";

/** 打开了一个 Java 文件：jdtls 没有就起（不等它起好），登记这份文档。返回当时的状态 */
/** 不是 Maven / Gradle 项目给 null：不起 jdtls，也不算错 */
export const javaOpen = (root: string, path: string, text: string) => invoke<JavaStatus | null>("java_open", { root, path, text });
/** 改了（全文） */
export const javaChange = (root: string, path: string, text: string) => invoke<void>("java_change", { root, path, text });
export const javaSave = (root: string, path: string) => invoke<void>("java_save", { root, path });
export const javaClose = (root: string, path: string) => invoke<void>("java_close", { root, path });
export const javaDefinition = (root: string, path: string, line: number, character: number) =>
  invoke<JavaLocation[]>("java_definition", { root, path, line, character });
export const javaReferences = (root: string, path: string, line: number, character: number) =>
  invoke<JavaRefs>("java_references", { root, path, line, character });
/** 跳到实现（⌥⌘B，docs/TAKEOVER.md G1）：接口 / 抽象方法的实现、类的子类。样子同查用法 */
export const javaImplementation = (root: string, path: string, line: number, character: number) =>
  invoke<JavaRefs>("java_implementation", { root, path, line, character });
/** 快速文档（F1）：签名 + Javadoc，一段 Markdown；没有文档是空串 */
export const javaHover = (root: string, path: string, line: number, character: number) => invoke<string>("java_hover", { root, path, line, character });
/** ⇧⇧ 的「类」（docs/TAKEOVER.md G2）。`libs`：连 jar / JDK 里的一起找。这个项目的 jdtls 没在跑给 null（按文件名找，不为一次搜索起它） */
export const javaTypes = (root: string, query: string, libs: boolean) => invoke<JavaTypes | null>("java_types", { root, query, libs });
/** 同名高亮（G11）：光标底下那个名字在这个文件里出现的地方（语义的：同名的别的变量不算） */
export const javaHighlights = (root: string, path: string, line: number, character: number) =>
  invoke<JavaHighlight[]>("java_highlights", { root, path, line, character });
/** 内嵌提示（G11）：从 `from` 行开头到 `(endLine, endChar)` 的参数名提示（行列从 0 数；终点要是文件里真有的位置，jdtls 对不存在的行整段回空） */
export const javaInlayHints = (root: string, path: string, from: number, endLine: number, endChar: number) =>
  invoke<JavaInlay[]>("java_inlay_hints", { root, path, from, endLine, endChar });
/** 调用层级 / 类型层级的根（G10）：光标底下那个方法 / 类型。`types`：类型层级（⌃H） */
export const javaHierPrepare = (root: string, path: string, line: number, character: number, types: boolean) =>
  invoke<JavaHierNode[]>("java_hier_prepare", { root, path, line, character, types });
/** 一个节点的下一层：`in` 谁调了它 / `out` 它调了谁 / `super` 父类型 / `sub` 子类型 */
export const javaHierChildren = (root: string, raw: string, dir: "in" | "out" | "super" | "sub") => invoke<JavaHierChildren>("java_hier_children", { root, raw, dir });
/** ⌘N 生成（G5）第一步：光标所在的类里能选哪些（字段 / 可覆盖的方法） */
export type GenWhat = "constructor" | "getter" | "setter" | "accessors" | "equals" | "tostring" | "override";
export const javaGenStatus = (root: string, path: string, text: string, line: number, character: number, what: GenWhat) =>
  invoke<JavaGenChoices>("java_gen_status", { root, path, text, line, character, what });
/** 第二步：人勾好了（第一步那几项的 `raw`），要改动 —— 只改这个文件，交给编辑器 */
export const javaGenApply = (root: string, path: string, text: string, line: number, character: number, what: GenWhat, picked: string[]) =>
  invoke<JavaActionResult>("java_gen_apply", { root, path, text, line, character, what, picked });
/** 跳到父方法（⌘U）：覆盖的那个方法 / 父类（jdtls 的 `java/findLinks`） */
export const javaSuper = (root: string, path: string, line: number, character: number) =>
  invoke<JavaRefs>("java_super", { root, path, line, character });
/** 补全：带着编辑器此刻的全文（Rust 那边先同步再问 —— 去抖还没发出去的那几个字 jdtls 也要看到） */
export const javaComplete = (root: string, path: string, text: string, line: number, character: number) =>
  invoke<JavaCompletion>("java_complete", { root, path, text, line, character });
/** 参数提示：光标此刻在哪个调用的第几个参数上（带着编辑器此刻的全文，同补全） */
export const javaSignature = (root: string, path: string, text: string, line: number, character: number) =>
  invoke<JavaSignatures>("java_signature", { root, path, text, line, character });
/** 选中的那一项：自动 import + 说明文字 */
export const javaResolve = (root: string, raw: string) => invoke<JavaResolved>("java_resolve", { root, raw });
/** 光标底下能不能改名：null = 这里不能改（关键字、注释里） */
export const javaPrepareRename = (root: string, path: string, text: string, line: number, character: number) =>
  invoke<JavaRenameTarget | null>("java_prepare_rename", { root, path, text, line, character });
/** 改名（docs/IDEA.md 第 14 节）：照跨文件替换落盘，结果的形状同替换（开着的标签走同一条路跟上，撤销卡片照样能撤） */
export const javaRename = (root: string, path: string, text: string, line: number, character: number, newName: string, open: OpenDoc[]) =>
  invoke<ReplaceOutcome>("java_rename", { root, path, text, line, character, newName, open });
/** 移动类（F6）：能去哪几个包 / 预览要改哪些 / 真改（和预览列的对不上就不做） */
export const javaMoveDestinations = (root: string, path: string) => invoke<JavaMoveDest[]>("java_move_destinations", { root, path });
/** Spring Boot 配置文件能写的键（读这个文件所在模块 classpath 上的配置元数据；第一次要等 jdtls 导入完） */
export const springProps = (root: string, path: string) => invoke<SpringProp[]>("spring_props", { root, path });
/** 改签名（⌘F6）：光标那个方法现在的签名 */
export const javaMethodSig = (root: string, path: string, text: string, line: number, character: number) =>
  invoke<JavaMethodSig>("java_method_sig", { root, path, text, line, character });
/** 改签名的预览：改成 `sig` 要动哪些文件（不改） */
export const javaMethodSigPreview = (root: string, path: string, text: string, line: number, character: number, sig: JavaMethodSig, tabSize: number, spaces: boolean) =>
  invoke<JavaMovePreview>("java_method_sig_preview", { root, path, text, line, character, sig, tabSize, spaces });
/** 改签名：和预览时列的文件对不上就不做 */
export const javaChangeSignature = (root: string, path: string, text: string, line: number, character: number, sig: JavaMethodSig, tabSize: number, spaces: boolean, expect: string[], open: OpenDoc[]) =>
  invoke<ReplaceOutcome>("java_change_signature", { root, path, text, line, character, sig, tabSize, spaces, expect, open });
export const javaMovePreview = (root: string, path: string, text: string, dest: string) => invoke<JavaMovePreview>("java_move_preview", { root, path, text, dest });
export const javaMove = (root: string, path: string, text: string, dest: string, expect: string[], open: OpenDoc[]) =>
  invoke<ReplaceOutcome>("java_move", { root, path, text, dest, expect, open });
/** 格式化（⌥⌘L）：要做的改动。`range` 为空 = 整份；缩进照编辑器此刻的设置 */
export const javaFormat = (
  root: string,
  path: string,
  text: string,
  range: { line: number; character: number; endLine: number; endCharacter: number } | null,
  tabSize: number,
  insertSpaces: boolean,
) => invoke<JavaEdit[]>("java_format", { root, path, text, range, tabSize, insertSpaces });
/** 优化 import（⌃⌥O，docs/TAKEOVER.md G22）：这个文件要改的那几处。次序、合成 `.*` 的阈值照 IDEA 的代码风格（`codestyle.rs`） */
export const javaOrganizeImports = (root: string, path: string, text: string) => invoke<JavaEdit[]>("java_organize_imports", { root, path, text });
type Range = { line: number; character: number; endLine: number; endCharacter: number };
/** 光标那儿（或选中的那几行）能做的修复（⌥↵） */
export const javaCodeActions = (root: string, path: string, text: string, range: Range, only: string | null = null) =>
  invoke<JavaAction[]>("java_code_actions", { root, path, text, range, only });
/** 做选中的那条修复 */
export const javaApplyAction = (root: string, path: string, raw: string, open: OpenDoc[]) => invoke<JavaActionResult>("java_apply_action", { root, path, raw, open });
/** jar 里一个类的内容（`jdt://…`）：有源码 jar 就是源码，没有就是反编译的 */
export const javaClassContents = (root: string, uri: string) => invoke<string>("java_class_contents", { root, uri });
export const javaStatus = (root: string) => invoke<JavaStatus | null>("java_status", { root });
export const javaRestart = (root: string) => invoke<void>("java_restart", { root });

/** MyBatis（docs/IDEA.md 第 24 节）：打开的 Java 接口 / Mapper XML 里的点，和对面那些（按名字配对）。`text` 是编辑器里此刻的 */
export const mybatisForJava = (root: string, text: string) => invoke<MybatisLinks>("mybatis_for_java", { root, text });
export const mybatisForXml = (root: string, text: string) => invoke<MybatisLinks>("mybatis_for_xml", { root, text });

/** Spring 的接口（docs/IDEA.md 第 25 节，随处搜索的「接口」）；一个配置键在哪几份配置文件里（`@Value` 上 ⌘B） */
export const springEndpoints = (root: string) => invoke<SpringEndpoint[]>("spring_endpoints", { root });
export const springConfig = (root: string, key: string) => invoke<SpringConfigHit[]>("spring_config", { root, key });
