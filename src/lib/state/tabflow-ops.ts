import { probePath, readText, openLog, closeLog, scratchDir, pickFolder, trashEntry, revealInFinder } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";
import { project } from "./project.svelte";
import { scratches } from "./scratches.svelte";
import { files } from "./files.svelte";
import { tabflow, CONFIRM_EDIT_BYTES } from "./tabflow.svelte";
import type { TabState } from "./tab";

/**
 * 标签流程里的**动作**：开文件夹 / 最近项目、关项目、切模式、批量关闭、草稿进废纸篓。
 * 状态和首屏要跑的那几条在 `tabflow.svelte.ts`，这里只有函数。
 *
 * # 为什么分开放（入口包瘦身，2026-09-21）
 *
 * 判据同 `git-ops.ts`：**入口包是首屏之前必须解析执行完的那一段**。这里每一条都要先
 * 有人点了什么才跑；分屏（issue #35）把入口顶过了 138 KiB 的告警线，这些正好是
 * `tabflow.svelte.ts` 里体积最大、又一条都不在启动路径上的部分。`tabflow.svelte.ts`
 * 里每个动作是一行转发（`(await ops()).x(...)`），调用方一个字不用改 —— 它们本来就
 * 全是 `void tabflow.x()` 或者已经在 `await`。
 *
 * 循环引用是**有意的**：这里静态 import `tabflow`，那边动态 import 这里。
 * 动态的那条边不参与模块初始化顺序，所以不是环。
 *
 * 逻辑从 `tabflow.svelte.ts` 搬过来，一个字不改；`this.` 换成 `tabflow.`。
 */

/**
 * 关闭项目（issue #40 第三层）：回到「只有标签、没有项目」的轻窗口。
 *
 * 和切项目是同一套：旧项目的现场存到它自己那份、关掉它的**干净**标签
 * （脏的留着，自己会在关的时候问）、草稿不动。VS Code 的 Close Folder 也是
 * 这个语义。没有项目时什么都不做。
 */
export function closeProject() {
  const old = project.root;
  if (!old) return;
  notify.clear();
  tabflow.hooks.beforeRootChange?.(old, "");
  project.root = null;
  for (const t of [...tabs.list]) if (!t.dirty && !project.isScratch(t.path)) tabflow.doClose(t);
  tabs.audit("关闭项目");
}

/**
 * 开原生的选择文件夹面板。取消了什么也不做。
 *
 * 选中之后走的是 `openPath` —— 它对目录的处理就是把 `project.root` 设过去，
 * 和拖一个文件夹进来、命令行传目录**是同一条路**。
 * 另起一套的话，「切项目要不要清掉旧标签」这类判断就会有两份。
 */
export async function openFolder() {
  const dir = await pickFolder().catch(() => null);
  if (!dir) return;
  await tabflow.openPath(dir);
}

/**
 * 从菜单里选一个最近项目。
 *
 * **不预先探测存在性。** 每次开菜单去 stat 一遍 8 个路径，
 * 碰上没挂载的网络卷会把菜单卡住 —— 改成点了才发现：
 * 打不开就报一句并把它从列表里摘掉，那时用户已经知道自己在等什么了。
 */
export async function openRecent(dir: string) {
  const info = await probePath(dir).catch(() => null);
  if (info?.kind !== "dir") {
    notify.fail(`打不开 ${dir} —— 已从最近记录里移除`, 3200);
    project.recent = project.recent.filter((r) => r !== dir);
    return;
  }
  await tabflow.openPath(dir);
}

/**
 * 在 Finder 里显示草稿目录。
 *
 * 这条原来是「把草稿目录当项目根打开」—— 零新代码就有文件树、⌘P、⇧⌘F，
 * 但代价是**切走当前项目**：翻一条笔记要换工作区，多数人选不翻（issue #40）。
 * 翻草稿现在是侧边栏的「草稿」视图；这条只剩「去 Finder 里整理」这一个用途，
 * 而草稿目录在 Finder 里默认看不见（「资源库」是隐藏的），所以它还值得留着。
 *
 * 目录不存在**不是错误**，是「你还一条都没记过」—— 报一句红字会让人以为坏了。
 */
export async function openScratchDir() {
  notify.clear();
  try {
    const dir = await scratchDir();
    if (!(await probePath(dir).catch(() => null))) {
      notify.ok("还没有草稿 —— ⌘N 记第一条", 2600);
      return;
    }
    await revealInFinder(dir);
  } catch (e) {
    notify.fail(String(e));
  }
}

/**
 * 从草稿列表里把一份移到废纸篓（issue #40）。走的是文件树那条现成的废纸篓路径 ——
 * 应用里没有第二条删除路径。开着的标签一起关掉：留一个指向废纸篓里文件的标签，
 * 下次 ⌘S 会把它原地复活。
 */
