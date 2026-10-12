/** 桩：日志块的线格式编码，`log.ts` 的 `log_lines` 用它把行打包成和 Rust 侧一样的字节。 */
const enc = new TextEncoder();

/** 与 Rust 侧 `MAX_LINE_BYTES` 一致：一行最多传这么多字节 */
const MAX_LINE_BYTES = 64 * 1024;

/** 与 Rust 侧 block::encode 完全一致的线格式（含截过的行：长度最高位置 1、后面跟一个 u64 原长）。
 * `tests/log-block.test.ts` 直接拿它测：测试原来自己抄了一份，桩这份没人测 —— 改格式时漏改桩，`pnpm dev` 里乱码而测试全绿（code review 2026-10-12）。
 * 单独一个文件、不 import 任何东西：`pnpm test` 是裸 node 跑的，不认 `./data` 这种不带后缀的导入 */
export function encodeBlock(first: number, texts: string[]): ArrayBuffer {
  const full = texts.map((t) => enc.encode(t));
  const parts = full.map((p) => p.subarray(0, MAX_LINE_BYTES));
  const total = 12 + 4 * parts.length + parts.reduce((a, b, i) => a + b.length + (full[i].length > b.length ? 8 : 0), 0);
  const buf = new ArrayBuffer(total);
  const dv = new DataView(buf);
  const u8 = new Uint8Array(buf);
  dv.setBigUint64(0, BigInt(first), true);
  dv.setUint32(8, parts.length, true);
  let pos = 12 + 4 * parts.length;
  parts.forEach((p, i) => {
    const clipped = full[i].length > p.length;
    dv.setUint32(12 + i * 4, (p.length | (clipped ? 0x8000_0000 : 0)) >>> 0, true);
    u8.set(p, pos);
    pos += p.length;
    if (clipped) {
      dv.setBigUint64(pos, BigInt(full[i].length), true);
      pos += 8;
    }
  });
  return buf;
}
