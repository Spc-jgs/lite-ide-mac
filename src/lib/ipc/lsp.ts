/** Java 以外的语言服务器（docs/LANGPACK.md）的命令包装：只有通用的语言服务器客户端（editor/lsp-generic.ts）用，它是懒的 */
import { invoke } from "@tauri-apps/api/core";
import type { JavaCompletion, JavaLocation, JavaRefs, LspPacks } from "./commands";

export const lspPacks = () => invoke<LspPacks>("lsp_packs");
/** 打开 / 更新一个文档（全文）。回这个文件归哪个语言包（null = 没有）；没装语言服务器是错误（带怎么装） */
export const lspOpen = (root: string, path: string, text: string) => invoke<string | null>("lsp_open", { root, path, text });
export const lspClose = (root: string, path: string) => invoke<void>("lsp_close", { root, path });
export const lspDefinition = (root: string, path: string, line: number, character: number) => invoke<JavaLocation[]>("lsp_definition", { root, path, line, character });
export const lspReferences = (root: string, path: string, line: number, character: number) => invoke<JavaRefs>("lsp_references", { root, path, line, character });
export const lspHover = (root: string, path: string, line: number, character: number) => invoke<string>("lsp_hover", { root, path, line, character });
export const lspComplete = (root: string, path: string, text: string, line: number, character: number) => invoke<JavaCompletion>("lsp_complete", { root, path, text, line, character });
