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

// ── Java 改名（docs/IDEA.md 第 14 节）走同一套：改类名连文件一起改名，开着的标签挪到新路径；撤销时名字和内容都回来 ──
// 浏览器里撞见过：撤销后标签回到了原名，内容却还是新名字 —— 桩报回的是改名前的路径，前端按它找不到还在新路径上的标签
// （真实现按文件身份认出开着的标签、报的是标签此刻的路径，桩改成同样的）
{
  const { javaRename } = await import("../src/lib/ipc/java");
  const SHOP = "/proj/r/ZzShop.java";
  const STORE = "/proj/r/ZzStore.java";
  const USE = "/proj/r/ZzUse.java";
  FILES[SHOP] = "public class ZzShop {}\n";
  FILES[USE] = "class ZzUse { ZzShop s = new ZzShop(); }\n";
  await tabflow.openPath(SHOP, { preview: false });
  await tabflow.openPath(USE, { preview: false });
  const o = await javaRename("/proj", SHOP, FILES[SHOP], 0, 15, "ZzStore", ops.openDocs());
  await ops.landInTabs(o);
  const shopTab = () => tabs.list.find((t) => t.path === STORE || t.path === SHOP);
  ok(FILES[STORE] === "public class ZzStore {}\n" && FILES[SHOP] === undefined, "类名改了、文件跟着改名");
  ok(shopTab()?.path === STORE && shopTab()?.content === "public class ZzStore {}\n" && !shopTab()?.dirty, `开着的标签挪到新路径、内容跟上：${JSON.stringify(shopTab() && { p: shopTab()!.path, c: shopTab()!.content })}`);
  ok(tabs.list.find((t) => t.path === USE)?.content === "class ZzUse { ZzStore s = new ZzStore(); }\n", "用到它的文件（开着的）内容跟上");
  replace.done = { root: "/proj", files: o.changed.length, hits: 3, label: "已改名" };
  await ops.undo();
  ok(FILES[SHOP] === "public class ZzShop {}\n" && FILES[STORE] === undefined, "撤销：文件名改回来、内容回来");
  ok(shopTab()?.path === SHOP && shopTab()?.content === "public class ZzShop {}\n" && !shopTab()?.dirty, `撤销：标签回到原路径、内容回到原样、不是「未保存」：${JSON.stringify(shopTab() && { p: shopTab()!.path, c: shopTab()!.content, d: shopTab()!.dirty })}`);
  ok(tabs.list.find((t) => t.path === USE)?.content === "class ZzUse { ZzShop s = new ZzShop(); }\n", "撤销：用到它的文件也回去了");
}

console.log(`${fail === 0 ? "✅" : "❌"} 跨文件替换的状态层：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
