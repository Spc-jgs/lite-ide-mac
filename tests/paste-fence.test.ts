import { looksLikeJsonBlock, fenced } from "../src/lib/editor/paste-fence.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

ok(looksLikeJsonBlock('{"a":1,"b":[1,2]}'), "对象是");
ok(looksLikeJsonBlock('  [1, 2, 3]\n'), "数组是（两头空白不算）");
ok(!looksLikeJsonBlock("42"), "一个数不是 —— 那是句子里粘个值");
ok(!looksLikeJsonBlock('"str"'), "一个字符串不是");
ok(!looksLikeJsonBlock("{a: 1}"), "解析不过的不猜（猜错的代价是把正文包进代码块）");
ok(!looksLikeJsonBlock("{'a': 1}"), "单引号不猜");
ok(!looksLikeJsonBlock(""), "空");
ok(!looksLikeJsonBlock("{"), "半个括号");
ok(fenced('{"a":1}') === '```json\n{\n  "a": 1\n}\n```', `压扁的会被展开成两空格缩进：${JSON.stringify(fenced('{"a":1}'))}`);

console.log(`${fail === 0 ? "✅" : "❌"} 粘贴围栏：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
