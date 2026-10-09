/**
 * 一个文件的符号表，**不用先把它打开**（issue #43：⌘P 里 `文件@符号`）。解析本身在 `outline.ts` 的 `outlineOfText`，
 * 这里只管按文件名把语言扩展懒加载进来、记住上一次的结果。
 *
 * 跟着 ⌘P 浮层按需 `import()`，打了 `@` 才付这个代价（`langs-load` 和语言包本来就是懒的）。
 */
import { outlineOfText, type Sym } from "./outline";
import { langOf } from "./langs";
import { loadLang } from "./langs-load";

/*
 * 同一个文件在一次 ⌘P 里会被问很多次（`@` 后面每敲一个字都问）。按路径记住上一次的文本和结果，
 * 文本一样就不重新解析。比的是整段文本而不是 mtime：开着的文件可能有没存的改动，盘上的时间说明不了它
 */
const cache = new Map<string, { text: string; syms: Sym[] }>();
const CACHE_MAX = 16;

/** 这个文件的符号。语言没有 Lezer 语法树（走 legacy stream parser 的）时是空表 */
export async function fileSymbols(path: string, text: string): Promise<Sym[]> {
  const hit = cache.get(path);
  if (hit && hit.text === text) return hit.syms;
  const ext = await loadLang(langOf(path));
  const syms = ext ? outlineOfText(text, ext) : [];
  cache.delete(path);
  cache.set(path, { text, syms });
  // Map 按插入顺序迭代：第一个就是最久没用的
  if (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value!);
  return syms;
}
