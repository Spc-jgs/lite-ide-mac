/** Spring Boot 配置文件里光标在哪个键上（src/lib/editor/spring-path.ts，docs/TAKEOVER.md G30）：补全要父键、悬停要整个键名 */
import { propsKeyAt, propsSpot, yamlKeyAt, yamlParents, yamlSpot } from "../src/lib/editor/spring-path.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}`);

// 注释顶格写在 spring: 和 datasource: 中间：它比 datasource 浅，往上找父键时一定会碰到它 —— 不跳过注释就断在这儿
const yml = `spring:
# 数据源
  datasource:
    url: jdbc:mysql://h/db

    hikari:
      maximum-pool-size: 10
  profiles:
    active: dev
server:
  port: 8080
list:
  - name: a
    port: 1
doc: |
  not: a key
---
other:
  key: x`.split("\n");
eq(yamlParents(yml, 3), ["spring", "datasource"], "隔着注释找到父键");
eq(yamlParents(yml, 6), ["spring", "datasource", "hikari"], "隔着空行、往上三层");
eq(yamlParents(yml, 8), ["spring", "profiles"], "兄弟分支：缩进回到 profiles 那层");
eq(yamlParents(yml, 10), ["server"], "顶层的另一棵");
eq(yamlParents(yml, 9), [], "顶层键没有父键");
eq(yamlParents(yml, 13), [], "列表项里面的不认父键（那是一组对象）");
eq(yamlParents(yml, 15), [], "多行字面量里面的不当键");
eq(yamlParents(yml, 18), ["other"], "--- 之后是新文档");
eq(yamlKeyAt(yml, 6), "spring.datasource.hikari.maximum-pool-size", "悬停：整个键名");
eq(yamlKeyAt(yml, 1), null, "注释不是键");

eq(yamlSpot("  data"), { kind: "key", typed: "data" }, "写键");
eq(yamlSpot(""), { kind: "key", typed: "" }, "空行也能弹");
eq(yamlSpot("    port: 80"), { kind: "value", key: "port", typed: "80" }, "写值");
eq(yamlSpot("    url: jdbc:my"), { kind: "value", key: "url", typed: "jdbc:my" }, "值里有冒号的（URL）照样认成在写 url 的值（没有可选值，什么都不弹）");
eq(propsSpot("server.po"), { kind: "key", typed: "server.po" }, "properties 写键");
eq(propsSpot("spring.main.banner-mode=co"), { kind: "value", key: "spring.main.banner-mode", typed: "co" }, "properties 写值");
eq(propsSpot("# server.port"), null, "注释里不弹");
eq(propsKeyAt("server.port = 8080"), "server.port", "properties 悬停的键");

console.log(`${fail === 0 ? "✅" : "❌"} 配置文件的键：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
