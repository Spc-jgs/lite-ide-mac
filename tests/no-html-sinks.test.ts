/**
 * 页面里不许直接插 HTML（2026-10-10 整体审核）。
 *
 * 为什么卡这一条：IPC 能读写任意文件、能往终端里写命令 —— **页面里一旦能跑别人的脚本，就等于拿到这台机器**。
 * 防线是两层：CSP 只许跑自己打包的脚本（不许内联、不许 eval）；页面里所有文字都走 Svelte 的文本插值（自动转义）。
 * 后一层原来是靠大家自觉：审核时全仓库一处 `{@html}` 都没有、`innerHTML` 只有一处清空 —— 这条测试把「没有」守住。
 *
 * 真要渲染富文本（以后做 Markdown 预览之类）：先过一个白名单清洗，再把这里的规则改成认那个函数，别直接放开。
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

/** 一行里有没有「把一段字符串当 HTML 插进页面」的写法。`innerHTML = ""`（清空）放行 */
function sink(line: string): string | null {
  if (/\{@html\b/.test(line)) return "{@html}";
  if (/\binsertAdjacentHTML\s*\(/.test(line)) return "insertAdjacentHTML";
  if (/\bdocument\.write(ln)?\s*\(/.test(line)) return "document.write";
  if (/\bouterHTML\s*=(?!=)/.test(line)) return "outerHTML =";
  const m = line.match(/\binnerHTML\s*=(?!=)\s*(.*)$/);
  if (m && !/^(""|''|``)\s*;?\s*(\/\/.*)?$/.test(m[1].trim())) return "innerHTML =";
  return null;
}

// ── 先验检测本身：规则写坏了的话，下面那段扫全仓库就成了永远绿的空断言 ──
{
  const bad = ['{@html marked(md)}', 'el.innerHTML = text;', "el.innerHTML = `<b>${x}</b>`", 'el.insertAdjacentHTML("beforeend", s)', 'node.outerHTML = s', "document.write(s)"];
  for (const b of bad) ok(sink(b) !== null, `该认出来：${b}`);
  const fine = ['root.innerHTML = "";', "root.innerHTML = ''", "if (el.innerHTML === s) {}", "const innerHTMLish = 1", "// 不用 {@ html}"];
  for (const f of fine) ok(sink(f) === null, `不该误报：${f}`);
}

// ── 扫 src/ ──
const root = join(import.meta.dirname, "..", "src");
const found: string[] = [];
const walk = (dir: string) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(svelte|ts)$/.test(name)) {
      readFileSync(p, "utf8").split("\n").forEach((line, i) => {
        const s = sink(line);
        if (s) found.push(`${relative(root, p)}:${i + 1}  ${s}`);
      });
    }
  }
};
walk(root);
ok(found.length === 0, `页面里直接插 HTML 的地方（要么改成文本插值，要么先过清洗、再改这条规则）：\n    ${found.join("\n    ")}`);

console.log(`${fail === 0 ? "✅" : "❌"} 不许直接插 HTML：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
