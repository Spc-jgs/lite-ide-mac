/**
 * 跨文件替换（#42 第 3 步）的状态层：替换之后开着的标签怎么跟上、撤销之后怎么回去。
 * 跑在裸 node 里，IPC 走浏览器桩（mock/replace.ts，规则照 Rust 的 replacesvc）。盘上的那些（两段提交、日志、恢复）
 * 在 replacesvc 自己的测试里，这里只测前端独有的那半 —— 它错了，丢的是用户没存的字。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { FILES } = await import("../src/lib/dev/mock/data");
const { tabflow } = await import("../src/lib/state/tabflow.svelte");
const { tabs } = await import("../src/lib/state/tabs.svelte");
const { docs } = await import("../src/lib/state/docs.svelte");
const { project } = await import("../src/lib/state/project.svelte");
const { replace } = await import("../src/lib/state/replace.svelte");
const ops = await import("../src/lib/state/replace-ops");
const { hitKey } = await import("../src/lib/search/replace-model");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

project.root = "/proj";
// 三个文件各一处；词取得独一无二，别撞上桩里本来的那些示例文件
const A = "/proj/r/a.txt";
const B = "/proj/r/b.txt";
const C = "/proj/r/c.txt";
FILES[A] = "ZZOLD in a\n";
FILES[B] = "ZZOLD in b\n";
FILES[C] = "ZZOLD in c\n";

// a 开着、干净；b 开着、有没存的改动（编辑器替身交出实时文本，同 state-tabflow 那份）
await tabflow.openPath(A, { preview: false });
await tabflow.openPath(B, { preview: false });
let bLive = "ZZOLD in b\n我还没存的一行\n";
docs.onEditorLive(B, () => bLive);
const tb = tabs.byPath(B)!;
tb.dirty = true;

replace.query = "ZZOLD";
replace.replacement = "ZZNEW";
await ops.rescan();
ok(replace.scan?.files.length === 3, `扫到三个文件，实得 ${replace.scan?.files.map((f) => f.rel)}`);
ok(replace.scan?.files.find((f) => f.path === B)?.editor === true, "b 有没存的改动：用编辑器里那份");
ok(replace.after[0]?.[0]?.text === "ZZNEW in a", `预览的改后那一行，实得 ${JSON.stringify(replace.after[0]?.[0])}`);

// 取消勾 c 那一处
const fc = replace.scan!.files.find((f) => f.path === C)!;
replace.unchecked = new Set([hitKey(fc.rel, fc.hits[0])]);
const tickBefore = docs.savedTick;
await ops.apply();

const ta = tabs.byPath(A)!;
ok(FILES[A] === "ZZNEW in a\n", "a 写到了盘上");
ok(ta.content === "ZZNEW in a\n" && ta.draft === undefined && !ta.dirty, "a 的标签跟上了，而且不脏（盘上和编辑器一样）");
ok(docs.savedTick > tickBefore, "savedTick 加了：已挂载的编辑器会换成新内容");
ok(FILES[B] === "ZZOLD in b\n", "b 有没存的改动：盘上那份没动");
ok(tb.draft === "ZZNEW in b\n我还没存的一行\n" && tb.dirty, `b 的替换进了草稿，没存的那行还在、圆点还在，实得 ${JSON.stringify(tb.draft)}`);
ok(FILES[C] === "ZZOLD in c\n", "c 取消勾了：没动");
ok(replace.done?.files === 2 && replace.done?.hits === 2, `撤销卡片：2 个文件 2 处，实得 ${JSON.stringify(replace.done)}`);
ok(replace.scan === null && replace.unchecked.size === 0, "这份扫描作废了，勾选清掉");

// 编辑器替身照真编辑器那样把草稿换进去（Editor.svelte：initial 变了就换内容）
bLive = tb.draft!;
await ops.undo();
ok(FILES[A] === "ZZOLD in a\n" && ta.content === "ZZOLD in a\n" && !ta.dirty, "撤销：a 盘上和标签都回去了");
ok(tb.draft === "ZZOLD in b\n我还没存的一行\n" && tb.dirty, `撤销：b 回到替换前，没存的那行还在，实得 ${JSON.stringify(tb.draft)}`);
ok(replace.done === null, "撤销卡片收掉了");

console.log(`${fail === 0 ? "✅" : "❌"} 跨文件替换的状态层：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
