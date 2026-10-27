/**
 * 界面文案用产品话（.claude/rules/ui.md 第十八条，2026-10-27 用户：「弹窗内容和按钮过于口语化，我们需要产品话」）。
 *
 * 10-27 改过一轮，10-28 又在运行列表里看到「你建的」、按钮提示里的「重跑」、差异视图的「未改动」—— 靠记性守不住。
 * 这里只卡第十八条术语表里**已经定死要换掉**的那几个词，而且只看界面代码里的字（注释去掉之后）：
 * 注释、文档照旧可以写口语。AI 助手（src/lib/agent/）、开发桩和样例页（src/lib/dev/）不在范围里。
 *
 * 新加一个词之前先 grep 一遍：要么同一轮全改掉，要么别加 —— 一加就红的规则没人会信。
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

/** 术语表里「不用」的那一列，换成什么写在后面（报错时给出来） */
const BANNED: [RegExp, string][] = [
  [/你建的/, "自定义"],
  [/重跑/, "重新运行"],
  [/知道了/, "好"],
  [/改动/, "更改 / 修改"],
  [/起不来|读不了|拿不到/, "无法 + 动词"],
  [/扔进废纸篓/, "移到废纸篓"],
  [/撤不回来/, "无法撤销"],
];

/** 去掉注释：`//` 到行尾（不在字符串里的那种，够用：误判只会让测试更松）、块注释、HTML 注释 */
export function stripComments(src: string): string {
  return src
    .replace(/<!--[\s\S]*?-->/g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/(^|[^:"'`\\])\/\/.*$/gm, "$1");
}

// ── 先验规则本身 ──
{
  const code = stripComments('// 重跑那一下\nconst a = "重新运行"; /* 改动 */\n<!-- 你建的 -->\n<b>{n} 行未改动</b>\nconst u = "https://x"; // 知道了');
  ok(!/重跑|你建的|知道了/.test(code), `注释里的不算：${code}`);
  ok(/行未改动/.test(code), "界面里的字要认出来");
  ok(/https:\/\/x/.test(code), "字符串里的 // 不当注释");
}

// ── 扫 src/lib ──
const root = join(import.meta.dirname, "..", "src", "lib");
const found: string[] = [];
const walk = (dir: string) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (name === "agent" || name === "dev") continue;
      walk(p);
    } else if (/\.(svelte|ts)$/.test(name) && !name.endsWith(".test.ts")) {
      const lines = stripComments(readFileSync(p, "utf8")).split("\n");
      lines.forEach((l, i) => {
        for (const [re, use] of BANNED) if (re.test(l)) found.push(`${relative(root, p)}:${i + 1}  「${l.trim().slice(0, 80)}」→ 用「${use}」`);
      });
    }
  }
};
walk(root);
// 自检器的诊断（invariant.ts）只进日志、不进界面
const real = found.filter((f) => !f.startsWith("state/invariant.ts"));
ok(real.length === 0, `界面文案里还有口语（ui.md 第十八条）：\n    ${real.join("\n    ")}`);

console.log(`${fail === 0 ? "✅" : "❌"} 界面文案：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
