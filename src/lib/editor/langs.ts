/**
 * 语言支持按需懒加载。
 *
 * 四种语言的 Lezer parser 加起来有几百 KB，全部静态引入会让冷启动变慢 ——
 * 而"秒开"是这个项目的立身之本。改成动态 import 后，打开 .java 才付 Java 的代价。
 */

import type { Extension } from "@codemirror/state";

export type LangId = "java" | "javascript" | "typescript" | "python" | "markdown" | null;

/** 扩展名 → 语言。认不出来就纯文本，不猜。 */
export function langOf(filename: string): LangId {
  const ext = filename.slice(filename.lastIndexOf(".") + 1).toLowerCase();
  switch (ext) {
    case "java":
      return "java";
    case "js":
    case "mjs":
    case "cjs":
    case "jsx":
      return "javascript";
    case "ts":
    case "mts":
    case "cts":
    case "tsx":
      return "typescript";
    case "py":
    case "pyi":
    case "pyw":
      return "python";
    case "md":
    case "markdown":
      return "markdown";
    default:
      return null;
  }
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
      // 语法解析 + live preview 一起给。渲染纯粹是显示层的事，
      // 文档模型自始至终是那份 Markdown 源码
      // base 换成 markdownLanguage：它带 GFM（删除线、表格、任务列表），
      // 默认的 commonmarkLanguage 不认 ~~删除线~~
      ext = [
        m.markdown({ base: m.markdownLanguage, codeLanguages: [] }),
        live.markdownLivePreview,
      ];
      break;
    }
  }
  cache.set(id, ext);
  return ext;
}
