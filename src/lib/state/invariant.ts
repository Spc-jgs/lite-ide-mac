/**
 * 运行时不变量自检 —— 可观测性的第二层（issue #27）。
 *
 * # 为什么要有
 *
 * 这个应用里有一批不变量**只活在脑子里**：日志模式的标签必须有引擎句柄、
 * 编辑模式的标签不该拿着句柄、`dirty` 与 `draft` 必须同真同假、
 * 两个标签不能共用一个句柄。它们全都是「写的时候知道、过三个月就忘了」的那种。
 *
 * `stashed` 那个 bug（`c16f12d`）正是这个形状 —— 它当时不满足的是
 * 「快照写回去不该改变 mode」，现象是「按了切换按钮什么都没发生」，
 * 而**它是靠用户截图才被发现的**。第一层（`crates/applog`）能收住异常，
 * 但这类 bug 不抛异常：状态悄悄地不对，界面照常画。
 *
 * # 硬要求：不打断用户
 *
 * 不成立时**只写日志，不抛、不弹、不改状态**。
 * 一个会因为自检失败而崩掉的编辑器，比一个偶尔状态不对的编辑器糟得多 ——
 * 而且自检本身是新写的代码，它比被它检查的东西更可能有 bug。
 *
 * # 两条设计约束
 *
 * 1. **不在热路径上跑。** 挂在状态转换点上（切标签、开/关标签、切模式、
 *    会话恢复完成），不是每帧、不是 mousemove。整套检查是 O(标签数)，
 *    四十个标签也就几微秒，但「便宜」不是把它放进 requestAnimationFrame 的理由。
 * 2. **不刷屏。** 同一条连续失败要收敛（见 [`shouldReport`]）——
 *    否则 2MB 的 `app.log` 几秒钟就被一条 assert 灌满，把真正的错误冲走。
 *
 * # 为什么零 import
 *
 * 上报通道是**注入**的（[`setInvariantSink`]，在 `main.ts` 里接到 `app_log`），
 * 不是在这里 import IPC。这样 `tests/` 能拿裸 node 直接跑它 ——
 * 一个自己没被测过的自检器，只会把噪音写进日志。
 */

/** 一条不成立的不变量怎么送出去。默认空操作：没接通道时一个字都不该写 */
type Sink = (msg: string) => void;

let sink: Sink = () => {};

/** 每条不变量失败了几次。键是 `key`，不含 detail —— 否则每个标签各算一份，收敛就没了 */
const counts = new Map<string, number>();

export function setInvariantSink(s: Sink): void {
  sink = s;
}

/** 测试用：把计数清掉，好让同一条不变量在下一个用例里从第 1 次重新数 */
export function resetInvariants(): void {
  counts.clear();
}

/** 某条失败了几次。给测试和以后可能的「状态栏上报一个数」用 */
export function invariantCount(key: string): number {
  return counts.get(key) ?? 0;
}

/**
 * 第 n 次失败要不要写进日志：**只在 n 是 2 的幂时写**（1、2、4、8、16…）。
 *
 * 「记一次就再也不说」不行 —— 那样分不出「启动时抖了一下」和
 * 「从那以后一直不对」，而这两者的处理方式完全相反。
 * 「每次都写」也不行 —— 一条挂在切标签上的不变量，手快的人一分钟能触发几百次。
 *
 * 2 的幂两头都占：日志里最多留 log₂(n) 行，而最后那行自己带着次数
 * （「第 1024 次」），**量级直接读得出来**。
 */
export function shouldReport(n: number): boolean {
  return n > 0 && (n & (n - 1)) === 0;
}

/**
 * 断言一条不变量。**成立返回 true，不成立写日志并返回 false —— 从不抛。**
 *
 * `key` 是收敛的单位，必须是**常量文本**（不能把路径拼进去，否则每个文件
 * 各占一个计数器，二十个标签就是二十条日志）。会变的东西放 `detail`。
 */
export function invariant(ok: boolean, key: string, detail = ""): boolean {
  if (ok) return true;
  const n = (counts.get(key) ?? 0) + 1;
  counts.set(key, n);
  if (shouldReport(n)) {
    /*
     * **通道自己出事也不能往上抛。**
     *
     * 这不是顺手加的防御 —— 这一句是「不打断用户」那条硬要求唯一的实现。
     * `invariant` 跑在状态转换点上（切标签、关标签），那里抛一个异常
     * 就是「点了标签没反应」。而 sink 是**注入**的：接错一个、或者
     * `main.ts` 那边换了写法忘了 catch，这里就得替它兜住。
     * 验红时确认过：去掉这层 try，测试里那条「通道坏掉」当场变红。
     */
    try {
      sink(`${key}${detail ? ` —— ${detail}` : ""}${n > 1 ? `（第 ${n} 次）` : ""}`);
    } catch {
      /* 记录失败就记录失败，不能再连累调用方 */
    }
  }
  return false;
}

// ─────────────────────────── 被检查的那些不变量 ───────────────────────────

