/**
 * Java 智能（docs/IDEA.md 第 2 步）的命令包装。只有 Java 的编辑器扩展（懒加载）用：不进入口包。
 * 行、列从 0 数，列按 UTF-16 —— LSP 的默认，正好是 CM6 / JS 字符串的下标。
 */
import { invoke } from "@tauri-apps/api/core";
import type { JavaLocation, JavaRefs, JavaStatus } from "./commands";

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
/** jar 里一个类的内容（`jdt://…`）：有源码 jar 就是源码，没有就是反编译的 */
export const javaClassContents = (root: string, uri: string) => invoke<string>("java_class_contents", { root, uri });
export const javaStatus = (root: string) => invoke<JavaStatus | null>("java_status", { root });
export const javaRestart = (root: string) => invoke<void>("java_restart", { root });