export async function trashScratch(path: string) {
  notify.clear();
  try {
    /*
     * 开着且脏的先落盘再移：移走的是盘上那份，编辑器里 500ms 内还没写下去的字
     * 要跟着一起进废纸篓 —— 「放回原处」放回来的才是完整的。写不成就停下问，
     * 别一边报错一边把文件挪走。
     */
    for (const t of tabs.under(path, false)) {
      if (t.dirty && !(await docs.autosaveBeforeClose(t))) {
        tabs.show(t.id);
        tabflow.pendingClose = t;
        notify.fail("这份草稿有没写进盘的改动，先处理它再移到废纸篓", 3200);
        return;
      }
    }
    await trashEntry(path);
    for (const t of tabs.under(path, false)) tabflow.doClose(t);
    files.forget(path);
    void scratches.refresh();
  } catch (e) {
    notify.fail(String(e));
  }
}

export function requestSwitchMode(tab: TabState) {
  if (tab.dirty) {
    notify.fail("有未保存的改动，请先保存（⌘S）再切换模式", 2600);
    return;
  }
  const to = tab.mode === "edit" ? "log" : "edit";
  // 切到日志模式没有风险（mmap，内存与大小无关）；反方向要看体积
  if (to === "edit" && tab.size > CONFIRM_EDIT_BYTES) {
    tabflow.pendingSwitch = tab;
    return;
  }
  void tabflow.doSwitch(tab, to);
}

export async function doSwitch(tab: TabState, to: "edit" | "log") {
  tabflow.pendingSwitch = null;
  notify.clear();
  try {
    if (tab.mode === "log" && tab.handle !== undefined) {
      await closeLog(tab.handle);
      tab.handle = undefined;
    }
    if (to === "log") {
      tab.handle = (await openLog(tab.path)).handle;
      tab.content = undefined;
    } else {
      const t = await readText(tab.path, tab.forced ? tab.encoding : undefined);
      tab.content = t.content;
      tab.encoding = t.encoding;
      tab.bom = t.bom;
      tab.eol = t.eol;
      tab.lossy = t.lossy;
    }
    tab.mode = to;
    tab.forced = to;
    tabs.audit("切模式");
  } catch (e) {
    notify.fail(String(e));
    // 切换失败要退回原状态，否则标签会停在一个既没句柄也没内容的空壳上
    if (tab.mode === "log" && tab.handle === undefined) {
      try {
        tab.handle = (await openLog(tab.path)).handle;
      } catch {
        /* 连回退都失败，只能让用户重开 */
      }
    }
  }
}

/**
 * 关掉一批标签。干净的直接关，有改动的排队逐个问。
 *
 * **不能直接全关**：标签栏的「关闭其他 / 关闭右侧 / 关闭全部」一按下去，
 * 可能带走好几个正在改的文件，而它们的改动没有任何地方找得回来
 * （不像删文件还进废纸篓）。
 */
export function closeMany(ids: number[]) {
  const dirty: number[] = [];
  for (const id of ids) {
    const t = tabs.byId(id);
    // 批量关闭一律跳过钉住的：「关闭其他 / 右侧 / 全部」正是钉住要防的那几下
    if (!t || t.pinned) continue;
    if (t.dirty) dirty.push(id);
    else tabflow.doClose(t);
  }
  tabflow.closeQueue = dirty;
  askNextClose();
}

/** 从队列里取下一个来问；队列空了就把横幅收掉 */
function askNextClose() {
  while (tabflow.closeQueue.length) {
    const id = tabflow.closeQueue[0];
    tabflow.closeQueue = tabflow.closeQueue.slice(1);
    const t = tabs.byId(id);
    if (!t) continue; // 中途被别处关掉了
    /*
     * 草稿先自己写一次（issue #40）：写成了就直接关、接着问下一个，
     * 和单个 ⌘W 走的 `requestClose` 是同一条规矩 —— 「关闭全部」里夹着一份
     * 草稿不该突然弹出来问。写不成才问。
     */
    if (project.isScratch(t.path)) {
      void docs.autosaveBeforeClose(t).then((saved) => {
        const again = tabs.byId(id);
        if (again && saved && !again.dirty) {
          tabflow.doClose(again);
          askNextClose();
          return;
        }
        if (again) {
          tabs.show(again.id);
          tabflow.pendingClose = again;
        } else {
          askNextClose();
        }
      });
      return;
    }
    tabs.show(t.id); // 让人看见要丢的到底是什么
    tabflow.pendingClose = t;
    return;
  }
  tabflow.pendingClose = null;
}

/**
 * 「保存并关闭 / 丢弃改动 / 取消」三个按钮的落点。
 *
 * 取消**把整批都停掉**，不是只跳过这一个：连着弹五次确认框、每次都得
 * 再点一次取消，比没有批量关闭还烦人。
 */
export async function resolveClose(kind: "save" | "discard" | "cancel") {
  const t = tabflow.pendingClose;
  if (!t) return;
  if (kind === "cancel") {
    tabflow.closeQueue = [];
    tabflow.pendingClose = null;
    return;
  }
  if (kind === "save") {
    tabs.show(t.id);
    // 写失败就停在这儿，别往下关 —— 关了改动就真没了
    if (!(await docs.save(docs.liveText(t)))) {
      tabflow.closeQueue = [];
      return;
    }
  }
  tabflow.doClose(t);
  askNextClose();
}
