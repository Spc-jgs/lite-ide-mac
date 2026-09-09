import { errText } from "../src/lib/state/err-text.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

/*
 * 这条测试卡的是一个真的漏出去过的形状：
 * `gitSwitch` reject 的是 `SwitchErr` 对象，而 `notify.block` 原来直接
 * `String(body)` —— 分支不存在、名字重了这类失败在横幅上就是
 * 七个字 `[object Object]`。
 */
{
  const switchErr = {
    kind: "other",
    message: "分支 feature/x 已经存在",
    files: [] as string[],
    raw: "fatal: a branch named 'feature/x' already exists",
  };
  const t = errText(switchErr);
  ok(t === "分支 feature/x 已经存在", `SwitchErr 该取 message，实得 [${t}]`);
  ok(!t.includes("[object"), "不许出现 [object Object]");
}

// message 是空的时候退到 raw —— Rust 侧 Display 为空的那几档会这样
{
  const t = errText({ kind: "other", message: "", raw: "fatal: 原话" });
  ok(t === "fatal: 原话", `message 空该退到 raw，实得 [${t}]`);
}

// 认不出来的对象也不能变成 [object Object]：宁可给 JSON，信息还在
{
  const t = errText({ code: 128, detail: { why: "谁知道" } });
  ok(!t.includes("[object"), `认不出的对象不许变成 [object Object]，实得 [${t}]`);
  ok(t.includes("128"), `信息该留住，实得 [${t}]`);
}

// 循环引用不能把它弄崩 —— 这段跑在 catch 里，它自己再抛就把错误吞了
{
  const cyc: Record<string, unknown> = { a: 1 };
  cyc.self = cyc;
  let threw = false;
  let t = "";
  try { t = errText(cyc); } catch { threw = true; }
  ok(!threw, "循环引用不许抛");
  ok(t.length > 0, "循环引用也要给出点什么");
}

// 常规的三种
{
  ok(errText("就是一句话") === "就是一句话", "字符串原样");
  ok(errText(new Error("Error: 前缀要去掉")) === "前缀要去掉", "Error 取 message 并去前缀");
  ok(errText("Error: 字符串上的前缀也去掉") === "字符串上的前缀也去掉", "字符串的 Error: 前缀");
}

console.log(`✅ 错误文案：${pass} 通过，${fail} 失败`);
if (fail > 0) process.exit(1);
