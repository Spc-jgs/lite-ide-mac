/**
 * `.http` 文件里不碰编辑器的那一半（src/lib/http/http-file.ts）：切出每个请求、名字、头和体、`< 文件`、响应处理脚本、变量替换。
 * 每一条都是「发出去的东西和文件里写的不一样」的一种形状 —— 那种错不报错，只是服务器回一个莫名其妙的 400。
 */
import { parseHttp, requestAt, resolve, dynamic, tabName } from "../src/lib/http/http-file.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

const SRC = `@host = localhost:8080
@base = http://{{host}}/api

### 查订单
GET {{base}}/orders?status=PAID
    &page=1
Accept: application/json
# 头中间的注释不算头
X-Trace: {{$uuid}}

###
# @name createOrder
// @no-redirect
POST http://{{host}}/api/orders HTTP/1.1
Content-Type: application/json
Authorization: Bearer {{token}}

{
  "sku": "{{sku}}",
  "nested": {"a": {"b": 1}}
}

> {%
  client.global.set("id", response.body.id);
%}

### 从文件读请求体
PUT http://{{host}}/api/orders/1
Content-Type: application/json

< ./body.json

### 只写了 URL
https://example.com/health

### 不是请求的一块：说明文字
这里是一段说明，不该被当成请求
`;

const f = parseHttp(SRC);
eq(f.vars, { host: "localhost:8080", base: "http://{{host}}/api" }, "文件变量");
eq(f.requests.map((r) => [r.line, r.method, r.name]), [[5, "GET", "查订单"], [14, "POST", "createOrder"], [28, "PUT", "从文件读请求体"], [34, "GET", "只写了 URL"]], "四个请求（说明文字那块不算）：请求行、方法、名字");
const [get, post, put, bare] = f.requests;
eq(get.url, "{{base}}/orders?status=PAID&page=1", "缩进的 & 行接在 URL 后面");
eq(get.headers, [["Accept", "application/json"], ["X-Trace", "{{$uuid}}"]], "头（注释跳过）");
eq(get.body, null, "GET 没有体");
ok(get.follow && !post.follow, "# @no-redirect / // @no-redirect");
eq(post.url, "http://{{host}}/api/orders", "HTTP/1.1 不算 URL 的一部分");
eq(post.body, '{\n  "sku": "{{sku}}",\n  "nested": {"a": {"b": 1}}\n}', "体原样（响应处理脚本摘掉、首尾空行去掉）");
ok(post.handler, "认出了响应处理脚本（没跑，要说一句）");
eq([put.bodyFile, put.body], ["./body.json", null], "< 文件");
eq([bare.method, bare.url], ["GET", "https://example.com/health"], "只写 URL 就是 GET");

// 光标在哪就发哪个：两个请求之间的空行算上面那个；第一个请求上面的注释算第一个
eq(requestAt(f, 9)?.line, 5, "光标在第一个请求的头里");
eq(requestAt(f, 12)?.line, 5, "`###` 之前的空行算上面那个");
eq(requestAt(f, 20)?.line, 14, "光标在体里");
eq(requestAt(parseHttp("# 说明\n\nGET http://a/b\n"), 1)?.line, 3, "第一个请求上面的注释");

// 变量：文件变量里引用别的文件变量 / 环境；环境按路径取；没定义的不发、全列出来
const env = { token: "t-123", sku: "A-1", client: { id: 7, tags: ["x"] } };
const r = resolve(post, f.vars, env);
ok(r.ok, `都定义了：${JSON.stringify(r)}`);
if (r.ok) {
  eq(r.req.url, "http://localhost:8080/api/orders", "URL 里的文件变量");
  eq(r.req.headers[1], ["Authorization", "Bearer t-123"], "头里的环境变量");
  ok(r.req.body!.includes('"sku": "A-1"') && r.req.body!.includes('{"a": {"b": 1}}'), `体里的变量换了、JSON 自己的花括号没被当成变量：${r.req.body}`);
}
const g = resolve(get, f.vars, env);
ok(g.ok && g.req.url === "http://localhost:8080/api/orders?status=PAID&page=1", `文件变量的值里引用文件变量（展开一层）：${JSON.stringify(g)}`);
ok(g.ok && /^[0-9a-f-]{36}$/.test(g.req.headers[1][1]), "{{$uuid}}");
const nested = resolve(parseHttp("GET http://a/{{client.id}}?t={{client.tags}}\n").requests[0], {}, env);
ok(nested.ok && nested.req.url === 'http://a/7?t=["x"]', `{{a.b}} 按路径取，对象给 JSON：${JSON.stringify(nested)}`);
const miss = resolve(post, f.vars, { sku: "A-1" });
eq(miss.ok ? [] : miss.missing, ["token"], "没定义的列出来、不发");
const miss2 = resolve(parseHttp("GET http://a/{{$nope}}/{{x}}\n").requests[0], {}, null);
eq(miss2.ok ? [] : miss2.missing, ["$nope", "x"], "认不出的动态变量也算没定义（没选环境时环境是 null）");

// 动态变量
const now = new Date("2026-10-20T01:02:03.000Z");
eq([dynamic("$timestamp", now), dynamic("$isoTimestamp", now)], ["1792458123", "2026-10-20T01:02:03.000Z"], "时间戳");
const n = Number(dynamic("$random.integer(5, 7)"));
ok(n >= 5 && n < 7, `$random.integer(5, 7)：${n}`);
ok(/^[0-9a-f]{6}$/.test(dynamic("$random.hexadecimal(6)") ?? ""), "$random.hexadecimal(6)");
eq(dynamic("$random.integer(a, 2)"), undefined, "参数不对的不算");

// Cookie 罐的开关、响应脚本摘出来交给 http-script（#75 #76）
const sc = parseHttp(`### 登录
# @no-cookie-jar
POST http://a/login
Content-Type: application/json

{"u": "admin"}

> {%
  client.global.set("token", response.body.token);
%}

### 一行的
GET http://a/me
> {% client.global.set("me", response.body.id); %}

### 外面的脚本文件
GET http://a/x
> ./handler.js
`).requests;
eq(sc.map((r) => r.cookies), [false, true, true], "# @no-cookie-jar 只管它那一个");
eq(sc.map((r) => r.script), ['client.global.set("token", response.body.token);', 'client.global.set("me", response.body.id);', null], "脚本的文本摘出来（多行的、一行的）");
eq(sc[0].body, '{"u": "admin"}', "脚本不进请求体");
eq([sc[2].handler, sc[2].scriptFile, sc[2].script], [true, "./handler.js", null], "> 文件：认出来、不认它的内容");
// 脚本存下的变量排在环境前面：环境里那个过期的 token 要被登录拿到的盖过；文件里的 @变量 还是最先
const pri = parseHttp("@who = file\nGET http://a/{{token}}/{{who}}/{{sku}}\n");
const rp = resolve(pri.requests[0], pri.vars, { token: "old", who: "env", sku: "S" }, { token: "new", who: "global" });
eq(rp.ok ? rp.req.url : rp, "http://a/new/file/S", "优先级：文件变量 > 脚本存的 > 环境");

eq([tabName(get), tabName({ ...get, name: null }), tabName({ ...bare, name: null })], ["查订单", "GET /orders?status=PAID&page=1", "GET /health"], "运行窗那一格的名字");

console.log(`${fail === 0 ? "✅" : "❌"} .http 文件：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
