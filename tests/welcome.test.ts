/**
 * 欢迎页的纯逻辑（src/lib/welcome/welcome.ts，docs/TAKEOVER.md G27）：路径怎么缩写、筛选怎么排、克隆框什么时候能点。
 */
import { baseName, cloneProblem, defaultParent, filterRecent, initial, tildify } from "../src/lib/welcome/welcome.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

eq(tildify("/Users/me/work/a"), "~/work/a", "家目录缩成 ~");
eq(tildify("/Users/me"), "~", "家目录本身");
eq(tildify("/Users/meow2/x"), "~/x", "名字整段匹配");
eq(tildify("/Volumes/disk/proj"), "/Volumes/disk/proj", "别处原样");
eq(baseName("/a/b/order-service"), "order-service", "名字是最后一段");
eq(initial("/a/订单"), "订", "中文名取第一个字");
eq(initial("/a/lite"), "L", "首字母大写");

const R = ["/Users/me/lite/notes", "/Users/me/work/lite-ide", "/Users/me/work/gateway", "/Users/me/old/Billing"];
eq(filterRecent(R, ""), R, "空的不筛");
// 名字里命中的排在只有路径命中的前面：输 lite 先看到 lite-ide，住在 ~/lite/ 下面的 notes 排后面
eq(filterRecent(R, "lite"), ["/Users/me/work/lite-ide", "/Users/me/lite/notes"], "名字命中在前");
eq(filterRecent(R, "BILL"), ["/Users/me/old/Billing"], "不分大小写");
eq(filterRecent(R, "work g"), ["/Users/me/work/gateway"], "几段都要有");
eq(filterRecent(R, "~/old"), ["/Users/me/old/Billing"], "按缩写过的路径筛（界面上看到的就是这个）");

eq(defaultParent(["/Users/me/work/a", "/x/b"]), "/Users/me/work", "最近那个项目的上一级");
eq(defaultParent([]), "", "没有最近的就空着");
eq(defaultParent(["/proj"]), "", "在根目录下的不拿 / 当默认");

eq(cloneProblem("", "/p", "a"), "", "还没填地址时不唠叨");
eq(cloneProblem("--upload-pack=x", "/p", "a"), "这不像一个仓库地址", "像开关的地址");
eq(cloneProblem("https://x/a.git", "", "a"), "选一个放它的目录", "没选目录");
eq(cloneProblem("https://x/a.git", "/p", ""), "起一个目录名", "没目录名");
eq(cloneProblem("https://x/a.git", "/p", "../a"), "目录名里不能有 /", "目录名带路径");
eq(cloneProblem("https://x/a.git", "/p", "a"), "", "都齐了");

console.log(fail === 0 ? `✅ 欢迎页：${pass} 通过，0 失败` : `❌ 欢迎页：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
