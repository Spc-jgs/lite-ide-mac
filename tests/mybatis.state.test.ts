/**
 * MyBatis 跳转（docs/IDEA.md 第 24 节）前端那半：拿 Rust 的回答（这边的点 + 对面的点）按名字配对、画在哪一行、点了去哪。
 * 跑在裸 node 里，IPC 走浏览器桩（mock/java.ts，规则同 mybatis.rs 的简化版；规则本身在 Rust 的测试里）。
 *
 * 「说谎」的形状：XML 里没有的方法也画了图标（点了没反应）；图标画在了别的行上；点了跳到对面别的地方
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { EditorState } = await import("@codemirror/state");
const { FILES } = await import("../src/lib/dev/mock/data");
const { marksFor } = await import("../src/lib/editor/mybatis-gutter");
const { mybatisForJava, mybatisForXml } = await import("../src/lib/ipc/java");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const JAVA = "/proj/moduleB/src/main/java/com/demo/core/OrderMapper.java";
const XML = "/proj/moduleB/src/main/resources/mapper/OrderMapper.xml";
const lines = (st: InstanceType<typeof EditorState>, set: ReturnType<typeof marksFor>) => {
  const out: string[] = [];
  for (let it = set.iter(); it.value; it.next()) {
    const to = (it.value as unknown as { to: { path: string; line: number } }).to;
    out.push(`${st.doc.lineAt(it.from).number}→${to.path.slice(to.path.lastIndexOf("/") + 1)}:${to.line}`);
  }
  return out.join(",");
};

// ── 接口 → XML：XML 里有的两个方法画图标，count（MyBatis-Plus 那种通用方法，XML 里没有）不画 ──
{
  const text = FILES[JAVA] as string;
  const st = EditorState.create({ doc: text });
  const got = lines(st, marksFor(st, await mybatisForJava("/proj", text), true));
  ok(got === "6→OrderMapper.xml:4,8→OrderMapper.xml:8", `接口方法旁边：${got}`);
}

// ── XML → 接口：两段语句都指回接口的方法那一行 ──
{
  const text = FILES[XML] as string;
  const st = EditorState.create({ doc: text });
  const got = lines(st, marksFor(st, await mybatisForXml("/proj", text), false));
  ok(got === "4→OrderMapper.java:6,8→OrderMapper.java:8", `XML 语句旁边：${got}`);
}

// ── 不是 Mapper：一个都不画 ──
{
  const text = FILES["/proj/moduleB/src/main/java/com/demo/core/OrderClient.java"] as string;
  const st = EditorState.create({ doc: text });
  ok(marksFor(st, await mybatisForJava("/proj", text), true).size === 0, "普通的类不画");
}

console.log(`${fail === 0 ? "✅" : "❌"} MyBatis 跳转（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
