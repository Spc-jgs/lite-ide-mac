/**
 * 重构的快捷键（docs/TAKEOVER.md G12，IDEA 的 ⌥⌘V / ⌥⌘M / ⌥⌘C / ⌥⌘F / ⌥⌘N）里不碰编辑器的那一半：jdtls 的英文标题换成中文、抽完之后新名字在哪。
 * 拆出来在裸 node 里测（tests/refactor.test.ts）—— 接线在 java-lsp.ts。
 *
 * jdtls 只在客户端声明了 `advancedExtractRefactoringSupport` 时才把抽取做成「先问名字」的命令；没声明就直接给一笔改动、名字它自己起
 * （`extracted`、按表达式猜的 `length`）—— 读 1.61 `RefactorProcessor` 的字节码。我们不声明：那条路要接它一整套「执行重构命令」的往返。
 * 改成抽完立刻在新名字上开改名（⇧F6 那个框），效果同 IDEA 抽完光标停在名字上让你打
 */

/** jdtls 的标题（`CorrectionMessages.properties` / `ActionMessages.properties` 里的原文）→ 中文。没在表里的原样 */
const TITLES: Record<string, string> = {
  "Extract to local variable (replace all occurrences)": "抽成局部变量（替换所有相同的）",
  "Extract to local variable": "抽成局部变量（只换这一处）",
  "Extract to method": "抽成方法",
  "Extract lambda body to method": "把 lambda 抽成方法",
  "Extract to constant": "抽成常量",
  "Extract to field": "抽成字段",
  "Inline local variable": "内联局部变量",
  "Inline Method": "内联方法",
  "Inline Constant": "内联常量",
  "Convert local variable to field": "局部变量改成字段",
};

export function refactorTitle(t: string): string {
  return TITLES[t] ?? t;
}

const IDENT = /^[\p{L}_$][\p{L}\p{N}_$]*$/u;

const KEYWORDS = new Set(
  "abstract assert boolean break byte case catch char class const continue default do double else enum extends final finally float for goto if implements import instanceof int interface long native new package private protected public return short static strictfp super switch synchronized this throw throws transient try void volatile while var record yield true false null".split(" "),
);

/**
 * 抽完之后新名字在哪：第几笔改动、它插进去的字里第几个字开始。`old` 是这笔改动换掉的原文。认三种形状 ——
 * - 原来那段换成了一个名字（抽常量 / 字段时原处那一笔：`MAX`、`this.total`）
 * - 一个名字后面跟着括号、另一笔改动里也有它（调用处和新方法的声明分成两笔时）—— 取短的那笔（调用处）
 * - **一笔改动里全有**：jdtls 1.61 抽变量是把「声明 + 原来那两行」整段换掉，抽方法是「调用处 + 新方法」一笔（G12 验收时打出来看的）。
 *   这时比原文：新出现的名字（不是关键字）里出现次数最多的那个 —— 新变量声明一次、用两次，新方法调用一次、声明一次，`String` 这种只多出一次
 * 认不出来是 null（照样抽了，只是不接着改名）
 */
export function nameSpot(changes: readonly { insert: string; old?: string }[]): { index: number; offset: number } | null {
  for (let i = 0; i < changes.length; i++) {
    const t = changes[i].insert.trim();
    const lead = changes[i].insert.length - changes[i].insert.trimStart().length;
    if (IDENT.test(t)) return { index: i, offset: lead };
    if (t.startsWith("this.") && IDENT.test(t.slice(5))) return { index: i, offset: lead + 5 };
  }
  const order = changes.map((_, i) => i).sort((x, y) => changes[x].insert.length - changes[y].insert.length);
  for (const i of order) {
    for (const m of changes[i].insert.matchAll(/([\p{L}_$][\p{L}\p{N}_$]*)\s*\(/gu)) {
      if (changes.some((c, j) => j !== i && c.insert.includes(`${m[1]}(`))) return { index: i, offset: m.index! };
    }
  }
  let best: { index: number; offset: number; n: number } | null = null;
  for (let i = 0; i < changes.length; i++) {
    const old = changes[i].old;
    if (old === undefined) continue;
    const before = new Set(words(old).map((w) => w.text));
    const seen = new Map<string, { offset: number; n: number }>();
    for (const w of words(changes[i].insert)) {
      if (before.has(w.text) || KEYWORDS.has(w.text)) continue;
      const e = seen.get(w.text);
      if (e) e.n++;
      else seen.set(w.text, { offset: w.at, n: 1 });
    }
    for (const e of seen.values()) if (!best || e.n > best.n) best = { index: i, offset: e.offset, n: e.n };
  }
  return best ? { index: best.index, offset: best.offset } : null;
}

/** 一段字里的标识符和位置（字符串里的也算 —— 够用：新名字不会只出现在字符串里） */
function words(s: string): { text: string; at: number }[] {
  return [...s.matchAll(/[\p{L}_$][\p{L}\p{N}_$]*/gu)].map((m) => ({ text: m[0], at: m.index! }));
}
