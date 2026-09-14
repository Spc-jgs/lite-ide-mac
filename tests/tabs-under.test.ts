import { underPath } from "../src/lib/state/tab.ts";

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

// 「某条路径底下」是文件树改名 / 删除时决定哪些标签跟着走的唯一判据
ok(underPath("/p/a.ts", "/p/a.ts", false), "文件本身");
ok(!underPath("/p/a.ts", "/p/a", false), "同前缀的另一个文件不算（非目录）");
ok(underPath("/p/src/a.ts", "/p/src", true), "目录底下的文件");
ok(underPath("/p/src/x/y/a.ts", "/p/src", true), "多层底下也算");
ok(!underPath("/p/srcx/a.ts", "/p/src", true), "`/p/srcx` 不在 `/p/src` 底下 —— 前缀要带斜杠");
ok(!underPath("/p/src/a.ts", "/p/src", false), "不是目录时只认完全相等");
ok(underPath("/p/src", "/p/src", true), "目录自己也算在自己底下");

console.log(`${fail === 0 ? "✅" : "❌"} 路径归属：${pass} 通过，${fail} 失败`);
if (fail > 0) process.exit(1);
