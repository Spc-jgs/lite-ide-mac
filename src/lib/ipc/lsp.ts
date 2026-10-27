/** Java 以外的语言服务器（docs/LANGPACK.md）的命令包装：只有通用的语言服务器客户端（editor/lsp-generic.ts）用，它是懒的 */
import { invoke } from "@tauri-apps/api/core";
import type { JavaAction, JavaActionResult, JavaCompletion, JavaEdit, JavaLocation, JavaRefs, JavaRenameTarget, LspPacks, ReplaceOutcome } from "./commands";
import type { OpenDoc } from "./replace";

export const lspPacks = () => invoke<LspPacks>("lsp_packs");
/** 打开 / 更新一个文档（全文）。回这个文件归哪个语言包（null = 没有）；没装语言服务器是错误（带怎么装） */
export const lspOpen = (root: string, path: string, text: string) => invoke<string | null>("lsp_open", { root, path, text });
export const lspClose = (root: string, path: string) => invoke<void>("lsp_close", { root, path });
export const lspDefinition = (root: string, path: string, line: number, character: number) => invoke<JavaLocation[]>("lsp_definition", { root, path, line, character });
export const lspReferences = (root: string, path: string, line: number, character: number) => invoke<JavaRefs>("lsp_references", { root, path, line, character });
export const lspHover = (root: string, path: string, line: number, character: number) => invoke<string>("lsp_hover", { root, path, line, character });
export const lspComplete = (root: string, path: string, text: string, line: number, character: number) => invoke<JavaCompletion>("lsp_complete", { root, path, text, line, character });
/** 光标底下能不能改名（null = 不能） */
export const lspPrepareRename = (root: string, path: string, text: string, line: number, character: number) =>
  invoke<JavaRenameTarget | null>("lsp_prepare_rename", { root, path, text, line, character });
/** 改名：Rust 那边照跨文件替换落盘，结果同替换的形状（开着的标签走同一条路跟上） */
export const lspRename = (root: string, path: string, text: string, line: number, character: number, newName: string, open: OpenDoc[]) =>
  invoke<ReplaceOutcome>("lsp_rename", { root, path, text, line, character, newName, open });
export const lspFormat = (root: string, path: string, text: string, tabSize: number, insertSpaces: boolean) => invoke<JavaEdit[]>("lsp_format", { root, path, text, tabSize, insertSpaces });
type Range = { line: number; character: number; endLine: number; endCharacter: number };
export const lspCodeActions = (root: string, path: string, text: string, range: Range) => invoke<JavaAction[]>("lsp_code_actions", { root, path, text, range });
export const lspApplyAction = (root: string, path: string, raw: string, open: OpenDoc[]) => invoke<JavaActionResult>("lsp_apply_action", { root, path, raw, open });