/** `App.svelte` 的 `TabState` 里自检用得着的那几个字段 */
export interface TabLike {
  id: number;
  path: string;
  mode: "edit" | "log" | "diff" | "merge";
  dirty: boolean;
  handle?: number;
  content?: string;
  draft?: string;
}

/** 只取最后一段。`key` 要常量，路径全文进 detail 也太长 */
const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/**
 * 一个标签自己的不变量。返回 `[key, detail]` 对，空数组表示都成立。
 *
 * `live` = 这个标签此刻有没有挂着编辑器。**它不是可有可无的参数**：
 * 编辑器的 `onChange` 只改 `dirty`，`draft` 要等换文件/销毁时 `onStash`
 * 才回写（`{#key active.id}` 那套）。也就是说**正在被编辑的那个标签
 * 本来就会短暂地 dirty 而无 draft** —— 把它算进去，这条不变量会在
 * 每一次敲键盘时报假警。
 */
export function tabFaults(t: TabLike, live: boolean): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  const who = `${base(t.path)} #${t.id}`;

  // 日志模式没句柄 = 空白面板。doSwitch 的 catch 分支真能走到这儿：
  // closeLog 成功之后 readText 抛了，mode 还停在 log，而句柄已经没了
  if (t.mode === "log" && t.handle === undefined) {
    out.push(["日志模式的标签没有引擎句柄", who]);
  }
  // 反过来是 mmap 句柄泄漏：文件一直被映射着，进程内存不降（issue #10 那条预算）
  if (t.mode !== "log" && t.handle !== undefined) {
    out.push(["非日志模式的标签还拿着引擎句柄", `${who} mode=${t.mode}`]);
  }
  // dirty 与 draft 必须同真同假，否则「有未保存改动」这件事在说谎：
  // 说真话该拦的 ⌘W 不拦，说假话则拦一个其实没改动的标签
  if (!live && t.dirty !== (t.draft !== undefined)) {
    out.push([
      "dirty 与 draft 不同真同假",
      `${who} dirty=${t.dirty} draft=${t.draft === undefined ? "无" : "有"}`,
    ]);
  }
  // `stashed` 的后置条件：草稿和盘上那份一样时它会把草稿丢掉。
  // 留着一份「和磁盘相同的草稿」，基线判断从此多一层拐弯
  if (t.draft !== undefined && t.draft === (t.content ?? "")) {
    out.push(["草稿和磁盘内容相同却没被丢掉", who]);
  }
  return out;
}

/**
 * 整组标签之间的不变量。
 *
 * 注意这里**没有**「恢复出来的标签数 = 存进去的数」那一条 ——
 * 它不是不变量：`restoreSession` 明确是「能恢复多少算多少」，
 * 上次开着的文件这次被删了就是会少一个，那是对的行为。
 * 真正的不变量是反方向的那半边：**不能凭空多出来**（去重失效），
 * 由下面「路径重复」这条盖住。
 */
export function tabsFaults(
  tabs: TabLike[],
  activeId: number | null,
): Array<[string, string]> {
  const out: Array<[string, string]> = [];

  const paths = new Set<string>();
  const handles = new Map<number, number>();
  for (const t of tabs) {
    if (paths.has(t.path)) out.push(["同一个文件开了两个标签", base(t.path)]);
    paths.add(t.path);
    if (t.handle === undefined) continue;
    // 共用句柄的两个标签，关掉任意一个另一个当场变空白 —— 而且第二次
    // close_log 会打在一个已经回收的句柄上
    const prev = handles.get(t.handle);
    if (prev !== undefined) {
      out.push(["两个标签共用一个引擎句柄", `#${prev} 和 #${t.id} 都是 ${t.handle}`]);
    }
    handles.set(t.handle, t.id);
  }

  if (activeId !== null && !tabs.some((t) => t.id === activeId)) {
    out.push(["activeId 指向一个不存在的标签", `id=${activeId}`]);
  }
  // 反过来：标签条上有东西而内容区空着。`doClose` 只在关掉最后一个时
  // 把 activeId 置空，所以这两件事必须同时发生
  if (activeId === null && tabs.length > 0) {
    out.push(["有标签但没有活动标签", `${tabs.length} 个`]);
  }
  return out;
}

/**
 * 在一个状态转换点上把上面两组都跑一遍。
 *
 * `where` 只进日志不进 `key` —— 同一条不变量在切标签和关标签上各报一次，
 * 那是同一个 bug，不该占两个计数器。
 */
export function audit(
  tabs: TabLike[],
  activeId: number | null,
  liveId: number | null,
  where: string,
): void {
  for (const t of tabs) {
    for (const [key, detail] of tabFaults(t, t.id === liveId && t.mode === "edit")) {
      invariant(false, key, `${detail} @${where}`);
    }
  }
  for (const [key, detail] of tabsFaults(tabs, activeId)) {
    invariant(false, key, `${detail} @${where}`);
  }
}
