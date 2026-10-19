/**
 * Java 智能（docs/IDEA.md 第 2 步）的命令包装。只有 Java 的编辑器扩展（懒加载）用：不进入口包。
 * 行、列从 0 数，列按 UTF-16 —— LSP 的默认，正好是 CM6 / JS 字符串的下标。
 */
import { invoke } from "@tauri-apps/api/core";
import type { JavaAction, JavaActionResult, JavaCompletion, JavaEdit, JavaLocation, JavaRefs, JavaRenameTarget, JavaResolved, JavaStatus, ReplaceOutcome } from "./commands";
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
/** 补全：带着编辑器此刻的全文（Rust 那边先同步再问 —— 去抖还没发出去的那几个字 jdtls 也要看到） */
export const javaComplete = (root: string, path: string, text: string, line: number, character: number) =>
  invoke<JavaCompletion>("java_complete", { root, path, text, line, character });
/** 选中的那一项：自动 import + 说明文字 */
export const javaResolve = (root: string, raw: string) => invoke<JavaResolved>("java_resolve", { root, raw });
/** 光标底下能不能改名：null = 这里不能改（关键字、注释里） */
export const javaPrepareRename = (root: string, path: string, text: string, line: number, character: number) =>
  invoke<JavaRenameTarget | null>("java_prepare_rename", { root, path, text, line, character });
/** 改名（docs/IDEA.md 第 14 节）：照跨文件替换落盘，结果的形状同替换（开着的标签走同一条路跟上，撤销卡片照样能撤） */
export const javaRename = (root: string, path: string, text: string, line: number, character: number, newName: string, open: OpenDoc[]) =>
  invoke<ReplaceOutcome>("java_rename", { root, path, text, line, character, newName, open });
/** 格式化（⌥⌘L）：要做的改动。`range` 为空 = 整份；缩进照编辑器此刻的设置 */
export const javaFormat = (
  root: string,
  path: string,
  text: string,
  range: { line: number; character: number; endLine: number; endCharacter: number } | null,
  tabSize: number,
  insertSpaces: boolean,
) => invoke<JavaEdit[]>("java_format", { root, path, text, range, tabSize, insertSpaces });
type Range = { line: number; character: number; endLine: number; endCharacter: number };
/** 光标那儿（或选中的那几行）能做的修复（⌥↵） */
export const javaCodeActions = (root: string, path: string, text: string, range: Range) => invoke<JavaAction[]>("java_code_actions", { root, path, text, range });
/** 做选中的那条修复 */
export const javaApplyAction = (root: string, path: string, raw: string, open: OpenDoc[]) => invoke<JavaActionResult>("java_apply_action", { root, path, raw, open });
/** jar 里一个类的内容（`jdt://…`）：有源码 jar 就是源码，没有就是反编译的 */
export const javaClassContents = (root: string, uri: string) => invoke<string>("java_class_contents", { root, uri });
export const javaStatus = (root: string) => invoke<JavaStatus | null>("java_status", { root });
export const javaRestart = (root: string) => invoke<void>("java_restart", { root });
