/** `${…}` 里的配置键（src/lib/editor/spring-key.ts，docs/IDEA.md 第 25 节）：光标在哪个里、去掉默认值、嵌套取最里面 */
import { configKeyAt } from "../src/lib/editor/spring-key.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const at = (s: string, marker = "|") => configKeyAt(s.replace(marker, ""), s.indexOf(marker));

ok(at(`@Value("\${demo.gre|eting}")`) === "demo.greeting", "光标在键上");
ok(at(`@Value("|\${demo.greeting}")`) === "demo.greeting", "光标在 $ 上也算");
ok(at(`@Val|ue("\${demo.greeting}")`) === null, "在 @Value 上不算（交回原来的跳转）");
ok(at(`@Value("\${server.port:80|80}")`) === "server.port", "去掉默认值");
ok(at(`@Value("\${a.b}-\${c.|d}")`) === "c.d", "两个里光标在第二个");
ok(at(`@Value("\${outer:\${inner.ke|y}}")`) === "inner.key", "嵌套：最里面那个");
ok(at(`@Value("\${out|er:\${inner.key}}")`) === "outer", "嵌套：光标在外面那个的键上");
ok(at(`String s = "\${unclosed|";`) === null, "没配对的 } 不算");

console.log(`${fail === 0 ? "✅" : "❌"} 配置键：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
