import { detectEncoding } from "../ipc/commands";
import { openMerged as ipcOpenMerged, pickFiles } from "../ipc/log";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { tabflow } from "./tabflow.svelte";
import { project } from "./project.svelte";
import { mergePath, mergePaths, mergeTitle, shortNames } from "../logview/merge-path";

/**
 * 几份日志按时间合成一个标签（docs/MERGE.md）。按需加载：要先有人在文件树里多选了日志、
 * 或者在日志视图里点了「合并…」才用得上 —— 判据同 `tabflow-ops.ts`（入口包是首屏之前必须跑完的那一段）。
 *
 * 入口有四个，都走 `openMerged`：
 * - 文件树多选几份 → 右键「按时间合并查看」
 * - 日志视图里点「合并…」→ 选别处的文件（从服务器拉下来的多半在「下载」里，不在项目里）
 * - 合并视图里点「加一份…」→ 新开一个合并标签，原来那个关掉
 * - 会话恢复：快照里记的是 `merge:` 开头的路径，`tabflow.openPath` 认出来转到这里
 */

/** 合并上限，和 Rust 侧 `logengine::merge::MAX_SOURCES` 一致（过滤时「只看哪几份」是一个 u32 的位掩码） */
const MAX = 32;

export async function openMerged(paths: string[], opts: { quiet?: boolean; replace?: number } = {}) {
  // 同一份选了两次（文件树里选了、面板里又选了一遍）：去掉，保持先后
  const list = [...new Set(paths)];
  if (list.length < 2) {
    if (!opts.quiet) notify.hint("合并至少需要两份日志");
    return;
  }
  if (list.length > MAX) {
    if (!opts.quiet) notify.fail(`最多可合并 ${MAX} 份，当前选择了 ${list.length} 份`);
    return;
  }
  const key = mergePath(list);
  if (!key) {
    if (!opts.quiet) notify.fail("无法合并：文件路径中包含换行符");
    return;
  }
  const exist = tabs.byPath(key);
  if (exist) {
    if (!tabflow.restoringTabs) tabs.show(exist.id);
    return;
  }
  try {
    const info = await ipcOpenMerged(list);
    // 编码按第一份认：几份日志多半出自同一套部署，编码一样。真不一样的，状态栏那格照样能改（`reopenWith` 对日志只换解码标签）
    const encoding = await detectEncoding(list[0]).catch(() => "UTF-8");
    const replaced = opts.replace !== undefined ? tabs.byId(opts.replace) : null;
    const id = tabs.add(
      {
        path: key,
        name: mergeTitle(shortNames(list)),
        mode: "log",
        dirty: false,
        reason: `${list.length} 份日志按时间合并`,
        size: info.size,
        handle: info.handle,
        encoding,
        merge: info,
      },
      replaced ? tabs.list.indexOf(replaced) : undefined,
    );
    // 「加一份」：新的落在旧的那一格，旧的关掉（走 doClose：句柄要还）。先加后关，理由同预览标签顶掉预览标签
    if (replaced) tabflow.doClose(replaced);
    if (!tabflow.restoringTabs) {
      tabs.show(id);
      tabs.audit("合并日志");
    }
  } catch (e) {
    if (!opts.quiet) notify.fail(String(e));
  }
}

/** 会话恢复：`merge:` 开头的路径 → 几份文件。有一份已经不在盘上了，整个合并标签就不恢复（Rust 打不开会报错，这里静默） */
export async function openMergedPath(path: string, opts: { quiet?: boolean } = {}) {
  const list = mergePaths(path);
  if (!list) return;
  await openMerged(list, opts);
}

/**
 * 选几份别处的日志，和 `base` 合在一起。`replace` 是要被顶掉的那个合并标签（「加一份」）；
 * 从单文件日志点「合并…」时不顶：那个标签还要留着看
 */
export async function pickAndMerge(base: string[], replace?: number) {
  const dir = base[0] ? base[0].slice(0, base[0].lastIndexOf("/")) || "/" : project.root;
  const picked = await pickFiles(base.length > 1 ? "再加几份日志一起合" : "选几份日志和它按时间合并", dir).catch(() => [] as string[]);
  if (picked.length === 0) return;
  await openMerged([...base, ...picked], { replace });
}
