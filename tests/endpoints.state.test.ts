/**
 * 随处搜索里的「接口」怎么排（src/lib/search/endpoints.ts，docs/IDEA.md 第 25 节）。
 * 最要紧的一条：粘一个日志里的真 URL（带主机、带查询串、id 是具体的数）能找到模板是 `{id}` 的那个接口，而且整个对上的排在前面
 */
import { looksLikeUrl, rankEndpoints, urlMatch, urlPath } from "../src/lib/search/endpoints";

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const ep = (methods: string[], path: string, method: string) => ({ methods, path, controller: "OrderController", method, file: "/p/OrderController.java", line: 1 });
const LIST = [
  ep(["GET"], "/api/orders", "list"),
  ep(["GET"], "/api/orders/{id}", "get"),
  ep(["GET"], "/api/orders/{id}/items", "items"),
  ep(["POST"], "/api/orders/search", "search"),
  ep(["GET"], "/api/users/{id}", "user"),
];
const paths = (q: string, n = 10) => rankEndpoints(LIST, q, n).map((h) => h.item.path).join(" | ");

ok(urlPath("http://localhost:8080/api/orders/42?x=1#a") === "/api/orders/42", `去掉协议、主机、查询串：${urlPath("http://localhost:8080/api/orders/42?x=1#a")}`);
ok(urlPath("/api/orders/") === "/api/orders" && urlPath("/") === "/", "末尾的 / 去掉（根除外）");
ok(looksLikeUrl("/api") && looksLikeUrl("https://x/y") && !looksLikeUrl("orders"), "像不像地址");
ok(urlMatch("/api/orders/{id}", "/api/orders/42") === 0, "{id} 对任意一段：整个对上");
ok(urlMatch("/api/orders/{id}/items", "/api/orders/42") === 1, "对上开头（还没打完）");
ok(urlMatch("/api/orders/search", "/api/orders/sea") === 1, "最后一段打了一半");
ok(urlMatch("/api/users/{id}", "/api/orders/42") === -1, "对不上");
ok(paths("http://localhost:8080/api/orders/42?expand=true") === "/api/orders/{id} | /api/orders/{id}/items", `粘一个真 URL：整个对上的在前、开头对上的在后：${paths("http://localhost:8080/api/orders/42?expand=true")}`);
ok(paths("/api/orders/42/items") === "/api/orders/{id}/items", `更长的那个：${paths("/api/orders/42/items")}`);
ok(paths("/api/orders/search") === "/api/orders/search | /api/orders/{id} | /api/orders/{id}/items", `写死的比 {id} 先（两个都整个对上时 Spring 选的就是写死的那个）：${paths("/api/orders/search")}`);
ok(paths("ordersitems").startsWith("/api/orders/{id}/items"), `不像地址：模糊匹配路径 + 方法名：${paths("ordersitems")}`);
ok(paths("user").startsWith("/api/users/{id}"), `按方法名也找得到：${paths("user")}`);

console.log(`${fail === 0 ? "✅" : "❌"} 接口搜索：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
