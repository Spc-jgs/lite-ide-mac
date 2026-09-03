import { textToSave, settled, stashed, type Doc } from "../src/lib/state/doc.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// ── textToSave：编辑器活着的时候，它说了算 ────────────────────────────

/*
 * 这一条就是「保存并关闭把原文写回去」那个 bug。
 * 打开文件（content）→ 打字 → 切走再切回（draft）→ 再打字（只有 live 知道）。
 * 少了 live 这一档，写回磁盘的是几步之前的东西。
 */
const 三档都有: Doc = { content: "磁盘", draft: "草稿", dirty: true };
ok(textToSave(三档都有, "编辑器里的") === "编辑器里的", "有 live 就必须用 live");
ok(textToSave(三档都有, null) === "草稿", "没有 live 时退到草稿");
ok(textToSave({ content: "磁盘", dirty: false }, null) === "磁盘", "连草稿都没有才用磁盘那份");
ok(textToSave({ dirty: false }, null) === "", "什么都没有时给空串，不是 undefined");

// 空串是一份合法内容：用户把文件清空了，要写回去的就是空
ok(textToSave(三档都有, "") === "", "live 是空串时不能掉到草稿去");
ok(textToSave({ content: "磁盘", draft: "", dirty: true }, null) === "", "草稿是空串时不能掉到磁盘去");

// ── settled：读回磁盘的唯一出口，草稿一定清掉 ──────────────────────────

/*
 * 「外部改了、本地没改 → 直接跟上」那条路原来忘了清 draft，
 * 而保存和「用磁盘上的」两条路清了 —— 同一件事三处各写一遍，漏了一处。
 */
/*
 * 必须**贴到一个已经带着草稿的标签上**去测，不能只看 settled 的返回值：
 * 它造的是新对象，「没有 draft 这个键」和「draft: undefined」读出来都是
 * undefined，两种写法都能过。而真实用法是 Object.assign 到已有的 tab 上 ——
 * 少写那个键，旧草稿就原封不动地留在那儿，正是当初漏掉的那一行。
 * （第一版测试就是这么写的，把 settled 改坏了它照样绿。）
 */
const 有陈草稿 = { content: "旧内容", draft: "陈草稿", dirty: true };
const 落定 = Object.assign({ ...有陈草稿 }, settled("新内容"));
ok(落定.content === "新内容", "content 换成新的");
ok(落定.draft === undefined, "贴上去之后，旧草稿必须被清掉");
ok(落定.dirty === false, "落定之后不脏");

// ── stashed：编辑器交回文本 ───────────────────────────────────────────

const 改过 = stashed({ content: "原文", dirty: false }, "原文+改动");
ok(改过.draft === "原文+改动" && 改过.dirty === true, "和磁盘不同 → 存草稿并标脏");

const 改回去 = stashed({ content: "原文", draft: "原文+改动", dirty: true }, "原文");
ok(改回去.draft === undefined, "改回原样 → 草稿要清掉，不能存一份和磁盘一样的");
ok(改回去.dirty === false, "改回原样 → 不脏");

// content 没有时按空串比，别把 undefined 和 "" 判成不同
ok(stashed({ dirty: false }, "").draft === undefined, "空文件里没打字 → 没有草稿");
ok(stashed({ dirty: false }, "x").dirty === true, "空文件里打了字 → 脏");

// 别的字段不能被顺手抹掉
const 带别的 = stashed({ content: "原文", dirty: false, ...{ 无关: 1 } } as Doc, "改了");
ok((带别的 as Record<string, unknown>).无关 === 1, "stashed 不该丢掉无关字段");

console.log(`${fail === 0 ? "✅" : "❌"} 文本状态机：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
