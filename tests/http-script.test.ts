/**
 * 响应处理脚本的小子集（src/lib/http/http-script.ts，#76）：只认 `client.global.set(名称, 取值)`，**不执行 JavaScript**。
 * 这里的每一条都是「以为设上了、其实没有」或「执行了文件里不该执行的东西」的一种形状。
 */
import { runScript } from "../src/lib/http/http-script.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

const resp = {
  headers: [{ name: "X-Trace-Id", value: "t-9" }, { name: "Content-Type", value: "application/json" }],
  body: JSON.stringify({ code: 0, data: { token: "abc", roles: ["admin", "dev"], "user-id": 7, ok: true, none: null }, list: [{ id: 3 }] }, null, 2),
};

const r = runScript(
  `// 登录拿 token
  client.global.set("token", response.body.data.token);
  client.global.set('first', response.body.list[0].id)
  client.global.set("uid", response.body.data["user-id"]);
  client.global.set("roles", response.body.data.roles);
  client.global.set("flag", response.body.data.ok); /* 布尔 */
  client.global.set("trace", response.headers.valueOf("x-trace-id"));
  client.global.set("tenant", "acme");`,
  resp,
);
eq(r.set, [["token", "abc"], ["first", "3"], ["uid", "7"], ["roles", '["admin","dev"]'], ["flag", "true"], ["trace", "t-9"], ["tenant", "acme"]], "按路径取、下标、带 - 的键、数组给 JSON、头名不分大小写、字面量");
eq([r.skipped, r.errors], [[], []], "都认得、都取到了");

const bad = runScript(
  `client.global.set("a", response.body.data.nope);
  client.global.set("b", response.headers.valueOf("X-None"));
  client.global.set("n", response.body.data.none);
  client.test("登录成功", function () {
    client.assert(response.status === 200, "状态码不对; 应为 200");
  });
  client.global.set("c", response.body.data.token + "-x");
  require("child_process").exec("rm -rf ~");`,
  resp,
);
eq(bad.set, [["n", "null"]], "取不到的不设（不设一个 undefined 进去）；null 照实给");
eq(bad.errors, ["「a」未设置：响应体中不存在 response.body.data.nope", "「b」未设置：响应中没有 X-None 头"], "取不到的说清楚");
eq(bad.skipped, ['client.test("登录成功", function () {\n    client.assert(response.status === 200, "状态码不对; 应为 200");\n  })', 'client.global.set("c", response.body.data.token + "-x")', 'require("child_process").exec("rm -rf ~")'], "别的语句、不认的取值写法：不执行、原样列出来");

const html = runScript('client.global.set("t", response.body.token)', { headers: [], body: "<html>500</html>" });
eq([html.set, html.errors], [[], ["「t」未设置：响应体不是 JSON，无法读取 response.body.token"]], "不是 JSON 的响应体");
eq(runScript('client.global.set("u", "http://a/b")', resp).set, [["u", "http://a/b"]], "字符串里的 // 不当注释");
eq(runScript('client.global.set("s", "a;b")', resp).set, [["s", "a;b"]], "字符串里的分号不切");

console.log(`${fail === 0 ? "✅" : "❌"} 响应处理脚本：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
