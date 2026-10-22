/**
 * 日志行详情的纯逻辑（`logview/line-detail.ts`，#64）：整行怎么解（截过的不出乱码、GBK 照解）、行里的 JSON 怎么找
 */
import { decodeLineFull, extractJson, sizeOf } from "../src/lib/logview/line-detail.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const eq = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** 照 Rust 的 `log_line_full` 打包：8 字节真长度 + 原文 */
const pack = (bytes: Uint8Array, total = bytes.byteLength) => {
  const out = new Uint8Array(8 + bytes.byteLength);
  new DataView(out.buffer).setBigUint64(0, BigInt(total), true);
  out.set(bytes, 8);
  return out.buffer;
};

// 解整行
{
  const u8 = new TextEncoder().encode("订单 42 已支付");
  ok(eq(decodeLineFull(pack(u8), "utf-8"), { text: "订单 42 已支付", bytes: u8.byteLength, total: u8.byteLength }), "完整的一行照原样解");
  // 截在「付」（3 字节）的第 2 个字节上：流式解码不出半个字，不出 �
  const cut = u8.slice(0, u8.byteLength - 1);
  const r = decodeLineFull(pack(cut, u8.byteLength), "utf-8");
  ok(r?.text === "订单 42 已支" && r.total === u8.byteLength, `截在半个字上：末尾那半个不出乱码（得到 ${JSON.stringify(r?.text)}）`);
  // GBK：「订单」= b6 a9 b5 a5
  ok(decodeLineFull(pack(new Uint8Array([0xb6, 0xa9, 0xb5, 0xa5])), "gbk")?.text === "订单", "GBK 的行按文件的编码解");
  ok(decodeLineFull(new ArrayBuffer(0), "utf-8") === null, "空的 = 没有这一行");
  ok(decodeLineFull(pack(new TextEncoder().encode("x")), "no-such-encoding")?.text === "x", "认不出的编码名退回 UTF-8，不抛");
}

// 找 JSON
{
  const r = extractJson('2026-10-22 10:00:01 INFO [http-nio-8080-exec-1] c.d.OrderApi - req payload={"orderId":42,"items":[{"sku":"A","n":1}],"memo":"a}b"} cost=3ms');
  ok(r !== null && eq(r.value, { orderId: 42, items: [{ sku: "A", n: 1 }], memo: "a}b" }), "前缀里的 [线程名] 不算；字符串里的 } 不算配对");
  ok(r?.before.endsWith("payload=") && r.after === " cost=3ms", `前缀和尾巴分开：${JSON.stringify([r?.before.slice(-8), r?.after])}`);
  ok(extractJson("[INFO] started in 3.2s") === null, "[INFO] 不是 JSON");
  ok(extractJson("args={} list=[]") === null, "空对象、空数组不算（排版没意义）");
  ok(extractJson("ids=[1,2,3]") === null, "纯数字数组不算");
  const two = extractJson('a={"x":1} b={"y":{"z":[1,2]},"w":2}');
  ok(eq(two?.value, { y: { z: [1, 2] }, w: 2 }), "两段 JSON 取最长的那段");
  ok(eq(extractJson('[{"a":1},{"a":2}]')?.value, [{ a: 1 }, { a: 2 }]), "整行是对象数组");
  ok(extractJson('broken={"a":1') === null, "没闭合的不算");
  // 1MB 全是开括号：试的次数有上限，不卡
  const t0 = performance.now();
  extractJson("{".repeat(1 << 20));
  const ms = performance.now() - t0;
  ok(ms < 2000, `1MB 全是开括号也很快回来（${ms.toFixed(0)}ms）`);
}

ok(sizeOf(500) === "500 B" && sizeOf(2048) === "2.0 KB" && sizeOf(52_323_942) === "49.9 MB", "大小说成人话");

console.log(`${fail === 0 ? "✅" : "❌"} 日志行详情：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
