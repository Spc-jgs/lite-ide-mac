/**
 * 桩：Java 以外的语言服务器（docs/LANGPACK.md），对应 `src-tauri/src/commands/lsp.rs`（规则在 `src-tauri/src/langs.rs`）。
 * 语言包同内置的三份；`.rs` 装着一个假的 rust-analyzer：打开时说一句诊断（有 `bad` 这个词的那一行），补全回几个候选、F1 回一段文档；
 * `.py` / `.ts` 演示「没装」那条路（只说一次、带怎么装）。打开**故意等一会儿**（真的第一次要起服务器、建索引）
 */
import type { LspPack } from "../../ipc/commands";
import { type A, FILES, NOT_MINE } from "./data";
import { mockEmit } from "./events";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const PACKS: LspPack[] = [
  { id: "rust", name: "Rust", files: ["*.rs"] },
  { id: "python", name: "Python", files: ["*.py", "*.pyi"] },
  { id: "typescript", name: "TypeScript / JavaScript", files: ["*.ts", "*.tsx", "*.js", "*.jsx", "*.mts", "*.cts", "*.mjs", "*.cjs"] },
];
const INSTALL: Record<string, string> = { python: "pip install basedpyright（或 npm i -g basedpyright）", typescript: "npm i -g typescript typescript-language-server" };

export function lspCmd(cmd: string, a: A): unknown {
  const path = String(a.path ?? "");
  switch (cmd) {
    case "lsp_packs":
      return { packs: PACKS, problems: [] };
    case "lsp_open": {
      if (path.endsWith(".py")) return Promise.reject(`未找到 basedpyright-langserver：Python 的语言服务器未安装或不在 PATH 中（安装：${INSTALL.python}）`);
      if (/\.(ts|tsx|js|jsx)$/.test(path)) return Promise.reject(`未找到 typescript-language-server：TypeScript / JavaScript 的语言服务器未安装或不在 PATH 中（安装：${INSTALL.typescript}）`);
      if (!path.endsWith(".rs")) return null;
      const text = String(a.text ?? FILES[path] ?? "");
      return sleep(400).then(() => {
        const lines = text.split("\n");
        const i = lines.findIndex((l) => l.includes("bad"));
        const items = i < 0 ? [] : [{ line: i, character: lines[i].indexOf("bad"), endLine: i, endCharacter: lines[i].indexOf("bad") + 3, severity: 1, message: "cannot find value `bad` in this scope" }];
        mockEmit("lsp-diagnostics", { root: String(a.root), path, total: items.length, items });
        return "Rust";
      });
    }
    case "lsp_close":
      return null;
    case "lsp_definition":
      return [];
    case "lsp_references":
      return { items: [], total: 0 };
    case "lsp_hover":
      return "```rust\nfn yuan(&self) -> f64\n```\n\n把分换算成元。";
    case "lsp_complete":
      return {
        items: ["yuan", "cents", "new"].map((label, i) => ({ label, kind: i === 2 ? 3 : 2, detail: "", description: i === 1 ? "i64" : "", edit: null, insert: label, snippet: false, sort: String(i), raw: "{}" })),
        incomplete: false,
        total: 3,
      };
    // 改名、格式化、快速修复：桩里不演（要真改文件、真跨文件），说一句、不装作做成了；这几样在 scripts/accept/lsp.sh 上用真 rust-analyzer 验
    case "lsp_prepare_rename":
      return { line: Number(a.line), character: Number(a.character), endLine: Number(a.line), endCharacter: Number(a.character) + 4, placeholder: null };
    case "lsp_rename":
    case "lsp_apply_action":
      return Promise.reject("开发桩不演示此操作，请在真 .app 上使用");
    case "lsp_format":
      return [];
    case "lsp_code_actions":
      return [];
    default:
      return NOT_MINE;
  }
}
