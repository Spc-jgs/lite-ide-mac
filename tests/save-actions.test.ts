/**
 * 存盘时照 .editorconfig 去行尾空格、补 / 去末尾换行（src/lib/editor/save-actions.ts）。
 * 形状：把光标那行正在打的空格吃掉；CRLF 文件里把 \r 当空白删掉（整份换行符就乱了）；false 当成「不管」；改动重叠（CM6 会直接抛错）
 */
import { saveChanges, applyChanges } from "../src/lib/editor/save-actions.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);
const run = (t: string, trim: boolean | null, nl: boolean | null, keep: number[] = []) => applyChanges(t, saveChanges(t, { trim, finalNewline: nl }, new Set(keep)));

eq(run("a  \nb\t\n  c", true, null), "a\nb\n  c", "去行尾空格和 Tab，行首缩进不动");
eq(run("a  \nfoo \nc  ", true, null, [2]), "a\nfoo \nc", "光标所在那一行的行尾空格留着");
eq(run("a  \r\nb\r\n", true, null), "a\r\nb\r\n", "CRLF：\\r 不算空白");
eq(run("a\nb", null, true), "a\nb\n", "补末尾换行");
eq(run("a\r\nb", null, true), "a\r\nb\r\n", "CRLF 文件补的是 CRLF");
eq(run("a\nb\n", null, true), "a\nb\n", "已经有了不重复补");
eq(run("", null, true), "", "空文件不补");
eq(run("a\nb\n\n\n", null, false), "a\nb", "false 是「末尾不要换行」");
eq(run("a\n   \n", true, false), "a", "最后一行全是空白、两条规则都碰到结尾：改动不重叠");
eq(run("x\nfoo ", true, true, [2]), "x\nfoo \n", "光标在最后一行、正打着空格：空格留着、只补换行");
eq(run("x\nfoo ", true, true), "x\nfoo\n", "光标不在那一行：照常去掉");
eq(saveChanges("a\nb\n", { trim: null, finalNewline: null }), [], "都没写：一处不改");
eq(saveChanges("clean\n", { trim: true, finalNewline: true }), [], "已经干净的：一处不改（不产生空的修改，撤销栈里不多一笔）");
const cs = saveChanges("x \n   \n", { trim: true, finalNewline: false });
ok(cs.every((c, i) => i === 0 || c.from >= cs[i - 1].to), `改动按次序、互不重叠（CM6 要求）：${JSON.stringify(cs)}`);

console.log(`${fail === 0 ? "✅" : "❌"} 存盘处理：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
