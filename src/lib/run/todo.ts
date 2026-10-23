/**
 * TODO 工具窗（docs/TAKEOVER.md G18，IDEA 的 TODO tool window）里不碰界面的那一半：搜什么、搜回来怎么分组。
 * 拆出来在裸 node 里测（tests/todo.test.ts）—— 界面在 `TodoView.svelte`。
 *
 * IDEA 只看**注释**里的 TODO / FIXME。rg 不认得注释，退一步：同一行前面要有一个注释的开头（`//` `/*` `*` `#` `<!--` `--`），
 * 字只认大写的 `TODO` / `FIXME` —— 不然 `todoList` 这种变量名、Markdown 里的「TODO」小节全算进来
 */

/** 交给 rg 的正则（项目搜索那条路，忽略规则同 ⇧⌘F） */
// `#` 只认单个的（`## 标题` 不算）：rg 用的 Rust 正则没有「后面不是」那种写法，写成「前后都不是 #」
export const TODO_PATTERN = String.raw`(//|/\*|^\s*\*|(^|[^#])#([^#]|$)|<!--|--).*\b(TODO|FIXME)\b`;

/** 最多列几处。问的时候多要一条：拿回来正好多一条就是截断了，要说出来（AGENTS.md「截断了要说出来」） */
export const TODO_MAX = 1000;

export interface TodoItem {
  line: number;
  tag: "TODO" | "FIXME";
  /** `TODO` 后面那句话（去掉冒号、前后空白）；空的给整行 */
  text: string;
}

export interface TodoFile {
  path: string;
  items: TodoItem[];
}

/** 搜回来的一条（项目搜索的 `Hit`）→ 一处 TODO。这一行其实没有大写的 TODO / FIXME（正则只是放宽了一点）就丢掉 */
export function todoOf(text: string, line: number): TodoItem | null {
  const m = /\b(TODO|FIXME)\b[\s:：-]*(.*)$/.exec(text);
  if (!m) return null;
  const rest = m[2].replace(/\s*(\*\/|-->)\s*$/, "").trim();
  return { line, tag: m[1] as TodoItem["tag"], text: rest || text.trim() };
}

/** 按文件分组：文件按路径排、一个文件里按行号排 */
export function groupTodos(hits: readonly { path: string; line: number; text: string }[]): TodoFile[] {
  const by = new Map<string, TodoItem[]>();
  for (const h of hits) {
    // Markdown 是文档不是代码：「# TODO」是个标题；只认它里面的 HTML 注释
    if (/\.(md|markdown)$/i.test(h.path) && !h.text.includes("<!--")) continue;
    const t = todoOf(h.text, h.line);
    if (!t) continue;
    by.set(h.path, [...(by.get(h.path) ?? []), t]);
  }
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([path, items]) => ({ path, items: items.sort((x, y) => x.line - y.line) }));
}
