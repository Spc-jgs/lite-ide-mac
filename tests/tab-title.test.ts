import { wrapsByDefault, scratchTitle } from "../src/lib/state/tab.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) {
    pass++;
  } else {
    fail++;
    console.error("  ✗ " + m);
  }
};

// ── wrapsByDefault：文本换、代码不换 ──
ok(wrapsByDefault("/s/2026-09-16 1103.md"), "markdown 换");
ok(wrapsByDefault("/x/notes.TXT"), "txt 换（不分大小写）");
ok(wrapsByDefault("/var/log/app.log"), "log 换");
ok(wrapsByDefault("/proj/README"), "没有扩展名的当文本，换");
ok(!wrapsByDefault("/proj/src/main.rs"), "代码不换");
ok(!wrapsByDefault("/proj/a.json"), "json 不换");
ok(!wrapsByDefault("/proj/.gitignore"), ".gitignore 有扩展名形状，不换");

// ── scratchTitle：第一行有字的，去 # 前缀，截 30 字符 ──
ok(scratchTitle("# 周会要点\n\n- a") === "周会要点", "去掉 markdown 标题前缀");
ok(scratchTitle("\n\n   \ncurl -s x\n") === "curl -s x", "跳过空行和空白行");
ok(scratchTitle("") === undefined, "空文件没有标题（回落到文件名）");
ok(scratchTitle("   \n\n") === undefined, "全是空白也没有");
ok(scratchTitle("很".repeat(50))?.length === 30, "截到 30 个字符");
ok(scratchTitle("😀".repeat(40))!.length === 60 && [...scratchTitle("😀".repeat(40))!].length === 30, "按字符不按 UTF-16 单元截");

console.log(`${fail === 0 ? "✅" : "❌"} 标签名与换行：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
