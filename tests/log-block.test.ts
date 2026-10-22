/**
 * 日志块的线格式解码（`src/lib/logview/block.ts`）：截过的长行要带出「还有多少没显示」。
 * 编码用桩的 `encodeBlock`（浏览器里 `pnpm dev` 喂给日志视图的就是它）：同一份编码器既喂界面又被测，桩漂了这里就红。
 * 它和 Rust 侧 `logengine::block::encode` 的一致由两边的测试各卡一头。
 */
import { decodeBlock, cutNote } from "../src/lib/logview/block.ts";
import { encodeBlock as encode } from "../src/lib/dev/mock/block.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const MAX = 64 * 1024;

// 普通的块：没有截过的行，more 是 null（绝大多数块，别为它们白建一个数组）
{
  const b = decodeBlock(encode(7, ["a", "订单超时"]));
  ok(b.firstLine === 7 && b.lines.join("|") === "a|订单超时", "普通行原样解出来");
  ok(b.more === null, "没截过的块 more 是 null");
}

// 夹在中间的一条长行：前后两行不受影响（标记位和尾巴的 8 字节没把切分弄乱），长行带出还剩多少
{
  const long = "x".repeat(MAX * 3 + 10);
  const b = decodeBlock(encode(0, ["前", long, "后"]));
  ok(b.lines[0] === "前" && b.lines[2] === "后", `前后两行不受影响：${JSON.stringify([b.lines[0], b.lines[2]])}`);
  ok(b.lines[1].length === MAX, `长行只传了前 64KB：${b.lines[1].length}`);
  ok(b.more !== null && b.more[1] === MAX * 2 + 10, `还剩多少要是真数字：${b.more?.[1]}`);
  ok(b.more !== null && b.more[0] === 0 && b.more[2] === 0, "没截的行是 0");
}

// 正好 64KB 不算截过
{
  const b = decodeBlock(encode(0, ["y".repeat(MAX)]));
  ok(b.more === null, "正好到上限不算截过");
}

// 那句话：数字要真、单位跟着大小走
ok(cutNote(500) === "…这一行还有 500 字节没显示，双击看整行", cutNote(500));
ok(cutNote(1536) === "…这一行还有 1.5 KB 没显示，双击看整行", cutNote(1536));
ok(cutNote(50 * 1024 * 1024) === "…这一行还有 50.0 MB 没显示，双击看整行", cutNote(50 * 1024 * 1024));

console.log(`${fail === 0 ? "✅" : "❌"} 日志块解码：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
