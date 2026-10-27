/**
 * 重构快捷键里不碰编辑器的那一半（src/lib/editor/refactor.ts，docs/TAKEOVER.md G12）：标题换中文、抽完之后新名字在哪（接着开改名）。
 * 后两种「一笔改动里全有」的形状是 jdtls 1.61 真给的（G12 验收时打出来看的：抽变量、抽方法都是一整段换掉）。
 */
import { nameSpot, refactorTitle } from "../src/lib/editor/refactor.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

eq(refactorTitle("Extract to local variable (replace all occurrences)"), "引入局部变量（替换所有匹配项）", "标题换中文");
eq(refactorTitle("Add final modifier"), "Add final modifier", "表里没有的原样");

// 抽变量：声明一笔、原处换成名字一笔 —— 名字在第二笔开头
eq(nameSpot([{ insert: "int length = name.length();\n        " }, { insert: "length" }]), { index: 1, offset: 0 }, "抽变量：原处换成的名字");
eq(nameSpot([{ insert: "private static final int MAX = 100;\n" }, { insert: "MAX" }]), { index: 1, offset: 0 }, "抽常量");
eq(nameSpot([{ insert: "private int total;\n" }, { insert: "this.total" }]), { index: 1, offset: 5 }, "抽字段换成 this.x：名字从第 5 个字开始");
// 抽方法：调用处和声明都有 `extracted(`，取短的那笔（调用处）
eq(
  nameSpot([{ insert: "extracted(a, b);" }, { insert: "\n\n    private void extracted(int a, int b) {\n        System.out.println(a + b);\n    }" }]),
  { index: 0, offset: 0 },
  "抽方法：调用处",
);
eq(nameSpot([{ insert: "int r = extracted(a);" }, { insert: "private int extracted(int a) { return a; }" }]), { index: 0, offset: 8 }, "调用处在赋值右边");
// jdtls 1.61 真给的形状：一笔改动里全有，比原文认新名字（出现次数最多的那个）
eq(
  nameSpot([{ old: "int total = qty * 100 + 5;\n        int other = qty * 100", insert: "int i = qty * 100;\n\t\tint total = i + 5;\n        int other = i" }]),
  { index: 0, offset: 4 },
  "抽变量一整段：新名字 i（声明一次、用两次）",
);
eq(
  nameSpot([{ old: 'System.out.println("hello " + name);\n        System.out.println("bye " + name);\n    }', insert: 'extracted(name);\n    }\n\n\tprivate void extracted(String name) {\n\t\tSystem.out.println("hello " + name);\n\t}' }]),
  { index: 0, offset: 0 },
  "抽方法一整段：extracted 出现两次（调用、声明），String 只多出一次 —— 取前者、停在调用处",
);
eq(nameSpot([{ old: "a + b", insert: "a + b" }]), null, "没有新名字：null");
// println( 只出现在声明里那一笔，不能当成新名字
eq(nameSpot([{ insert: "System.out.println(x);" }]), null, "只有一笔、认不出新名字：null（照样抽，不接着改名）");
eq(nameSpot([{ insert: "\n" }, { insert: "   " }]), null, "空白不是名字");

console.log(fail === 0 ? `✅ 重构：${pass} 通过，0 失败` : `❌ 重构：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
