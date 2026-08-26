/**
 * 语言支持按需懒加载。
 *
 * 每个 Lezer parser 都是几十到几百 KB，全部静态引入会让冷启动变慢 ——
 * 而"秒开"是这个项目的立身之本。改成动态 import 后，打开 .java 才付 Java 的代价。
 */

import type { Extension } from "@codemirror/state";

export type LangId =
  | "java"
  | "javascript"
  | "typescript"
  | "python"
  | "markdown"
  | "json"
  | "rust"
  | "yaml"
  | "toml"
  | "html"
  | "css"
  | "xml"
  | "sql"
  | "shell"
  | "properties"
  | "dockerfile"
  | "go"
  | "c"
  | null;

/** 扩展名 → 语言 */
const BY_EXT: Record<string, LangId> = {
  java: "java",
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascript",
  ts: "typescript", mts: "typescript", cts: "typescript", tsx: "typescript",
  py: "python", pyi: "python", pyw: "python",
  md: "markdown", markdown: "markdown",
  json: "json", jsonc: "json", json5: "json", map: "json",
  rs: "rust",
  yaml: "yaml", yml: "yaml",
  toml: "toml", lock: "toml",
  html: "html", htm: "html", vue: "html", svelte: "html",
  css: "css", scss: "css", less: "css",
  xml: "xml", svg: "xml", xsd: "xml", plist: "xml", pom: "xml",
  sql: "sql",
  sh: "shell", bash: "shell", zsh: "shell", fish: "shell", zshrc: "shell", bashrc: "shell",
  properties: "properties", ini: "properties", cfg: "properties", conf: "properties", env: "properties",
  go: "go",
  c: "c", h: "c", cpp: "c", cc: "c", cxx: "c", hpp: "c",
};

/** 没有扩展名、但一眼能认出来的文件 */
const BY_NAME: Record<string, LangId> = {
  dockerfile: "dockerfile",
  makefile: "shell",
  ".zshrc": "shell",
  ".bashrc": "shell",
  ".profile": "shell",
  ".env": "properties",
  "cargo.lock": "toml",
  "gemfile": "shell",
};

/** 认出文件用什么语言。认不出来就纯文本，不猜。 */
export function langOf(filename: string): LangId {
  const base = filename.slice(filename.lastIndexOf("/") + 1);
  const lower = base.toLowerCase();

  const byName = BY_NAME[lower];
  if (byName) return byName;

  const dot = base.lastIndexOf(".");
  // 没有点，或者点在开头（.gitignore 这种）——按整名再查一次
  if (dot <= 0) return null;
  return BY_EXT[base.slice(dot + 1).toLowerCase()] ?? null;
}

const cache = new Map<string, Extension>();

/** 加载语言扩展。同一语言只会真正 import 一次。 */
export async function loadLang(id: LangId): Promise<Extension | null> {
  if (!id) return null;
  const hit = cache.get(id);
  if (hit) return hit;

  let ext: Extension;
  switch (id) {
    case "java": {
      const m = await import("@codemirror/lang-java");
      ext = m.java();
      break;
    }
    case "javascript": {
      const m = await import("@codemirror/lang-javascript");
      ext = m.javascript({ jsx: true });
      break;
    }
    case "typescript": {
      const m = await import("@codemirror/lang-javascript");
      ext = m.javascript({ jsx: true, typescript: true });
      break;
    }
    case "python": {
      const m = await import("@codemirror/lang-python");
      ext = m.python();
      break;
    }
    case "markdown": {
      const [m, live] = await Promise.all([
        import("@codemirror/lang-markdown"),
        import("./markdown-live"),
      ]);
      // base 换成 markdownLanguage：它带 GFM（删除线、表格、任务列表），
      // 默认的 commonmarkLanguage 不认 ~~删除线~~
      ext = [
        m.markdown({ base: m.markdownLanguage, codeLanguages: [] }),
        live.markdownLivePreview,
      ];
      break;
    }
    case "json": {
      const m = await import("@codemirror/lang-json");
      ext = m.json();
      break;
    }
    case "rust": {
      const m = await import("@codemirror/lang-rust");
      ext = m.rust();
      break;
    }
    case "yaml": {
      const m = await import("@codemirror/lang-yaml");
      ext = m.yaml();
      break;
    }
    case "html": {
      const m = await import("@codemirror/lang-html");
      ext = m.html();
      break;
    }
    case "css": {
      const m = await import("@codemirror/lang-css");
      ext = m.css();
      break;
    }
    case "xml": {
      const m = await import("@codemirror/lang-xml");
      ext = m.xml();
      break;
    }
    case "sql": {
      const m = await import("@codemirror/lang-sql");
      ext = m.sql();
      break;
    }
    // 以下几种没有独立的 Lezer 包，走 legacy stream parser。
    // 高亮质量略逊于 Lezer，但对配置文件和脚本完全够用。
    case "toml": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/toml"),
      ]);
      ext = StreamLanguage.define(m.toml);
      break;
    }
    case "shell": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/shell"),
      ]);
      ext = StreamLanguage.define(m.shell);
      break;
    }
    case "properties": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/properties"),
      ]);
      ext = StreamLanguage.define(m.properties);
      break;
    }
    case "dockerfile": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/dockerfile"),
      ]);
      ext = StreamLanguage.define(m.dockerFile);
      break;
    }
    case "go": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/go"),
      ]);
      ext = StreamLanguage.define(m.go);
      break;
    }
    case "c": {
      const [{ StreamLanguage }, m] = await Promise.all([
        import("@codemirror/language"),
        import("@codemirror/legacy-modes/mode/clike"),
      ]);
      ext = StreamLanguage.define(m.c);
      break;
    }
  }
  cache.set(id, ext);
  return ext;
}

/** 给状态栏显示用的名字 */
export function langLabel(id: LangId): string {
  if (!id) return "纯文本";
  const names: Record<string, string> = {
    java: "Java", javascript: "JavaScript", typescript: "TypeScript",
    python: "Python", markdown: "Markdown", json: "JSON", rust: "Rust",
    yaml: "YAML", toml: "TOML", html: "HTML", css: "CSS", xml: "XML",
    sql: "SQL", shell: "Shell", properties: "Properties",
    dockerfile: "Dockerfile", go: "Go", c: "C/C++",
  };
  return names[id] ?? id;
}
