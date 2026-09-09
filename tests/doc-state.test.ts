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

/*
 * **只带这三个字段回去，别的一律不带。**
 *
 * 这条原来反着写（「stashed 不该丢掉无关字段」，靠 `{ ...doc }` 过关）。
 * 那个担心是空的：调用方是 `Object.assign(tab, stashed(tab, text))`，
 * 返回值里没有的键本来就不会动到标签上那一份。而带上它们是有害的 ——
 * `doc` 传进来的就是整个标签，展开等于给标签拍一张快照，
 * 谁在这两毫秒里改了别的字段，谁就被这张快照盖回去。
 *
 * 真出过事：点「切换到日志模式」，`doSwitch` 刚把 mode 改成 log、
 * handle 换成引擎句柄，内容区跟着换掉，CodeMirror 销毁前调 onStash ——
 * 快照把 mode / handle 一起打回原形。按钮按了像没按，而 Rust 侧的
 * `open_log` 确实调过了。
 *
 * 照真实用法测（贴到标签上），不能只看返回值：缺键和 undefined 读出来一样。
 */
const 切到日志后的标签 = { mode: "log", handle: 7, content: undefined, draft: undefined, dirty: false };
/*
 * 迟到的 onStash 手里那份是**切换之前**的标签 —— 整个标签，不是三个字段。
 * 这一点是测试能不能红的关键：只传 `{ content, dirty }` 的话，
 * `{ ...doc }` 里根本没有 mode / handle，改坏了也照样绿。
 * （第一版就是这么写的，把 stashed 改回 `{ ...doc }` 跑，16 条全过。）
 */
const 切换前的标签 = { mode: "edit", handle: undefined, content: "原文", draft: undefined, dirty: false };
Object.assign(切到日志后的标签, stashed(切换前的标签 as Doc, "原文"));
ok(切到日志后的标签.mode === "log", "迟到的 stash 不能把 mode 打回 edit");
ok(切到日志后的标签.handle === 7, "迟到的 stash 不能把日志引擎的句柄抹掉");

console.log(`${fail === 0 ? "✅" : "❌"} 文本状态机：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
