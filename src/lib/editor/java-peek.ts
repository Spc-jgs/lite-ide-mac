/**
 * 成员跳转要看**别的文件**里有什么：`repo.findById()` 得知道 `OrderRepository.java` 里
 * 有没有 `findById`、在第几行。这里读那个文件、解析、缓存它的成员表。
 *
 * # 为什么是「缓存 + 回头补下划线」
 *
 * 下划线的判定是同步的 —— ⌘hover 鼠标每动一格就问一次（jump.ts 头上有说明），
 * 中间隔一次 IPC，下划线跟不上鼠标。而读文件是异步的。所以：
 *
 * - 缓存里有 → 当场答
 * - 没有 → 答「还不知道」（`undefined`，调用方不画下划线），后台去读；读回来调 `onReady`，
 *   编辑器在鼠标还停着的地方重问一次，下划线补上。实测是几毫秒的事，看不出先后。
 *
 * **还不知道的时候不画**，而不是先画上、点了再说 —— 「有下划线 = 这一下我确定」
 * 是跳转唯一的承诺（jump.ts），点下去才发现那个类里没这个方法（继承来的），就破了。
 *
 * # 新鲜度
 *
 * 条目 5 秒后算旧：旧的照样先拿来答，同时后台重读。5 秒内改了目标文件，最坏是下划线
 * 按旧的画、跳到旧的行号附近 —— 比每次 hover 都读一遍盘划算。目标文件如果开在某个标签里
 * 还没保存，读到的是盘上那份，行号可能差几行（同样的取舍）。
 *
 * 这个文件跟着 Java 语言包懒加载（langs-load.ts），经 languageData 交给 jump.ts。
 */
import { readText } from "../ipc/commands";
import { project } from "../state/project.svelte";
import { membersFromSource, type PeekMembers } from "./java-scope";

const TTL = 5_000;
/** 条目上限：追着 ⌘B 走几十个文件也就这么多，再多按最久没用的扔 */
const MAX = 64;
/** 比这大的不当源码读（生成的巨型文件） */
const MAX_BYTES = 2_000_000;

const cache = new Map<string, { at: number; members: PeekMembers | null }>();
const inflight = new Map<string, Promise<void>>();
const waiters = new Map<string, Set<() => void>>();

async function load(rel: string) {
  let members: PeekMembers | null = null;
  try {
    const root = project.root;
    // 只认 .java：同包 / import 的候选里还有 .kt / .scala，拿 Java 的语法去解析它们是在猜
    if (root && rel.endsWith(".java")) {
      const f = await readText(`${root}/${rel}`);
      if (f.content.length <= MAX_BYTES) {
        const cls = rel.slice(rel.lastIndexOf("/") + 1, -".java".length);
        members = membersFromSource(f.content, cls);
      }
    }
  } catch {
    members = null; // 读不了（删了、没权限）就是「这个类里没东西可跳」，不报错
  }
  cache.delete(rel);
  cache.set(rel, { at: Date.now(), members });
  if (cache.size > MAX) cache.delete(cache.keys().next().value!);
  inflight.delete(rel);
  const ws = waiters.get(rel);
  waiters.delete(rel);
  ws?.forEach((f) => f());
}

/**
 * 这个文件（相对项目根）的成员表。
 * `undefined` = 还在读（读完调 `onReady`）；`null` = 读过了，不是一个能跳的类。
 */
export function peekMembers(rel: string, onReady?: () => void): PeekMembers | null | undefined {
  const hit = cache.get(rel);
  if (hit && Date.now() - hit.at < TTL) return hit.members;
  if (onReady) {
    const ws = waiters.get(rel) ?? new Set();
    ws.add(onReady);
    waiters.set(rel, ws);
  }
  // 先登记再开始读（`.then` 推到下一拍）：`load` 在没有项目根 / 不是 .java 时一次 await 都不经过，
  // 直接调的话它同步跑完、先 `inflight.delete`，这里才 `set` —— 那一条就永远挂在「在读」上了
  if (!inflight.has(rel)) inflight.set(rel, Promise.resolve().then(() => load(rel)));
  return hit ? hit.members : undefined;
}

/** 等手上在读的都读完。⌘B 用：键盘没有「鼠标停着」可补，得等答案回来再跳 */
export function settlePeeks(): Promise<void> | null {
  return inflight.size ? Promise.all(inflight.values()).then(() => undefined) : null;
}
